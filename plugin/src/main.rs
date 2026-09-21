// SDK macro uses pre-2024 #[no_mangle], hence this thin package is edition 2021.
#[cfg(any(target_family = "wasm", test))]
mod wasm {
    #[cfg(test)]
    use self::tests::{change_host_folder, now, read_dir, send, set_timeout};
    #[cfg(not(test))]
    use serde::{Deserialize, Serialize};
    #[cfg(not(test))]
    use std::fs::read_dir;
    use std::time::{Duration, Instant};
    #[cfg(not(test))]
    use zellij_launchpad::workers::Engine;
    use zellij_launchpad::{
        workers::{Reply, Request},
        State,
    };
    use zellij_launchpad_core::app::{Action, App, Tool};
    use zellij_launchpad_core::remote::RemoteRequest;
    use zellij_tile::prelude::*;
    const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);
    #[cfg(not(test))]
    fn now() -> Instant {
        Instant::now()
    }
    #[cfg(not(test))]
    register_plugin!(Plugin);
    #[cfg(not(test))]
    register_worker!(IndexWorker, index_worker, INDEX);
    #[cfg(not(test))]
    #[derive(Default, Serialize, Deserialize)]
    struct IndexWorker {
        engine: Engine,
        #[cfg(feature = "worker-faults")]
        silenced: bool,
    }
    #[cfg(not(test))]
    impl ZellijWorker<'_> for IndexWorker {
        fn on_message(&mut self, _name: String, payload: String) {
            #[cfg(feature = "worker-faults")]
            {
                if _name == "silence" {
                    self.silenced = true;
                }
                if self.silenced {
                    return;
                }
            }
            if let Ok(request) = serde_json::from_str::<Request>(&payload) {
                // Only Start needs the host handshake (a synchronous host call).
                let cwd = if matches!(request, Request::Start { .. }) {
                    get_plugin_ids()
                        .initial_cwd
                        .to_str()
                        .unwrap_or("")
                        .to_owned()
                } else {
                    String::new()
                };
                let (reply, continuation) = self.engine.dispatch(request, &cwd);
                if let Some(reply) = reply {
                    post_message_to_plugin(PluginMessage::new_to_plugin(
                        "index-reply",
                        &serde_json::to_string(&reply).unwrap(),
                    ));
                }
                if let Some(continuation) = continuation {
                    send(continuation);
                }
            }
        }
    }
    #[cfg(not(test))]
    fn send(request: Request) {
        post_message_to(PluginMessage::new_to_worker(
            "index",
            "request",
            &serde_json::to_string(&request).unwrap(),
        ));
    }
    #[cfg(not(test))]
    pub fn initialize() {
        main();
    }
    #[derive(Default)]
    struct Plugin {
        state: State,
        initial_cwd: String,
        original_cwd: Option<String>,
        home: Option<String>,
        epoch: u64,
        ready: bool,
        scanning: bool,
        pending: Option<Instant>,
        timer_due: Option<Instant>,
        search_due: Option<Instant>,
        remounting: bool,
        // Host remounts have no request IDs. Keep the outstanding operation
        // across App resets; its epoch decides whether the result is still live.
        cwd_validation: Option<(u64, u64)>,
        failed: bool,
        simulate_launch: bool,
        launch_serial: u64,
        history_serial: u64,
        history_attempt: Option<String>,
        history_rows: Vec<zellij_launchpad::history::Entry>,
        launch_context: Option<std::collections::BTreeMap<String, String>>,
        renamed_tab: Option<(usize, String, String)>,
        work: Option<(u64, Instant)>,
        #[cfg(feature = "worker-faults")]
        silence_worker: bool,
    }
    impl Plugin {
        fn arm_timer(&mut self, deadline: Instant, now: Instant) {
            // Timers have no IDs and cannot be cancelled. An earlier wakeup may
            // supersede the watchdog; late callbacks must not clear a newer timer.
            if self.timer_due.is_none_or(|due| deadline < due) {
                set_timeout(
                    deadline
                        .saturating_duration_since(now)
                        .as_secs_f64()
                        .max(0.001),
                );
                self.timer_due = Some(deadline);
            }
        }
        fn reject_launch(&mut self) {
            if let Some((id, previous, written)) = self.renamed_tab.take() {
                // Do not undo a later name observed from the user/another plugin.
                // The host has no compare-and-swap rename: this check and write
                // are separate screen instructions (documented concurrency limit).
                if get_tab_info(id).is_some_and(|tab| tab.name == written) {
                    rename_tab_with_id(id as u64, &previous);
                    if !get_tab_info(id).is_some_and(|tab| tab.name == previous) {
                        eprintln!("Launchpad: tab-name rollback could not be confirmed");
                    }
                }
            }
            self.state.app.launch_rejected();
            if let Some(attempt) = self.history_attempt.take() {
                let (token, _) = self.history_token();
                match zellij_launchpad::history::Store::new(std::path::Path::new("/cache"))
                    .remove(&attempt, &token)
                {
                    Ok(()) => self.load_history(),
                    Err(error) => {
                        self.state.app.message = Some(format!(
                            "Zellij did not accept the launch. History rollback failed: {error}"
                        ));
                    }
                }
            }
        }
        fn load_history(&mut self) {
            if self.simulate_launch {
                return;
            }
            let (token, _) = self.history_token();
            let rows = match zellij_launchpad::history::Store::new(std::path::Path::new("/cache"))
                .refresh(&token)
            {
                Ok(rows) => rows,
                Err(error) => {
                    self.state.app.message = Some(format!("History unavailable: {error}"));
                    // F5's worker handshake recreates App before reaching here.
                    // Restore the last successful rows, not that empty App's list.
                    self.history_rows.clone()
                }
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.state.app.history = rows
                .iter()
                .enumerate()
                .map(|(id, row)| zellij_launchpad_core::app::Launch {
                    id: id as u64,
                    path: row.path.clone(),
                    tool: row.tool,
                    age: zellij_launchpad::history::age(row.opened_at, now),
                })
                .collect();
            self.state.app.recent = self.state.app.recent.min(rows.len().saturating_sub(1));
            self.history_rows = rows;
        }
        fn mutate_history(&mut self, mutation: zellij_launchpad_core::app::HistoryMutation) {
            use zellij_launchpad_core::app::HistoryMutation;
            let (token, _) = self.history_token();
            let store = zellij_launchpad::history::Store::new(std::path::Path::new("/cache"));
            let result = match mutation {
                HistoryMutation::Clear => store.clear(&token),
                HistoryMutation::Remove(id) => self
                    .history_rows
                    .get(id as usize)
                    .ok_or_else(|| "History selection changed; refresh and retry".to_string())
                    .and_then(|row| store.remove(&row.id, &token)),
            };
            match result {
                Ok(()) => {
                    self.state.app.message = None;
                    self.load_history();
                }
                Err(error) => {
                    self.state.app.message = Some(format!("History change failed: {error}"))
                }
            }
        }
        fn history_token(&mut self) -> (String, u64) {
            self.history_serial += 1;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let ids = get_plugin_ids();
            (
                format!(
                    "{:039}-{}-{}-{}",
                    now.as_nanos(),
                    ids.zellij_pid,
                    ids.plugin_id,
                    self.history_serial
                ),
                now.as_secs(),
            )
        }
        fn record_history(&mut self, launch: &zellij_launchpad_core::app::Launch) {
            let (id, opened_at) = self.history_token();
            self.history_attempt = Some(id.clone());
            let entry = zellij_launchpad::history::Entry {
                id,
                path: launch.path.clone(),
                tool: launch.tool,
                opened_at,
            };
            if let Err(error) =
                zellij_launchpad::history::Store::new(std::path::Path::new("/cache")).record(entry)
            {
                eprintln!("Launchpad: history not saved: {error}");
            }
        }
        fn spawn_launch(&mut self, launch: zellij_launchpad_core::app::Launch) {
            self.record_history(&launch);
            // Target this plugin explicitly, never the last-focused pane.
            // Close rather than suppress it: command exit cannot restore it.
            match launch.tool {
                Tool::Shell => {
                    // Preserve Zellij's configured default shell and its cwd.
                    if open_terminal_in_place_of_plugin(&launch.path, true).is_none() {
                        self.reject_launch();
                    }
                }
                tool => {
                    let Some(definition) = self.state.app.commands.get(tool).cloned() else {
                        self.reject_launch();
                        return;
                    };
                    let Some(executable) = definition.executable else {
                        self.reject_launch();
                        return;
                    };
                    self.launch_serial += 1;
                    let context = std::collections::BTreeMap::from([(
                        "launchpad-launch".into(),
                        self.launch_serial.to_string(),
                    )]);
                    self.launch_context = Some(context.clone());
                    run_action(
                        actions::Action::NewInPlacePane {
                            command: Some(actions::RunCommandAction {
                                command: executable.into(),
                                args: definition.arguments,
                                cwd: Some(launch.path.into()),
                                hold_on_close: false,
                                hold_on_start: false,
                                ..Default::default()
                            }),
                            pane_name: None,
                            near_current_pane: false,
                            no_focus: false,
                            pane_id_to_replace: Some(PaneId::Plugin(get_plugin_ids().plugin_id)),
                            close_replaced_pane: true,
                            tab_id: None,
                        },
                        context,
                    );
                }
            }
        }
        fn start(&mut self) {
            self.epoch += 1;
            self.ready = false;
            self.scanning = false;
            self.work = None;
            self.search_due = None;
            self.failed = false;

            self.pending = Some(now());
            self.state.app.search_status = "Verifying worker HOME mapping…".into();
            send(Request::Start {
                epoch: self.epoch,
                home: self.home.clone().unwrap(),
            });
        }
        fn fail(&mut self, message: String) {
            self.state.app.remote_failed(message);
            self.failed = true;
            self.work = None;
            self.search_due = None;
            self.pending = None;
            self.scanning = false;
        }
    }
    impl ZellijPlugin for Plugin {
        fn load(&mut self, configuration: std::collections::BTreeMap<String, String>) {
            self.simulate_launch = configuration
                .get("simulate_launch")
                .is_some_and(|v| v == "true");
            #[cfg(feature = "worker-faults")]
            {
                self.silence_worker = configuration
                    .get("test_silence_worker")
                    .is_some_and(|v| v == "true");
            }
            self.initial_cwd = get_plugin_ids()
                .initial_cwd
                .to_str()
                .unwrap_or("")
                .to_owned();
            subscribe(&[
                EventType::Key,
                EventType::Mouse,
                EventType::PastedText,
                EventType::PermissionRequestResult,
                EventType::HostFolderChanged,
                EventType::FailedToChangeHostFolder,
                EventType::Timer,
                EventType::CustomMessage,
                EventType::ActionComplete,
            ]);
            // Capture before HOME remount; /data is instance-local across reload.
            if !self.simulate_launch {
                match zellij_launchpad::cwd::load(
                    std::path::Path::new("/data"),
                    &get_plugin_ids().initial_cwd,
                ) {
                    Ok(cwd) => self.original_cwd = Some(cwd),
                    Err(error) => {
                        self.fail(error);
                        return;
                    }
                }
            }
            self.state.app.configure(&configuration);
            self.state.app.simulate_launch = self.simulate_launch;
            self.state.app.search_status = "Waiting for HOME permissions…".into();
            let mut permissions = vec![
                PermissionType::ReadSessionEnvironmentVariables,
                PermissionType::FullHdAccess,
                PermissionType::ChangeApplicationState,
            ];
            if !self.simulate_launch {
                permissions.extend([
                    PermissionType::OpenTerminalsOrPlugins,
                    PermissionType::RunActionsAsUser,
                    PermissionType::ReadApplicationState,
                ]);
            }
            request_permission(&permissions);
        }
        fn update(&mut self, event: Event) -> bool {
            let now = now();
            let mut changed = true;
            match event {
                Event::PermissionRequestResult(PermissionStatus::Granted)
                    if self.home.is_none() =>
                {
                    let home = zellij_launchpad::session_home(&get_session_environment_variables());
                    if self.state.prepare_home(home.clone()) {
                        self.home = home;
                        if self.home.as_deref().is_some_and(|home| {
                            zellij_launchpad_core::host_path::same(home, &self.initial_cwd)
                        }) {
                            self.start();
                        } else if std::path::Path::new("/data/worker-reload-attempted").exists() {
                            self.fail("Worker HOME reload failed. Reopen the plugin.".into());
                        } else {
                            self.remounting = true;
                            self.pending = Some(now);
                            change_host_folder(self.home.clone().unwrap().into());
                        }
                    }
                }
                Event::PermissionRequestResult(PermissionStatus::Denied) => self
                    .fail("Permission denied. Reopen and allow the requested permissions.".into()),
                Event::HostFolderChanged(path)
                    if self.remounting
                        && path.to_str().zip(self.home.as_deref()).is_some_and(
                            |(path, home)| zellij_launchpad_core::host_path::same(path, home),
                        ) =>
                {
                    self.remounting = false;
                    if std::fs::write("/data/worker-reload-attempted", b"1").is_ok() {
                        self.state.app.search_status = "Reloading worker HOME mapping once…".into();
                        reload_plugin_with_id(get_plugin_ids().plugin_id);
                    } else {
                        self.fail("Cannot guard worker reload. Reopen the plugin.".into());
                    }
                }
                Event::HostFolderChanged(path)
                    if self.cwd_validation.is_some()
                        && path.to_str().zip(self.original_cwd.as_deref()).is_some_and(
                            |(path, cwd)| zellij_launchpad_core::host_path::same(path, cwd),
                        ) =>
                {
                    let (epoch, generation) = self.cwd_validation.take().unwrap();
                    // Only this exact host-supplied directory is an exception to
                    // HOME validation. No traversal or catalogue is added here.
                    changed = if epoch == self.epoch {
                        self.work = None;
                        let result = read_dir("/host")
                            .map(|_| self.original_cwd.clone().unwrap())
                            .map_err(|e| format!("Invoking directory unavailable: {e}"));
                        self.state.app.finish_remote_validation(generation, result)
                    } else {
                        false
                    };
                }
                Event::FailedToChangeHostFolder(_) if self.cwd_validation.is_some() => {
                    let (epoch, generation) = self.cwd_validation.take().unwrap();
                    changed = if epoch == self.epoch {
                        self.work = None;
                        self.state.app.finish_remote_validation(
                            generation,
                            Err("Invoking directory unavailable; no fallback.".into()),
                        )
                    } else {
                        false
                    };
                }
                Event::FailedToChangeHostFolder(_) if self.remounting => {
                    self.remounting = false;
                    self.fail("HOME filesystem unavailable. Reopen the plugin.".into())
                }
                Event::CustomMessage(name, payload) if name == "index-reply" && !self.failed => {
                    match serde_json::from_str::<Reply>(&payload) {
                        Ok(Reply::Ready { epoch, cwd })
                            if epoch == self.epoch
                                && self.home.as_deref().is_some_and(|home| {
                                    zellij_launchpad_core::host_path::same(&cwd, home)
                                }) =>
                        {
                            self.pending = Some(now);
                            self.ready = true;
                            self.scanning = true;
                            self.state.replace_app(App::from_remote(cwd.into()));
                            if let Some(cwd) = &self.original_cwd {
                                self.state.app.set_initial_cwd(cwd.clone());
                            }
                            self.state.app.simulate_launch = self.simulate_launch;
                            self.load_history();
                            #[cfg(feature = "worker-faults")]
                            if self.silence_worker {
                                post_message_to(PluginMessage::new_to_worker(
                                    "index", "silence", "",
                                ));
                            }
                        }
                        Ok(Reply::Progress {
                            epoch,
                            revision,
                            status,
                            scanning,
                        }) if epoch == self.epoch => {
                            self.pending = scanning.then_some(now);
                            self.scanning = scanning;
                            self.state.app.remote_progress(revision, status);
                        }
                        Ok(Reply::Results {
                            epoch,
                            generation,
                            revision,
                            paths,
                        }) if epoch == self.epoch
                            && self.work.is_some_and(|(g, _)| g == generation) =>
                        {
                            self.work = None;
                            changed = self.state.app.accept_remote_revision(revision)
                                && self.state.app.apply_remote_results(generation, paths);
                        }
                        Ok(Reply::Validated {
                            epoch,
                            generation,
                            result,
                        }) if epoch == self.epoch
                            && self.work.is_some_and(|(g, _)| g == generation) =>
                        {
                            self.work = None;
                            changed = self.state.app.finish_remote_validation(generation, result);
                        }
                        Ok(Reply::Failed { epoch, error }) if epoch == self.epoch => {
                            self.fail(error)
                        }
                        _ => changed = false,
                    }
                }
                Event::Timer(_) => {
                    if self.timer_due.is_some_and(|due| now >= due) {
                        self.timer_due = None;
                    }
                    changed = false;
                    if self
                        .pending
                        .is_some_and(|t| now.saturating_duration_since(t) > Duration::from_secs(15))
                        || self.work.is_some_and(|(_, t)| {
                            now.saturating_duration_since(t) > Duration::from_secs(15)
                        })
                    {
                        self.fail("Worker timed out. Reopen the plugin; no scan fallback.".into());
                        changed = true;
                    }
                }
                Event::ActionComplete(_, pane_id, context)
                    if self.launch_context.as_ref() == Some(&context) =>
                {
                    self.launch_context = None;
                    // run_action is asynchronous and returns no acceptance result.
                    // A successful replacement normally destroys us before this
                    // event. None means no affected pane, not an agent exit code.
                    if pane_id.is_none() {
                        self.reject_launch();
                    }
                }
                Event::HostFolderChanged(_)
                | Event::FailedToChangeHostFolder(_)
                | Event::PermissionRequestResult(_)
                | Event::ActionComplete(..) => changed = false,
                event => {
                    let quit = matches!(&event, Event::Key(key) if zellij_launchpad::key_action(key.clone()) == Some(Action::Quit));
                    let edit = match &event {
                        Event::PastedText(_) => true,
                        Event::Key(key) => matches!(
                            zellij_launchpad::key_action(key.clone()),
                            Some(
                                Action::Text(_)
                                    | Action::Backspace
                                    | Action::Delete
                                    | Action::Clear
                            )
                        ),
                        _ => false,
                    };
                    let refresh_before = self.state.app.remote_refresh();
                    if self.ready || quit {
                        changed = self.state.handle(event);
                    } else {
                        changed = false;
                    }
                    // Include repeats at the start of an empty field and at the
                    // input limit: they must not turn into immediate searches.
                    if edit && self.ready && !self.failed {
                        self.search_due = Some(now + SEARCH_DEBOUNCE);
                    }
                    if self.state.app.remote_refresh() != refresh_before && self.ready {
                        self.start();
                    }
                }
            }
            if let Some(mutation) = self.state.app.take_history_mutation() {
                self.mutate_history(mutation);
            }
            if let Some(launch) = self.state.app.take_launch() {
                let plugin_id = get_plugin_ids().plugin_id;
                // Session snapshots can lag a newly opened/replaced plugin.
                // A synchronous focus tuple is safe ONLY when its pane ID is us.
                let tab_id = get_focused_pane_info()
                    .ok()
                    .and_then(|(id, pane)| (pane == PaneId::Plugin(plugin_id)).then_some(id))
                    .or_else(|| {
                        get_session_list().ok().and_then(|snapshot| {
                            snapshot
                                .live_sessions
                                .into_iter()
                                .find(|s| s.is_current_session)
                                .and_then(|s| zellij_launchpad::launch_tab(&s, plugin_id))
                                .map(|tab| tab.tab_id)
                        })
                    });
                let tab = tab_id.and_then(get_tab_info);
                let Some(tab) = tab else {
                    self.state.app.launch_rejected();
                    self.state.app.message =
                        Some("Originating tab unavailable; no launch. Retry.".into());
                    return true;
                };
                let name = zellij_launchpad::tab_name(
                    &launch.path,
                    &self.state.app.tool_label(launch.tool),
                );
                self.renamed_tab = Some((tab.tab_id, tab.name, name.clone()));
                // The direct stable-ID command and synchronous read go to the
                // same screen queue. run_action's CLI-only ID variant is NOT
                // serializable in SDK 0.45.1.
                rename_tab_with_id(tab.tab_id as u64, &name);
                if get_tab_info(tab.tab_id).is_some_and(|t| t.name == name) {
                    self.spawn_launch(launch);
                } else {
                    self.reject_launch();
                }
                return true;
            }
            // Drain the anonymous host acknowledgement before taking any new
            // validation, even after F5 recreated App with reused generations.
            if self.search_due.is_some_and(|due| now >= due) {
                self.search_due = None;
            }
            if self.ready
                && !self.failed
                && self.work.is_none()
                && self.cwd_validation.is_none()
                && !self.state.app.quit
            {
                if let Some(request) = self
                    .state
                    .app
                    .take_remote_request_with_search(self.search_due.is_none())
                {
                    self.work = Some((request.generation(), now));
                    if let RemoteRequest::Validate { generation, raw } = &request {
                        self.search_due = None;
                        if let Some(cwd) = &self.original_cwd {
                            if raw == cwd || *raw == self.state.app.path_label(cwd) {
                                self.cwd_validation = Some((self.epoch, *generation));
                                change_host_folder(cwd.into());
                                self.arm_timer(now + Duration::from_secs(1), now);
                                return true;
                            }
                        }
                    }
                    send(match request {
                        RemoteRequest::Query { generation, text } => Request::Query {
                            epoch: self.epoch,
                            generation,
                            text,
                        },
                        RemoteRequest::Validate { generation, raw } => Request::Validate {
                            epoch: self.epoch,
                            generation,
                            raw,
                        },
                    });
                }
            }
            if self.state.app.quit {
                close_self();
                return false;
            }
            if let Some(due) = self.search_due {
                self.arm_timer(due, now);
            }
            if self.pending.is_some() || self.scanning || self.work.is_some() {
                // Worker continuations still drive scanning; timers only wake a
                // debounced search or check for a missing worker response.
                self.arm_timer(now + Duration::from_secs(1), now);
            }
            changed
        }
        fn render(&mut self, rows: usize, cols: usize) {
            print!("{}", self.state.render_frame(rows, cols));
        }
    }
    #[cfg(test)]
    mod tests;
}
#[cfg(target_family = "wasm")]
fn main() {
    wasm::initialize();
}
#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!("Build this plugin for wasm32-wasip1; use the root package for the native TUI.");
}

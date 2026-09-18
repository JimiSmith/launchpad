// SDK macro uses pre-2024 #[no_mangle], hence this thin package is edition 2021.
#[cfg(target_family = "wasm")]
mod wasm {
    use launchpad_plugin::{
        workers::{Engine, Reply, Request},
        State,
    };
    use serde::{Deserialize, Serialize};
    use std::time::{Duration, Instant};
    use zellij_launchpad_prototype::app::{Action, App, Tool};
    use zellij_launchpad_prototype::remote::RemoteRequest;
    use zellij_tile::prelude::*;
    register_plugin!(Plugin);
    register_worker!(IndexWorker, index_worker, INDEX);
    #[derive(Default, Serialize, Deserialize)]
    struct IndexWorker {
        engine: Engine,
        #[cfg(feature = "worker-faults")]
        silenced: bool,
    }
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
                    get_plugin_ids().initial_cwd.to_string_lossy().into_owned()
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
    fn send(request: Request) {
        post_message_to(PluginMessage::new_to_worker(
            "index",
            "request",
            &serde_json::to_string(&request).unwrap(),
        ));
    }
    pub fn initialize() {
        main();
    }
    #[derive(Default)]
    struct Plugin {
        state: State,
        initial_cwd: String,
        home: Option<String>,
        epoch: u64,
        ready: bool,
        scanning: bool,
        pending: Option<Instant>,
        timer_pending: bool,
        remounting: bool,
        failed: bool,
        simulate_launch: bool,
        launch_serial: u64,
        launch_context: Option<std::collections::BTreeMap<String, String>>,
        work: Option<(u64, Instant)>,
        #[cfg(feature = "worker-faults")]
        silence_worker: bool,
    }
    impl Plugin {
        fn start(&mut self) {
            self.epoch += 1;
            self.ready = false;
            self.scanning = false;
            self.work = None;
            self.failed = false;
            self.pending = Some(Instant::now());
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
            self.initial_cwd = get_plugin_ids().initial_cwd.to_string_lossy().into_owned();
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
            if configuration.get("demo").is_some_and(|v| v == "true") {
                self.state.app = App::demo();
            } else {
                self.state.app.host_launch = !self.simulate_launch;
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
                    ]);
                }
                request_permission(&permissions);
            }
        }
        fn update(&mut self, event: Event) -> bool {
            if self.state.app.is_demo() {
                let changed = self.state.handle(event);
                if self.state.app.quit {
                    close_self();
                }
                return changed;
            }
            let mut changed = true;
            match event {
                Event::PermissionRequestResult(PermissionStatus::Granted)
                    if self.home.is_none() =>
                {
                    let home = get_session_environment_variables().remove("HOME");
                    if self.state.prepare_home(home.clone()) {
                        self.home = home;
                        if self.home.as_deref() == Some(&self.initial_cwd) {
                            self.start();
                        } else if std::path::Path::new("/data/worker-reload-attempted").exists() {
                            self.fail("Worker HOME reload failed. Reopen the plugin.".into());
                        } else {
                            self.remounting = true;
                            self.pending = Some(Instant::now());
                            change_host_folder(self.home.clone().unwrap().into());
                        }
                    }
                }
                Event::PermissionRequestResult(PermissionStatus::Denied) => self
                    .fail("Permission denied. Reopen and allow the requested permissions.".into()),
                Event::HostFolderChanged(path)
                    if self.remounting && path.to_str() == self.home.as_deref() =>
                {
                    self.remounting = false;
                    if std::fs::write("/data/worker-reload-attempted", b"1").is_ok() {
                        self.state.app.search_status = "Reloading worker HOME mapping once…".into();
                        reload_plugin_with_id(get_plugin_ids().plugin_id);
                    } else {
                        self.fail("Cannot guard worker reload. Reopen the plugin.".into());
                    }
                }
                Event::FailedToChangeHostFolder(_) => {
                    self.fail("HOME filesystem unavailable. Reopen the plugin.".into())
                }
                Event::CustomMessage(name, payload) if name == "index-reply" && !self.failed => {
                    match serde_json::from_str::<Reply>(&payload) {
                        Ok(Reply::Ready { epoch, cwd })
                            if epoch == self.epoch && Some(&cwd) == self.home.as_ref() =>
                        {
                            self.pending = Some(Instant::now());
                            self.ready = true;
                            self.scanning = true;
                            self.state.app = App::from_remote(cwd.into());
                            self.state.app.host_launch = !self.simulate_launch;
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
                            self.pending = scanning.then(Instant::now);
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
                    self.timer_pending = false;
                    changed = false;
                    if self
                        .pending
                        .is_some_and(|t| t.elapsed() > Duration::from_secs(15))
                        || self
                            .work
                            .is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(15))
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
                        self.state.app.host_launch_rejected();
                    }
                }
                Event::HostFolderChanged(_)
                | Event::PermissionRequestResult(_)
                | Event::ActionComplete(..) => changed = false,
                event => {
                    let quit = matches!(&event, Event::Key(key) if launchpad_plugin::key_action(key.clone()) == Some(Action::Quit));
                    let refresh_before = self.state.app.remote_refresh();
                    if self.ready || quit {
                        changed = self.state.handle(event);
                    } else {
                        changed = false;
                    }
                    if self.state.app.remote_refresh() != refresh_before && self.ready {
                        self.start();
                    }
                }
            }
            if let Some(launch) = self.state.app.take_host_launch() {
                // Target this plugin explicitly, never the last-focused pane.
                // Close rather than suppress it: command exit cannot restore it.
                match launch.tool {
                    Tool::Shell => {
                        // Preserve Zellij's configured default shell and its cwd.
                        if open_terminal_in_place_of_plugin(&launch.path, true).is_none() {
                            self.state.app.host_launch_rejected();
                        }
                    }
                    tool => {
                        self.launch_serial += 1;
                        let context = std::collections::BTreeMap::from([(
                            "launchpad-launch".into(),
                            self.launch_serial.to_string(),
                        )]);
                        self.launch_context = Some(context.clone());
                        run_action(
                            actions::Action::NewInPlacePane {
                                command: Some(actions::RunCommandAction {
                                    command: match tool {
                                        Tool::Claude => "claude",
                                        Tool::Codex => "codex",
                                        Tool::Copilot => "copilot",
                                        Tool::Hermes => "hermes",
                                        Tool::Shell => unreachable!(),
                                    }
                                    .into(),
                                    args: Vec::new(),
                                    cwd: Some(launch.path.into()),
                                    hold_on_close: false,
                                    hold_on_start: false,
                                    ..Default::default()
                                }),
                                pane_name: None,
                                near_current_pane: false,
                                no_focus: false,
                                pane_id_to_replace: Some(PaneId::Plugin(
                                    get_plugin_ids().plugin_id,
                                )),
                                close_replaced_pane: true,
                                tab_id: None,
                            },
                            context,
                        );
                    }
                }
                return true;
            }
            if self.ready && !self.failed && self.work.is_none() && !self.state.app.quit {
                if let Some(request) = self.state.app.take_remote_request() {
                    self.work = Some((request.generation(), Instant::now()));
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
            if (self.pending.is_some() || self.scanning || self.work.is_some())
                && !self.timer_pending
            {
                // Watchdog only: worker continuations, not UI timers, drive the scan.
                set_timeout(1.0);
                self.timer_pending = true;
            }
            if self.state.app.quit {
                close_self();
                return false;
            }
            changed
        }
        fn render(&mut self, rows: usize, cols: usize) {
            print!("{}", self.state.render_frame(rows, cols));
        }
    }
}
#[cfg(target_family = "wasm")]
fn main() {
    wasm::initialize();
}
#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!("Build this plugin for wasm32-wasip1; use the root package for the native TUI.");
}

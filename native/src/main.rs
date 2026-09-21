use clap::Parser;
use crossterm::event::{self, Event, MouseButton, MouseEventKind};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zellij_launchpad::{
    config::{Config, xdg_path},
    history::{self, Entry, Store},
    input,
    terminal::Screen,
    worker::{self, Reply, Request, Worker},
    zellij::{Failure, Zellij, tab_name},
};
use zellij_launchpad_core::{
    app::{Action, App, HistoryMutation, Launch},
    view::{self, HitMap, Pointer},
};

#[derive(Parser)]
#[command(
    version,
    about = "Find a directory and launch a tool in your Zellij pane"
)]
struct Args {
    /// TOML configuration (default: $XDG_CONFIG_HOME/zellij-launchpad/config.toml)
    #[arg(long)]
    config: Option<PathBuf>,
    /// Private handoff between two instances; not a user-facing launch mode.
    #[arg(long, hide = true)]
    shell_handoff: Option<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct ShellHandoff {
    config: PathBuf,
    explicit_config: bool,
    attempt: String,
    tab_id: u32,
    previous_name: String,
    attempted_name: String,
}
fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("Launchpad: {error}");
        std::process::exit(1);
    }
}
fn run(args: Args) -> Result<(), String> {
    let host = Zellij::discover()?;
    let handoff: Option<ShellHandoff> = args
        .shell_handoff
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("Invalid shell handoff: {e}"))?;
    // The CLI discards --cwd when command is absent. This instance was launched
    // with an explicit command/cwd, so Zellij can inherit its actual cwd.
    let shell_failure = if handoff.is_some() {
        match host.launch_default_shell() {
            Ok(()) => return Ok(()),
            Err(error) => Some(error),
        }
    } else {
        None
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("Launchpad requires an interactive terminal".into());
    }
    let home = zellij_launchpad::config::home_from_env(|key| std::env::var_os(key))
        .ok_or("Home directory is missing; set HOME or USERPROFILE")?;
    zellij_launchpad_core::search::HomeIndex::new(home.clone(), home.clone())?;
    let cwd = worker::initial_cwd()?;
    let default_config = xdg_path("XDG_CONFIG_HOME", ".config", &home).join("config.toml");
    let config_path = handoff
        .as_ref()
        .map(|h| h.config.clone())
        .or(args.config.clone())
        .unwrap_or(default_config);
    let config_path = std::path::absolute(config_path).map_err(|e| e.to_string())?;
    let explicit_config = handoff
        .as_ref()
        .map_or(args.config.is_some(), |h| h.explicit_config);
    let config = Config::load(&config_path, explicit_config)?;
    let mut app = App::from_remote(home.clone());
    let ignore = config.apply(&mut app);
    app.set_initial_cwd(cwd.clone());
    let root = xdg_path("XDG_STATE_HOME", ".local/state", &home);
    // Failure remains nonfatal: history is optional, launching is not.
    if let Err(e) = std::fs::create_dir_all(&root) {
        app.message = Some(format!("History unavailable: {e}"));
    }
    let mut history = History {
        store: Store::new(&root),
        rows: Vec::new(),
        serial: 0,
    };
    history.load(&mut app);
    if let (Some(handoff), Some(error)) = (&handoff, &shell_failure) {
        let mut message = format!("Default shell failed: {error}");
        if matches!(error, Failure::Rejected(_)) {
            let (token, _) = history.token();
            if let Err(e) = history.store.remove(&handoff.attempt, &token) {
                message.push_str(&format!(" History rollback failed: {e}"));
            }
            let original = zellij_launchpad::zellij::Pane {
                id: host.pane_id,
                is_plugin: false,
                tab_id: handoff.tab_id,
                tab_name: handoff.previous_name.clone(),
            };
            if let Err(e) = host.restore_name(&original, &handoff.attempted_name) {
                message.push_str(&format!(" Tab restore failed: {e}"));
            }
            history.load(&mut app);
        }
        app.message = Some(message);
    }
    let worker = Worker::start_with_ignore(home, cwd, ignore).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        signal_hook::flag::register(signal, stop.clone()).map_err(|e| e.to_string())?;
    }
    let mut screen = Screen::new().map_err(|e| format!("Initialize terminal: {e}"))?;
    let mut hits = HitMap::default();
    let mut area = ratatui::layout::Rect::default();
    let mut dirty = true;
    let mut busy: Option<(u64, Instant)> = None;
    let mut search_due = Instant::now();
    let mut mouse_after = Instant::now();
    let mut launch_unknown = matches!(shell_failure, Some(Failure::Unknown(_)));
    loop {
        if stop.load(Ordering::Relaxed) || app.quit {
            break;
        }
        while let Ok(reply) = worker.replies.try_recv() {
            dirty = true;
            match reply {
                Reply::Progress {
                    epoch,
                    revision,
                    status,
                } if epoch == app.remote_refresh() => app.remote_progress(revision, status),
                Reply::Results {
                    epoch,
                    generation,
                    revision,
                    paths,
                } => {
                    busy = None;
                    if epoch == app.remote_refresh() && app.accept_remote_revision(revision) {
                        app.apply_remote_results(generation, paths);
                    }
                }
                Reply::Validated {
                    epoch,
                    generation,
                    result,
                } => {
                    busy = None;
                    if epoch == app.remote_refresh() {
                        app.finish_remote_validation(generation, result);
                    }
                }
                Reply::Failed(error) => {
                    busy = None;
                    app.remote_failed(error);
                }
                _ => (),
            }
        }
        if busy.is_some_and(|(_, since)| since.elapsed() > Duration::from_secs(15)) {
            app.remote_failed("Search worker is unresponsive. Reopen Launchpad.".into());
            // Keep the outstanding slot occupied until its late response arrives.
            // No synchronous filesystem fallback or duplicate requests.
            dirty = true;
        }
        if let Some(mutation) = app.take_history_mutation() {
            history.mutate(&mut app, mutation);
            dirty = true;
        }
        if let Some(launch) = app.take_launch() {
            let mut command = app
                .commands
                .get(launch.tool)
                .cloned()
                .ok_or("Selected command disappeared")?;
            let original = match host.origin() {
                Ok(pane) => pane,
                Err(error) => {
                    app.launch_rejected();
                    app.message = Some(error.to_string());
                    dirty = true;
                    continue;
                }
            };
            let name = tab_name(&launch.path, &command.label);
            if let Err(error) = host.rename(original.tab_id, &name) {
                let _ = host.restore_name(&original, &name);
                app.launch_rejected();
                app.message = Some(error.to_string());
                dirty = true;
                continue;
            }
            let attempt = history.record(&launch);
            if command.executable.is_none() {
                command.executable = Some(
                    std::env::current_exe()
                        .map_err(|e| e.to_string())?
                        .to_str()
                        .ok_or("Launchpad executable path is not UTF-8")?
                        .into(),
                );
                command.arguments = vec![
                    "--shell-handoff".into(),
                    serde_json::to_string(&ShellHandoff {
                        config: config_path.clone(),
                        explicit_config,
                        attempt: attempt.clone(),
                        tab_id: original.tab_id,
                        previous_name: original.tab_name.clone(),
                        attempted_name: name.clone(),
                    })
                    .map_err(|e| e.to_string())?,
                ];
            }
            screen.suspend();
            match host.launch(&launch.path, &command) {
                Ok(()) => break,
                Err(error) => {
                    let rejected = matches!(error, Failure::Rejected(_));
                    let mut message = error.to_string();
                    if rejected {
                        if let Err(e) = host.restore_name(&original, &name) {
                            message.push_str(&format!(" Tab restore failed: {e}"));
                        }
                        let (token, _) = history.token();
                        if let Err(e) = history.store.remove(&attempt, &token) {
                            message.push_str(&format!(" History rollback failed: {e}"));
                        }
                        app.launch_rejected();
                        history.load(&mut app);
                    } else {
                        launch_unknown = true;
                    }
                    app.message = Some(message);
                    // Pane replacement may hang up the shell and CLI before a
                    // reply arrives. Do not re-enter the now-closed terminal.
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    screen
                        .enter()
                        .map_err(|e| format!("Restore launcher terminal: {e}"))?;
                    dirty = true;
                }
            }
        }
        if busy.is_none()
            && !launch_unknown
            && let Some(request) = app.take_remote_request_with_search(Instant::now() >= search_due)
        {
            let generation = request.generation();
            worker
                .requests
                .send(Request {
                    epoch: app.remote_refresh(),
                    request,
                })
                .map_err(|_| "Search worker stopped")?;
            busy = Some((generation, Instant::now()));
        }
        if dirty {
            screen
                .terminal
                .draw(|frame| {
                    let new_area = frame.area();
                    if new_area != area {
                        area = new_area;
                        mouse_after = Instant::now() + Duration::from_millis(100);
                    }
                    app.compact = area.width < 40 || area.height < 10;
                    hits = view::render_with_hits(frame, &app);
                })
                .map_err(|e| format!("Draw launcher: {e}"))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(16))
            .map_err(|e| format!("Poll terminal input: {e}"))?
        {
            let action = match event::read().map_err(|e| format!("Read terminal input: {e}"))? {
                Event::Key(key) => input::key_action(key),
                Event::Paste(text) => Some(Action::Text(text)),
                Event::Resize(_, _) => {
                    dirty = true;
                    mouse_after = Instant::now() + Duration::from_millis(100);
                    None
                }
                Event::Mouse(mouse) if Instant::now() >= mouse_after => {
                    let pointer = match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => Some(Pointer::Click),
                        MouseEventKind::ScrollUp => Some(Pointer::ScrollUp),
                        MouseEventKind::ScrollDown => Some(Pointer::ScrollDown),
                        _ => None,
                    };
                    pointer.and_then(|p| hits.action(p, mouse.column, mouse.row, area))
                }
                _ => None,
            };
            if let Some(action) = action {
                if action == Action::Quit {
                    break;
                }
                if launch_unknown {
                    continue;
                }
                if matches!(
                    action,
                    Action::Text(_) | Action::Backspace | Action::Delete | Action::Clear
                ) {
                    search_due = Instant::now() + Duration::from_millis(120);
                }
                let reset = action == Action::Reset;
                app.update(action);
                if reset {
                    history.load(&mut app);
                    search_due = Instant::now();
                }
                dirty = true;
            }
        }
    }
    Ok(())
}
struct History {
    store: Store,
    rows: Vec<Entry>,
    serial: u64,
}
impl History {
    fn token(&mut self) -> (String, u64) {
        self.serial += 1;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        (
            format!(
                "{:039}-{}-{}",
                now.as_nanos(),
                std::process::id(),
                self.serial
            ),
            now.as_secs(),
        )
    }
    fn load(&mut self, app: &mut App) {
        let (token, now) = self.token();
        match self.store.refresh(&token) {
            Ok(rows) => self.rows = rows,
            Err(error) => app.message = Some(format!("History unavailable: {error}")),
        }
        app.history = self
            .rows
            .iter()
            .enumerate()
            .map(|(id, row)| Launch {
                id: id as u64,
                path: row.path.clone(),
                tool: row.tool,
                age: history::age(row.opened_at, now),
            })
            .collect();
        app.recent = app.recent.min(self.rows.len().saturating_sub(1));
    }
    fn mutate(&mut self, app: &mut App, mutation: HistoryMutation) {
        let (token, _) = self.token();
        let result = match mutation {
            HistoryMutation::Clear => self.store.clear(&token),
            HistoryMutation::Remove(id) => self
                .rows
                .get(id as usize)
                .ok_or_else(|| "History selection changed; refresh and retry".to_string())
                .and_then(|row| self.store.remove(&row.id, &token)),
        };
        match result {
            Ok(()) => {
                app.message = None;
                self.load(app);
            }
            Err(error) => app.message = Some(format!("History change failed: {error}")),
        }
    }
    fn record(&mut self, launch: &Launch) -> String {
        let (id, opened_at) = self.token();
        if let Err(error) = self.store.record(Entry {
            id: id.clone(),
            path: launch.path.clone(),
            tool: launch.tool,
            opened_at,
        }) {
            eprintln!("Launchpad: history not saved: {error}");
        }
        id
    }
}

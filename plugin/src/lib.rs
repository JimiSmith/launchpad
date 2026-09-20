//! Zellij 0.45.1 event adapter; the UI and its state live in the core crate.
pub mod cwd;
pub mod history;
pub mod workers;
/// Resolve position-keyed panes and stable-ID tabs from ONE coherent snapshot.
pub fn launch_tab(
    session: &zellij_tile::prelude::SessionInfo,
    plugin_id: u32,
) -> Option<zellij_tile::prelude::TabInfo> {
    let position = session.panes.panes.iter().find_map(|(position, panes)| {
        panes
            .iter()
            .any(|p| p.is_plugin && p.id == plugin_id)
            .then_some(*position)
    })?;
    session
        .tabs
        .iter()
        .find(|tab| tab.position == position)
        .cloned()
}

pub fn tab_name(path: &str, label: &str) -> String {
    let basename = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("/");
    format!("{basename} · {label}")
}
use zellij_launchpad_core::app::{Action, Focus};
use zellij_tile::prelude::{BareKey, KeyModifier, KeyWithModifier};

pub fn key_action(key: KeyWithModifier) -> Option<Action> {
    use BareKey::*;
    use KeyModifier::*;
    let mods = &key.key_modifiers;
    if mods.contains(&Alt) || mods.contains(&Super) {
        return None;
    }
    if mods.contains(&Ctrl) {
        return match key.bare_key {
            Char('c' | 'q') => Some(Action::Quit),
            Char('p') => Some(Action::Focus(Focus::Path)),
            Char('t') => Some(Action::Focus(Focus::Tools)),
            Char('r') => Some(Action::Focus(Focus::History)),
            Char('a') => Some(Action::Home),
            Char('e') => Some(Action::End),
            Char('u') => Some(Action::Clear),
            Char('l') => Some(Action::ClearHistory),
            _ => None,
        };
    }
    Some(match key.bare_key {
        Char(c) => Action::Text(c.to_string()),
        Left => Action::Left,
        Right => Action::Right,
        Up => Action::Up,
        Down => Action::Down,
        Home => Action::Home,
        End => Action::End,
        Backspace => Action::Backspace,
        Delete => Action::Delete,
        Tab if mods.contains(&Shift) => Action::BackTab,
        Tab => Action::Tab,
        Enter => Action::Enter,
        Esc => Action::Escape,
        F(1) => Action::Help,
        F(5) => Action::Reset,
        _ => return None,
    })
}

#[derive(Default)]
pub struct State {
    pub app: zellij_launchpad_core::app::App,
    hits: zellij_launchpad_core::view::HitMap,
    area: ratatui::layout::Rect,
    pointer: Option<(u16, u16)>,
    mouse_quiet_since: Option<std::time::Instant>,
    pending_home: Option<std::path::PathBuf>,
}
impl State {
    pub fn replace_app(&mut self, mut app: zellij_launchpad_core::app::App) {
        app.commands = self.app.commands.clone();
        app.simulate_launch = self.app.simulate_launch;
        self.app = app;
    }
    pub fn prepare_home(&mut self, home: Option<String>) -> bool {
        self.pending_home = None;
        let Some(home) = home else {
            self.app.search_status =
                "Session HOME is missing. Reopen from a session with HOME set.".into();
            return false;
        };
        let path = std::path::PathBuf::from(home);
        if let Err(error) =
            zellij_launchpad_core::search::HomeIndex::new(path.clone(), "/host".into())
        {
            self.app.search_status = error;
            return false;
        }
        self.pending_home = Some(path);
        self.app.search_status = "Opening HOME filesystem…".into();
        true
    }
    pub fn handle(&mut self, event: zellij_tile::prelude::Event) -> bool {
        use zellij_launchpad_core::view::Pointer;
        use zellij_tile::prelude::{Event, Mouse};
        if matches!(event, Event::Mouse(_)) {
            if let Some(since) = self.mouse_quiet_since {
                if since.elapsed() < std::time::Duration::from_millis(100) {
                    // SDK mouse events have no layout generation. Wait for a
                    // quiet interval after resize, extending it for queued mice.
                    self.mouse_quiet_since = Some(std::time::Instant::now());
                    return false;
                }
                self.mouse_quiet_since = None;
            }
        }
        let action = match event {
            Event::PermissionRequestResult(zellij_tile::prelude::PermissionStatus::Denied) => {
                self.pending_home = None;
                self.app.search_status =
                    "HOME access denied. Reopen the plugin and allow its two permissions.".into();
                return true;
            }
            Event::HostFolderChanged(home) => {
                if self.pending_home.as_ref() == Some(&home) {
                    self.pending_home = None;
                    self.replace_app(zellij_launchpad_core::app::App::from_remote(home));
                    return true;
                }
                return false;
            }
            Event::FailedToChangeHostFolder(_) => {
                self.pending_home = None;
                self.app.search_status =
                    "HOME filesystem unavailable. Check HOME access and reopen the plugin.".into();
                return true;
            }
            Event::Timer(_) => return self.app.index_tick(),
            Event::Key(key) => key_action(key),
            Event::PastedText(text) => Some(Action::Text(text)),
            Event::Mouse(
                Mouse::Hover(row, col)
                | Mouse::Hold(row, col)
                | Mouse::Release(row, col)
                | Mouse::RightClick(row, col),
            ) => {
                self.pointer = u16::try_from(col).ok().zip(u16::try_from(row).ok());
                None
            }
            Event::Mouse(mouse @ (Mouse::ScrollUp(_) | Mouse::ScrollDown(_))) => {
                let (pointer, count) = match mouse {
                    Mouse::ScrollUp(n) => (Pointer::ScrollUp, n),
                    Mouse::ScrollDown(n) => (Pointer::ScrollDown, n),
                    _ => unreachable!(),
                };
                if let Some((x, y)) = self.pointer {
                    if let Some(action) = self.hits.action(pointer, x, y, self.area) {
                        for _ in 0..count.min(128) {
                            self.app.update(action.clone());
                        }
                        return count > 0;
                    }
                }
                None
            }
            Event::Mouse(Mouse::LeftClick(row, col)) => {
                self.pointer = u16::try_from(col).ok().zip(u16::try_from(row).ok());
                u16::try_from(row)
                    .ok()
                    .zip(u16::try_from(col).ok())
                    .and_then(|(y, x)| self.hits.action(Pointer::Click, x, y, self.area))
            }
            _ => None,
        };
        if let Some(action) = action {
            self.app.update(action);
            true
        } else {
            false
        }
    }

    pub fn render_frame(&mut self, rows: usize, cols: usize) -> String {
        use ratatui::{backend::TestBackend, layout::Rect, style::Modifier, Terminal};
        use zellij_launchpad_core::{ansi, view};
        let width = cols.min(u16::MAX as usize) as u16;
        let height = rows.min(u16::MAX as usize) as u16;
        let area = Rect::new(0, 0, width, height);
        if self.area != area {
            if !self.area.is_empty() {
                self.mouse_quiet_since = Some(std::time::Instant::now());
            }
            self.pointer = None;
        }
        self.area = area;
        self.app.compact = width < 40 || height < 10;
        // TestBackend is Ratatui's in-memory backend: no raw mode, tty or OS IO.
        // A fresh full buffer is required: Zellij clears the viewport every render.
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| self.hits = view::render_with_hits(f, &self.app))
            .unwrap();
        let backend = terminal.backend();
        let mut buffer = backend.buffer().clone();
        if backend.cursor_visible() {
            if let Some(cell) = buffer.cell_mut(backend.cursor_position()) {
                cell.modifier.insert(Modifier::REVERSED);
            }
        }
        ansi::serialize(&buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zellij_launchpad_core::app::{App, Launch, Tool};
    use zellij_launchpad_core::remote::RemoteRequest;

    const HOME: &str = "/home/example";

    /// A worker-backed State with a settled suggestion list and recent rows,
    /// built the way the plugin builds one. There are no fixture directories.
    fn state() -> State {
        let mut app = App::from_remote(HOME.into());
        app.simulate_launch = true;
        app.configure(&std::collections::BTreeMap::from([
            ("commands".into(), "claude,codex".into()),
            ("command_claude".into(), "claude".into()),
            ("command_codex".into(), "codex".into()),
        ]));
        app.update(zellij_launchpad_core::app::Action::Text("notes".into()));
        let Some(RemoteRequest::Query { generation, .. }) = app.take_remote_request() else {
            panic!("expected a pending query");
        };
        assert!(app.apply_remote_results(
            generation,
            (0..6)
                .map(|i| format!("{HOME}/Projects/notes-{i}"))
                .collect(),
        ));
        app.history = (0..6)
            .map(|i| Launch {
                id: i,
                path: format!("{HOME}/Projects/recent-{i}"),
                tool: if i % 2 == 0 {
                    Tool::Shell
                } else {
                    Tool::new("claude").unwrap()
                },
                age: format!("{}h ago", i + 1),
            })
            .collect();
        State {
            app,
            ..State::default()
        }
    }

    #[test]
    fn home_access_waits_for_ack_and_denial_never_indexes() {
        use zellij_tile::prelude::{Event, PermissionStatus};
        let mut state = State::default();
        state.handle(Event::PermissionRequestResult(PermissionStatus::Denied));
        assert!(state.app.search_status.contains("denied"));
        assert!(state.app.history.is_empty());
        assert!(state.prepare_home(Some(HOME.into())));
        state.handle(Event::HostFolderChanged("/somewhere/else".into()));
        assert_eq!(
            state.app.search_status, "Opening HOME filesystem…",
            "a non-HOME mount is not our remount"
        );
        state.handle(Event::HostFolderChanged(HOME.into()));
        assert!(
            state.app.take_remote_request().is_some(),
            "the accepted mount hands the index to the worker"
        );
    }

    #[test]
    fn missing_home_and_failed_remount_stay_restricted() {
        use zellij_tile::prelude::Event;
        for home in [
            None,
            Some("".into()),
            Some("relative".into()),
            Some("/".into()),
        ] {
            let mut state = State::default();
            assert!(!state.prepare_home(home));
            assert!(state.app.search_status.contains("HOME"));
        }
        let mut state = State::default();
        state.prepare_home(Some(HOME.into()));
        state.handle(Event::FailedToChangeHostFolder(Some("denied".into())));
        assert!(state.app.search_status.contains("unavailable"));
    }

    #[test]
    fn sdk_click_hold_release_and_paste_obey_domain_contract() {
        use zellij_tile::prelude::{Event, Mouse};
        let mut s = state();
        s.render_frame(24, 80);
        s.handle(Event::Key(key(BareKey::Char('u'), &[KeyModifier::Ctrl])));
        s.handle(Event::PastedText("notes\n\t".into()));
        assert_eq!(s.app.editor.text, "notes");

        s.render_frame(24, 80);
        let tools = hit(&s, Action::SelectTool(Tool::new("codex").unwrap()));
        s.handle(Event::Mouse(Mouse::LeftClick(tools.1, tools.0 as usize)));
        assert_eq!(s.app.tool, Tool::new("codex").unwrap());
        assert!(s.app.take_launch().is_none(), "selection never launches");

        let launch = hit(&s, Action::LaunchForm);
        for m in [
            Mouse::Hold(launch.1, launch.0 as usize),
            Mouse::Release(launch.1, launch.0 as usize),
            Mouse::RightClick(launch.1, launch.0 as usize),
            Mouse::LeftClick(-1, launch.0 as usize),
        ] {
            s.handle(Event::Mouse(m));
        }
        assert!(
            !matches!(
                s.app.take_remote_request(),
                Some(RemoteRequest::Validate { .. })
            ),
            "drag, release, right click and off-screen clicks are inert"
        );
        s.handle(Event::Mouse(Mouse::LeftClick(launch.1, launch.0 as usize)));
        assert!(
            matches!(
                s.app.take_remote_request(),
                Some(RemoteRequest::Validate { .. })
            ),
            "an explicit launch click validates first"
        );
    }

    #[test]
    fn wheel_uses_last_known_section_and_forgets_it_on_resize() {
        use zellij_tile::prelude::{Event, Mouse};
        let mut s = state();
        s.render_frame(24, 80);
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(
            s.app.recent, 0,
            "no guessed wheel focus without coordinates"
        );
        let history = wheel(&s, ScrollTarget::History);
        s.handle(Event::Mouse(Mouse::Hover(history.1, history.0 as usize)));
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(s.app.recent, 3);
        s.handle(Event::Mouse(Mouse::Hover(0, 0)));
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(s.app.recent, 3, "the title bar is not a scroll target");
        s.handle(Event::Mouse(Mouse::Hover(history.1, history.0 as usize)));
        s.render_frame(36, 120);
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(s.app.recent, 3, "a resize forgets the pointer");
    }

    #[test]
    fn queued_mouse_is_quarantined_after_resize() {
        use zellij_tile::prelude::{Event, Mouse};
        let mut s = state();
        s.render_frame(24, 80);
        s.render_frame(36, 120);
        let (x, y) = hit(&s, Action::LaunchForm);
        s.handle(Event::Mouse(Mouse::LeftClick(y, x as usize)));
        assert!(
            s.app.take_remote_request().is_none(),
            "coordinates queued against the old frame cannot activate new controls"
        );
    }

    #[test]
    fn renders_shared_ui_caret_hits_and_resizes_to_guard() {
        let mut s = state();
        let frame = s.render_frame(24, 80);
        assert!(frame.contains("Launchpad"));
        assert!(frame.contains("\x1b[7m"), "synthetic caret is visible");
        let launch = hit(&s, Action::LaunchForm);
        let narrow = s.render_frame(8, 30);
        assert!(narrow.contains("Resize to at least"));
        assert!(s.app.compact);
        assert_eq!(
            s.hits
                .action(Pointer::Click, launch.0, launch.1 as u16, s.area),
            None
        );
        assert_eq!(s.render_frame(0, 0), "");
        assert!(s.render_frame(36, 120).contains("Launchpad"));
        assert!(!s.app.compact);
    }

    #[test]
    fn simulate_launch_is_labelled_and_real_mode_says_it_replaces_the_pane() {
        let mut s = state();
        assert!(s.render_frame(36, 120).contains("launch suppressed"));
        s.app.simulate_launch = false;
        let frame = s.render_frame(36, 120);
        assert!(frame.contains("replace this pane"));
        assert!(!frame.contains("suppressed"));
    }

    use zellij_launchpad_core::app::ScrollTarget;
    use zellij_launchpad_core::view::Pointer;

    /// First screen cell whose click maps to `action`, as (x, y).
    fn hit(s: &State, action: Action) -> (u16, isize) {
        (0..s.area.height)
            .flat_map(|y| (0..s.area.width).map(move |x| (x, y)))
            .find(|&(x, y)| s.hits.action(Pointer::Click, x, y, s.area) == Some(action.clone()))
            .map(|(x, y)| (x, y as isize))
            .unwrap_or_else(|| panic!("no hit region for {action:?}"))
    }
    /// First screen cell whose wheel maps to `target`, as (x, y).
    fn wheel(s: &State, target: ScrollTarget) -> (u16, isize) {
        (0..s.area.height)
            .flat_map(|y| (0..s.area.width).map(move |x| (x, y)))
            .find(|&(x, y)| {
                s.hits.action(Pointer::ScrollDown, x, y, s.area)
                    == Some(Action::Scroll(target, true))
            })
            .map(|(x, y)| (x, y as isize))
            .expect("no wheel region")
    }

    fn key(bare_key: BareKey, modifiers: &[KeyModifier]) -> KeyWithModifier {
        KeyWithModifier {
            bare_key,
            key_modifiers: modifiers.iter().copied().collect(),
        }
    }
    #[test]
    fn sdk_keys_preserve_control_focus_shift_tab_and_literal_text() {
        use BareKey::*;
        use KeyModifier::*;
        for (k, mods, action) in [
            (Char('q'), vec![], Action::Text("q".into())),
            (Char('q'), vec![Ctrl], Action::Quit),
            (Char('t'), vec![Ctrl], Action::Focus(Focus::Tools)),
            (Tab, vec![Shift], Action::BackTab),
            (Enter, vec![], Action::Enter),
            (Char('修'), vec![], Action::Text("修".into())),
            (F(1), vec![], Action::Help),
        ] {
            assert_eq!(key_action(key(k, &mods)), Some(action));
        }
        assert_eq!(key_action(key(Char('q'), &[Alt])), None);
        assert_eq!(key_action(key(Char('u'), &[Super])), None);
        assert_eq!(
            key_action(key(F(6), &[])),
            None,
            "no fixture toggle remains"
        );
    }
}

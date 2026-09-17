//! Zellij 0.45.1 event adapter; UI and fixtures remain in the shared crate.
use zellij_launchpad_prototype::app::{Action, Focus};
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
        F(6) => Action::ToggleCopilot,
        _ => return None,
    })
}

#[derive(Default)]
pub struct State {
    pub app: zellij_launchpad_prototype::app::App,
    hits: zellij_launchpad_prototype::view::HitMap,
    area: ratatui::layout::Rect,
    pointer: Option<(u16, u16)>,
    mouse_quiet_since: Option<std::time::Instant>,
    pending_home: Option<std::path::PathBuf>,
}
impl State {
    pub fn prepare_home(&mut self, home: Option<String>) -> bool {
        self.pending_home = None;
        let Some(home) = home else {
            self.app.search_status =
                "Session HOME is missing. Reopen from a session with HOME set.".into();
            return false;
        };
        let path = std::path::PathBuf::from(home);
        if let Err(error) =
            zellij_launchpad_prototype::search::HomeIndex::new(path.clone(), "/host".into())
        {
            self.app.search_status = error;
            return false;
        }
        self.pending_home = Some(path);
        self.app.search_status = "Opening HOME filesystem…".into();
        true
    }
    pub fn handle(&mut self, event: zellij_tile::prelude::Event) -> bool {
        use zellij_launchpad_prototype::view::Pointer;
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
                    self.app =
                        zellij_launchpad_prototype::app::App::from_home(home, "/host".into());
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
        use zellij_launchpad_prototype::{ansi, view};
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
    fn demo_state() -> State {
        State {
            app: zellij_launchpad_prototype::app::App::demo(),
            ..State::default()
        }
    }
    #[test]
    fn home_access_waits_for_ack_and_denial_is_not_demo() {
        use zellij_tile::prelude::{Event, PermissionStatus};
        let mut state = State::default();
        assert!(!state.app.is_indexing());
        state.handle(Event::PermissionRequestResult(PermissionStatus::Denied));
        assert!(state.app.search_status.contains("denied"));
        assert!(state.app.history.is_empty());
        assert!(!state.app.is_indexing());
        assert!(state.prepare_home(Some("/home/example".into())));
        state.handle(Event::HostFolderChanged("/somewhere/else".into()));
        assert!(!state.app.is_indexing(), "never index a non-HOME mount");
        state.handle(Event::HostFolderChanged("/home/example".into()));
        assert!(state.app.is_indexing());
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
            assert!(!state.app.is_indexing());
            assert!(state.app.search_status.contains("HOME"));
        }
        let mut state = State::default();
        state.prepare_home(Some("/home/example".into()));
        state.handle(Event::FailedToChangeHostFolder(Some("denied".into())));
        assert!(state.app.search_status.contains("unavailable"));
        assert!(!state.app.is_indexing());
    }
    #[test]
    fn sdk_click_hold_release_and_paste_obey_domain_contract() {
        use zellij_launchpad_prototype::app::{Screen, Tool};
        use zellij_tile::prelude::{Event, Mouse};
        let mut s = demo_state();
        s.render_frame(24, 80);
        s.handle(Event::Key(key(BareKey::Char('u'), &[KeyModifier::Ctrl])));
        s.handle(Event::PastedText("notes\n\t".into()));
        assert_eq!(s.app.editor.text, "notes");
        s.handle(Event::Key(key(BareKey::Down, &[])));
        s.handle(Event::Key(key(BareKey::Enter, &[])));
        assert_eq!(s.app.screen, Screen::Dashboard);
        assert_eq!(s.app.history[0].id, 10);
        s.render_frame(24, 80);
        s.handle(Event::Mouse(Mouse::LeftClick(9, 26)));
        assert_eq!(s.app.tool, Tool::Codex);
        for m in [
            Mouse::Hold(9, 68),
            Mouse::Release(9, 68),
            Mouse::RightClick(9, 68),
            Mouse::LeftClick(-1, 68),
        ] {
            s.handle(Event::Mouse(m));
        }
        assert_eq!(s.app.screen, Screen::Dashboard);
        s.handle(Event::Mouse(Mouse::LeftClick(9, 68)));
        assert!(matches!(s.app.screen, Screen::Terminal(_)));
        let id = s.app.history[0].id;
        s.handle(Event::Mouse(Mouse::Hold(9, 68)));
        s.handle(Event::Mouse(Mouse::Release(9, 68)));
        s.handle(Event::Key(key(BareKey::Enter, &[])));
        assert_eq!(s.app.history[0].id, id);
    }
    #[test]
    fn wheel_uses_last_known_section_and_forgets_it_on_resize() {
        use zellij_tile::prelude::{Event, Mouse};
        let mut s = demo_state();
        s.render_frame(24, 80);
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(
            s.app.recent, 0,
            "no guessed wheel focus without coordinates"
        );
        s.handle(Event::Mouse(Mouse::Hover(14, 10)));
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(s.app.recent, 3);
        s.handle(Event::Mouse(Mouse::Hover(0, 0)));
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(s.app.recent, 3);
        s.handle(Event::Mouse(Mouse::Hover(14, 10)));
        s.render_frame(36, 120);
        s.handle(Event::Mouse(Mouse::ScrollDown(3)));
        assert_eq!(s.app.recent, 3);
    }
    #[test]
    fn queued_mouse_is_quarantined_after_resize() {
        use zellij_launchpad_prototype::{app::Screen, view::Pointer};
        use zellij_tile::prelude::{Event, Mouse};
        let mut s = demo_state();
        s.render_frame(24, 80);
        s.render_frame(36, 120);
        let (x, y) = (0..36)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .find(|&(x, y)| s.hits.action(Pointer::Click, x, y, s.area) == Some(Action::LaunchForm))
            .unwrap();
        s.handle(Event::Mouse(Mouse::LeftClick(y as isize, x as usize)));
        assert_eq!(
            s.app.screen,
            Screen::Dashboard,
            "coordinates queued against old frame cannot activate new controls"
        );
    }
    #[test]
    fn renders_shared_ui_caret_hits_and_resizes_to_guard() {
        use zellij_launchpad_prototype::view::Pointer;
        let mut state = demo_state();
        let frame = state.render_frame(24, 80);
        assert!(frame.contains("Launchpad"));
        assert!(frame.contains("\x1b[7m"), "synthetic caret is visible");
        assert_eq!(
            state.hits.action(Pointer::Click, 68, 9, state.area),
            Some(Action::LaunchForm)
        );
        let narrow = state.render_frame(8, 30);
        assert!(narrow.contains("Resize to at least"));
        assert!(state.app.compact);
        assert_eq!(state.hits.action(Pointer::Click, 68, 9, state.area), None);
        assert_eq!(state.render_frame(0, 0), "");
        assert!(state.render_frame(36, 120).contains("Launchpad"));
        assert!(!state.app.compact);
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
    }
}

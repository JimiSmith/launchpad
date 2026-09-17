//! Native adapter only. The app model and view never import Crossterm.
use crossterm::{
    cursor::Show,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
    style::ResetColor,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, IsTerminal},
    time::Duration,
};
use zellij_launchpad_prototype::{
    app::{Action, App, Focus},
    view,
};

pub fn action(key: KeyEvent) -> Option<Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c' | 'q') => Some(Action::Quit),
            KeyCode::Char('p') => Some(Action::Focus(Focus::Path)),
            KeyCode::Char('t') => Some(Action::Focus(Focus::Tools)),
            KeyCode::Char('r') => Some(Action::Focus(Focus::History)),
            KeyCode::Char('a') => Some(Action::Home),
            KeyCode::Char('e') => Some(Action::End),
            KeyCode::Char('u') => Some(Action::Clear),
            KeyCode::Char('l') => Some(Action::ClearHistory),
            _ => None,
        };
    }
    if key.kind == KeyEventKind::Repeat && matches!(key.code, KeyCode::Enter | KeyCode::F(_)) {
        return None;
    }
    Some(match key.code {
        KeyCode::Char(c) => Action::Text(c.to_string()),
        KeyCode::Left => Action::Left,
        KeyCode::Right => Action::Right,
        KeyCode::Up => Action::Up,
        KeyCode::Down => Action::Down,
        KeyCode::Home => Action::Home,
        KeyCode::End => Action::End,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Delete => Action::Delete,
        KeyCode::Tab => Action::Tab,
        KeyCode::BackTab => Action::BackTab,
        KeyCode::Enter => Action::Enter,
        KeyCode::Esc => Action::Escape,
        KeyCode::F(1) => Action::Help,
        KeyCode::F(5) => Action::Reset,
        KeyCode::F(6) => Action::ToggleCopilot,
        _ => return None,
    })
}

fn mouse_action(
    mouse: MouseEvent,
    hits: &view::HitMap,
    current: ratatui::layout::Rect,
    ready: bool,
) -> Option<Action> {
    if !ready || !mouse.modifiers.is_empty() {
        return None;
    }
    let pointer = match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => view::Pointer::Click,
        MouseEventKind::ScrollUp => view::Pointer::ScrollUp,
        MouseEventKind::ScrollDown => view::Pointer::ScrollDown,
        _ => return None,
    };
    hits.action(pointer, mouse.column, mouse.row, current)
}

fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        ResetColor,
        Show,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}
struct Session;
impl Session {
    fn start() -> io::Result<Self> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        let session = Self;
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture
        )?;
        Ok(session)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        restore();
    }
}

fn input_wait(indexing: bool, mouse_ready: bool) -> Duration {
    Duration::from_millis(if indexing && mouse_ready { 10 } else { 100 })
}

pub fn run() -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "an interactive terminal is required (stdin and stdout must be TTYs)",
        ));
    }
    let _session = Session::start()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = if std::env::args().skip(1).any(|arg| arg == "--demo") {
        App::demo()
    } else if let Some(home) = std::env::var_os("HOME") {
        App::from_home(home.clone().into(), home.into())
    } else {
        let mut app = App::default();
        app.search_status = "HOME is missing. Set HOME to your home directory and restart.".into();
        app
    };
    let mut mouse_ready = true;
    let mut last_size = terminal.size()?;
    while !app.quit {
        app.index_tick();
        let size = terminal.size()?;
        if size != last_size {
            mouse_ready = false;
            last_size = size;
        }
        app.compact = size.width < 40 || size.height < 10;
        let mut hits = view::HitMap::default();
        terminal.draw(|f| hits = view::render_with_hits(f, &app))?;
        if !event::poll(input_wait(app.is_indexing(), mouse_ready))? {
            // Resize has no generation tag in the mouse protocol. Quarantine
            // queued coordinates until the resized frame is drawn and input is quiet.
            mouse_ready = true;
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if let Some(action) = action(key) {
                    app.update(action);
                }
            }
            Event::Paste(text) => app.update(Action::Text(text)),
            Event::Mouse(mouse) => {
                let current = terminal.size()?;
                if current != size {
                    mouse_ready = false;
                }
                let area = ratatui::layout::Rect::new(0, 0, current.width, current.height);
                if let Some(action) = mouse_action(mouse, &hits, area, mouse_ready) {
                    app.update(action);
                }
            }
            Event::Resize(_, _) => {
                mouse_ready = false;
                terminal.autoresize()?;
            }
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use zellij_launchpad_prototype::app::Focus;
    // Invoked in isolated real PTYs by tools/verify_cleanup.py. Fault injection
    // is test-only: the release executable has no hidden modes or environment reads.
    #[test]
    #[ignore = "requires an isolated PTY; run tools/verify_cleanup.py"]
    fn session_cleanup_probe() {
        assert!(io::stdin().is_terminal() && io::stdout().is_terminal());
        let mode = std::env::var("LAUNCHPAD_CLEANUP_PROBE").unwrap();
        let exercise = || -> io::Result<()> {
            let _session = Session::start()?;
            if mode == "panic" {
                panic!("intentional cleanup probe");
            }
            Err(io::Error::other("intentional cleanup probe"))
        };
        if mode == "panic" {
            assert!(std::panic::catch_unwind(exercise).is_err());
        } else {
            assert!(exercise().is_err());
        }
    }
    #[test]
    fn indexing_keeps_resize_mouse_quarantine_at_one_hundred_ms() {
        assert_eq!(input_wait(true, false), Duration::from_millis(100));
        assert_eq!(input_wait(true, true), Duration::from_millis(10));
        assert_eq!(input_wait(false, true), Duration::from_millis(100));
    }
    #[test]
    fn mouse_adapter_only_accepts_left_down_and_vertical_wheel() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        use ratatui::{backend::TestBackend, layout::Rect};
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut hits = view::HitMap::default();
        terminal
            .draw(|f| hits = view::render_with_hits(f, &App::demo()))
            .unwrap();
        let area = Rect::new(0, 0, 80, 24);
        let event = |kind| MouseEvent {
            kind,
            column: 68,
            row: 9,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            mouse_action(
                event(MouseEventKind::Down(MouseButton::Left)),
                &hits,
                area,
                true
            ),
            Some(Action::LaunchForm)
        );
        for kind in [
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            assert_eq!(mouse_action(event(kind), &hits, area, true), None);
        }
        let left = event(MouseEventKind::Down(MouseButton::Left));
        assert_eq!(
            mouse_action(left, &hits, area, false),
            None,
            "queued mouse after resize is quarantined"
        );
        assert_eq!(
            mouse_action(left, &hits, Rect::new(0, 0, 40, 12), true),
            None
        );
        let mut modified = left;
        modified.modifiers = KeyModifiers::CONTROL;
        assert_eq!(mouse_action(modified, &hits, area, true), None);
        for (kind, down) in [
            (MouseEventKind::ScrollDown, true),
            (MouseEventKind::ScrollUp, false),
        ] {
            let wheel = MouseEvent {
                column: 10,
                row: 14,
                ..event(kind)
            };
            assert_eq!(
                mouse_action(wheel, &hits, area, true),
                Some(Action::Scroll(
                    zellij_launchpad_prototype::app::ScrollTarget::History,
                    down
                ))
            );
        }
    }
    #[test]
    fn maps_shortcuts_without_stealing_printable_q_or_repeated_enter() {
        let key = |code, mods| KeyEvent::new(code, mods);
        assert_eq!(
            action(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::Text("q".into()))
        );
        assert_eq!(
            action(key(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            action(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            Some(Action::Home)
        );
        assert_eq!(
            action(key(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Some(Action::Clear)
        );
        for (c, f) in [
            ('p', Focus::Path),
            ('t', Focus::Tools),
            ('r', Focus::History),
        ] {
            assert_eq!(
                action(key(KeyCode::Char(c), KeyModifiers::CONTROL)),
                Some(Action::Focus(f))
            );
        }
        assert_eq!(
            action(key(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(Action::BackTab)
        );
        assert_eq!(
            action(KeyEvent::new_with_kind(
                KeyCode::Enter,
                KeyModifiers::NONE,
                KeyEventKind::Repeat
            )),
            None
        );
        assert_eq!(
            action(KeyEvent::new_with_kind(
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                KeyEventKind::Release
            )),
            None
        );
    }
}

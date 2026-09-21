use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use zellij_launchpad_core::app::{Action, Focus};

pub fn key_action(key: KeyEvent) -> Option<Action> {
    use KeyCode::*;
    if key.kind == KeyEventKind::Release
        || key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
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
    Some(match key.code {
        Char(c) => Action::Text(c.to_string()),
        Left => Action::Left,
        Right => Action::Right,
        Up => Action::Up,
        Down => Action::Down,
        Home => Action::Home,
        End => Action::End,
        Backspace => Action::Backspace,
        Delete => Action::Delete,
        Tab if key.modifiers.contains(KeyModifiers::SHIFT) => Action::BackTab,
        Tab => Action::Tab,
        BackTab => Action::BackTab,
        Enter => Action::Enter,
        Esc => Action::Escape,
        F(1) => Action::Help,
        F(5) => Action::Reset,
        _ => return None,
    })
}

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
            Left => Some(Action::SegmentLeft),
            Right => Some(Action::SegmentRight),
            Backspace | Char('h') => Some(Action::DeleteSegmentLeft),
            Delete => Some(Action::DeleteSegmentRight),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_bindings_preserve_plain_keys_and_handle_modifiers() {
        use KeyCode::*;
        for (code, expected) in [
            (Left, Action::SegmentLeft),
            (Right, Action::SegmentRight),
            (Backspace, Action::DeleteSegmentLeft),
            (Delete, Action::DeleteSegmentRight),
            (Char('h'), Action::DeleteSegmentLeft),
        ] {
            for modifiers in [
                KeyModifiers::CONTROL,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ] {
                for kind in [KeyEventKind::Press, KeyEventKind::Repeat] {
                    assert_eq!(
                        key_action(KeyEvent::new_with_kind(code, modifiers, kind)),
                        Some(expected.clone())
                    );
                }
                assert_eq!(
                    key_action(KeyEvent::new_with_kind(
                        code,
                        modifiers,
                        KeyEventKind::Release
                    )),
                    None
                );
                for extra in [KeyModifiers::ALT, KeyModifiers::SUPER] {
                    assert_eq!(key_action(KeyEvent::new(code, modifiers | extra)), None);
                }
            }
        }
        for (code, expected) in [
            (Left, Action::Left),
            (Right, Action::Right),
            (Home, Action::Home),
            (End, Action::End),
            (Backspace, Action::Backspace),
            (Delete, Action::Delete),
            (Char('h'), Action::Text("h".into())),
        ] {
            assert_eq!(
                key_action(KeyEvent::new(code, KeyModifiers::NONE)),
                Some(expected)
            );
        }
        for (code, expected) in [
            (Char('a'), Action::Home),
            (Char('e'), Action::End),
            (Char('u'), Action::Clear),
        ] {
            assert_eq!(
                key_action(KeyEvent::new(code, KeyModifiers::CONTROL)),
                Some(expected)
            );
        }
    }
}

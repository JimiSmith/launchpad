use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use launchpad_core::{
    app::{Action, Focus},
    commands::Commands,
    shortcut::{Key, Shortcut},
};

/// Configured shortcuts first; validation rejects any that shadow a built-in.
pub fn event_action(key: KeyEvent, commands: &Commands) -> Option<Action> {
    shortcut(key)
        .and_then(|s| commands.by_shortcut(s))
        .map(Action::Shortcut)
        .or_else(|| key_action(key))
}

/// The canonical shortcut for a press, or None without Ctrl, Alt or Super.
pub fn shortcut(key: KeyEvent) -> Option<Shortcut> {
    let m = key.modifiers;
    if key.kind != KeyEventKind::Press
        || !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    let code = match key.code {
        KeyCode::Char(' ') => Key::Space,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::F(n) => Key::F(n),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        // Crossterm reports Shift+Tab as BackTab.
        KeyCode::BackTab => {
            return Some(Shortcut::new(
                m.contains(KeyModifiers::CONTROL),
                m.contains(KeyModifiers::ALT),
                m.contains(KeyModifiers::SUPER),
                true,
                Key::Tab,
            ));
        }
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Esc => Key::Esc,
        _ => return None,
    };
    Some(Shortcut::new(
        m.contains(KeyModifiers::CONTROL),
        m.contains(KeyModifiers::ALT),
        m.contains(KeyModifiers::SUPER),
        m.contains(KeyModifiers::SHIFT),
        code,
    ))
}

/// Whether a built-in binding already handles this shortcut.
pub fn clashes(shortcut: Shortcut) -> bool {
    let code = match shortcut.key {
        Key::Char(c) => KeyCode::Char(c),
        Key::F(n) => KeyCode::F(n),
        Key::Enter => KeyCode::Enter,
        Key::Tab => KeyCode::Tab,
        Key::Space => KeyCode::Char(' '),
        Key::Backspace => KeyCode::Backspace,
        Key::Delete => KeyCode::Delete,
        Key::Insert => KeyCode::Insert,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        Key::Up => KeyCode::Up,
        Key::Down => KeyCode::Down,
        Key::Left => KeyCode::Left,
        Key::Right => KeyCode::Right,
        Key::Esc => KeyCode::Esc,
    };
    let mut modifiers = KeyModifiers::NONE;
    for (on, flag) in [
        (shortcut.ctrl, KeyModifiers::CONTROL),
        (shortcut.alt, KeyModifiers::ALT),
        (shortcut.super_, KeyModifiers::SUPER),
        (shortcut.shift, KeyModifiers::SHIFT),
    ] {
        modifiers.set(flag, on);
    }
    key_action(KeyEvent::new(code, modifiers)).is_some()
}

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
    fn shortcuts_match_legacy_and_kitty_encodings() {
        let alt_shift_c = Shortcut::parse("alt+shift+c").unwrap();
        // Legacy `ESC C` and Kitty `CSI 99;4u` decode differently.
        for event in [
            KeyEvent::new(KeyCode::Char('C'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        ] {
            assert_eq!(shortcut(event), Some(alt_shift_c));
        }
        assert_ne!(
            shortcut(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT)),
            Some(alt_shift_c)
        );
        assert_eq!(
            shortcut(KeyEvent::new(
                KeyCode::BackTab,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            Some(Shortcut::parse("ctrl+shift+tab").unwrap())
        );
        for event in [
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
            KeyEvent::new_with_kind(KeyCode::Char('c'), KeyModifiers::ALT, KeyEventKind::Release),
            KeyEvent::new_with_kind(KeyCode::Char('c'), KeyModifiers::ALT, KeyEventKind::Repeat),
        ] {
            assert_eq!(shortcut(event), None);
        }
    }

    #[test]
    fn builtin_bindings_clash_and_free_keys_do_not() {
        for text in [
            "ctrl+p",
            "ctrl+t",
            "ctrl+r",
            "ctrl+q",
            "ctrl+c",
            "ctrl+a",
            "ctrl+e",
            "ctrl+u",
            "ctrl+l",
            "ctrl+h",
            "ctrl+left",
            "ctrl+right",
            "ctrl+backspace",
            "ctrl+delete",
            "ctrl+shift+p",
            "ctrl+shift+left",
        ] {
            assert!(clashes(Shortcut::parse(text).unwrap()), "{text}");
        }
        for text in [
            "alt+c",
            "alt+p",
            "super+x",
            "ctrl+i",
            "ctrl+m",
            "ctrl+x",
            "ctrl+f6",
            "ctrl+enter",
        ] {
            assert!(!clashes(Shortcut::parse(text).unwrap()), "{text}");
        }
    }

    #[test]
    fn configured_shortcuts_take_precedence_over_dropping_modified_keys() {
        let mut commands = Commands::default();
        commands.entries.push(launchpad_core::commands::Command {
            id: launchpad_core::commands::Tool::new("claude").unwrap(),
            label: "Claude".into(),
            executable: Some("claude".into()),
            arguments: Vec::new(),
            shortcut: Some(Shortcut::parse("alt+c").unwrap()),
        });
        let tool = commands.entries[1].id;
        assert_eq!(
            event_action(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT),
                &commands
            ),
            Some(Action::Shortcut(tool))
        );
        assert_eq!(
            event_action(
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT),
                &commands
            ),
            None
        );
        assert_eq!(
            event_action(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
                &commands
            ),
            Some(Action::Text("c".into()))
        );
    }

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

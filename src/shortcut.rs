//! Configured launch shortcuts: a key plus at least one of Ctrl, Alt or Super.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Letters are stored lowercase; Shift is a separate modifier.
    Char(char),
    F(u8),
    Enter,
    Tab,
    Space,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Esc,
}
const NAMED: &[(&str, Key)] = &[
    ("enter", Key::Enter),
    ("tab", Key::Tab),
    ("space", Key::Space),
    ("backspace", Key::Backspace),
    ("delete", Key::Delete),
    ("insert", Key::Insert),
    ("home", Key::Home),
    ("end", Key::End),
    ("pageup", Key::PageUp),
    ("pagedown", Key::PageDown),
    ("up", Key::Up),
    ("down", Key::Down),
    ("left", Key::Left),
    ("right", Key::Right),
    ("esc", Key::Esc),
];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub super_: bool,
    pub shift: bool,
    pub key: Key,
}
impl Shortcut {
    /// Build the canonical form of a received key. Uppercase letters imply
    /// Shift, since legacy terminals report Alt+Shift+c as Alt+C.
    pub fn new(ctrl: bool, alt: bool, super_: bool, shift: bool, key: Key) -> Self {
        let (shift, key) = match key {
            Key::Char(c) if c.is_uppercase() => {
                let mut lower = c.to_lowercase();
                match (lower.next(), lower.next()) {
                    (Some(l), None) => (true, Key::Char(l)),
                    _ => (shift, key),
                }
            }
            _ => (shift, key),
        };
        Self {
            ctrl,
            alt,
            super_,
            shift,
            key,
        }
    }
    /// Parse `ctrl+alt+x`, `alt+shift+c` or `super+f6`. Case-insensitive.
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split('+').collect();
        // A trailing empty part means the key itself is `+`, as in `alt++`.
        let (modifiers, key) = match parts.as_slice() {
            [rest @ .., "", ""] if !rest.is_empty() => (rest, "+"),
            [rest @ .., key] => (rest, *key),
            [] => unreachable!("split yields at least one part"),
        };
        let (mut ctrl, mut alt, mut super_, mut shift) = (false, false, false, false);
        for modifier in modifiers {
            let flag = match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => &mut ctrl,
                "alt" => &mut alt,
                "super" => &mut super_,
                "shift" => &mut shift,
                _ => {
                    return Err(format!(
                        "unknown modifier {modifier:?} (use ctrl, alt, super, shift)"
                    ));
                }
            };
            if std::mem::replace(flag, true) {
                return Err(format!("modifier {modifier:?} repeats"));
            }
        }
        if !(ctrl || alt || super_) {
            return Err("require ctrl, alt or super".into());
        }
        let lower = key.to_ascii_lowercase();
        let mut chars = key.chars();
        let key = if let (Some(c), None) = (chars.next(), chars.next()) {
            if c.is_control() || c.is_whitespace() {
                return Err("key must be a visible character or key name".into());
            }
            if shift && !c.is_alphabetic() {
                return Err(format!(
                    "shift applies to letters and named keys; write the shifted character instead of shift+{c}"
                ));
            }
            // Uppercase letters imply Shift through the canonical form.
            return Ok(Self::new(ctrl, alt, super_, shift, Key::Char(c)));
        } else if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
            && (1..=24).contains(&n)
            && !lower.starts_with("f0")
        {
            Key::F(n)
        } else if let Some(&(_, named)) = NAMED.iter().find(|(name, _)| *name == lower) {
            named
        } else {
            return Err(format!("unknown key {key:?}"));
        };
        Ok(Self::new(ctrl, alt, super_, shift, key))
    }
}
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.ctrl, "ctrl+"),
            (self.alt, "alt+"),
            (self.super_, "super+"),
            (self.shift, "shift+"),
        ] {
            if on {
                f.write_str(name)?;
            }
        }
        match self.key {
            Key::Char(c) => write!(f, "{c}"),
            Key::F(n) => write!(f, "f{n}"),
            key => f.write_str(NAMED.iter().find(|(_, k)| *k == key).unwrap().0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_shortcuts() {
        for (text, canonical) in [
            ("alt+c", "alt+c"),
            ("Alt+C", "alt+shift+c"),
            ("alt+shift+c", "alt+shift+c"),
            ("CTRL+ALT+x", "ctrl+alt+x"),
            ("super+F6", "super+f6"),
            ("ctrl+PageDown", "ctrl+pagedown"),
            ("alt+1", "alt+1"),
            ("alt+!", "alt+!"),
            ("alt++", "alt++"),
            ("ctrl+shift+enter", "ctrl+shift+enter"),
        ] {
            assert_eq!(
                Shortcut::parse(text).unwrap().to_string(),
                canonical,
                "{text}"
            );
        }
    }

    #[test]
    fn rejects_missing_modifiers_and_unknown_keys() {
        for text in [
            "c",
            "shift+c",
            "f6",
            "alt+",
            "alt+ ",
            "alt+cc",
            "hyper+c",
            "alt+alt+c",
            "alt+f0",
            "alt+f25",
            "alt+f01",
            "alt+shift+1",
            "",
        ] {
            assert!(Shortcut::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn legacy_uppercase_matches_explicit_shift() {
        let legacy = Shortcut::new(false, true, false, false, Key::Char('C'));
        let kitty = Shortcut::new(false, true, false, true, Key::Char('c'));
        assert_eq!(legacy, kitty);
        assert_eq!(legacy, Shortcut::parse("alt+shift+c").unwrap());
    }
}

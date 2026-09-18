/// Maximum typed/pasted input, counted in Unicode scalar values (not bytes).
pub const MAX_INPUT_CHARS: usize = 100;

#[derive(Debug, Default, Clone)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}
impl Editor {
    pub fn set(&mut self, text: &str) {
        // Completion/history paths are identities: never truncate them into
        // another target. Only user insertion is capped.
        self.text = text.into();
        self.cursor = text.len();
    }
    pub fn left(&mut self) {
        use unicode_segmentation::UnicodeSegmentation;
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
    }
    pub fn right(&mut self) {
        use unicode_segmentation::UnicodeSegmentation;
        self.cursor += self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(0, str::len);
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
        self.snap_forward(self.cursor);
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.text.replace_range(start..self.cursor, "");
        self.snap_forward(start);
    }
    pub fn insert(&mut self, text: &str) {
        let remaining = MAX_INPUT_CHARS.saturating_sub(self.text.chars().count());
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(remaining)
            .collect();
        self.text.insert_str(self.cursor, &text);
        let target = self.cursor + text.len();
        self.snap_forward(target);
    }
    // Both insertion and deletion can join neighboring graphemes.
    fn snap_forward(&mut self, target: usize) {
        use unicode_segmentation::UnicodeSegmentation;
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|&i| i >= target)
            .unwrap_or(self.text.len());
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }
    pub fn clear(&mut self) {
        self.set("");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typing_and_paste_stop_at_100_unicode_scalars() {
        for scalar in ["a", "修", "🦀", "\u{301}"] {
            let mut e = Editor::default();
            for _ in 0..99 {
                e.insert(scalar);
            }
            e.home();
            e.insert(&format!("\n\u{1b}{}", scalar.repeat(4096)));
            assert_eq!(e.text, scalar.repeat(100));
            e.insert("more");
            assert_eq!(e.text, scalar.repeat(100));
            assert!(e.text.is_char_boundary(e.cursor));
        }
        let mut e = Editor::default();
        e.insert(&"a".repeat(100));
        e.backspace();
        e.insert("修x");
        assert_eq!(e.text, format!("{}修", "a".repeat(99)));
    }
    #[test]
    fn deletion_that_joins_neighbors_keeps_cursor_on_grapheme_boundary() {
        let mut e = Editor::default();
        e.set("🇬x🇧");
        e.left();
        e.backspace();
        assert_eq!(e.text, "🇬🇧");
        assert_eq!(e.cursor, e.text.len());
        e.backspace();
        assert_eq!(e.text, "");
    }
    #[test]
    fn edits_whole_graphemes_and_keeps_literal_text() {
        let mut e = Editor::default();
        e.set("修理/e\u{301}👩🏽‍💻");
        e.left();
        assert_eq!(&e.text[e.cursor..], "👩🏽‍💻");
        e.backspace();
        assert_eq!(e.text, "修理/👩🏽‍💻");
        e.delete();
        assert_eq!(e.text, "修理/");
        e.home();
        e.right();
        e.insert("' ; $HOME q");
        assert_eq!(e.text, "修' ; $HOME q理/");
        e.end();
        e.insert("\n\u{1b}x");
        assert!(e.text.ends_with("/x"));
        e.clear();
        e.backspace();
        e.delete();
        e.left();
        e.right();
        assert_eq!(e.cursor, 0);
    }
}

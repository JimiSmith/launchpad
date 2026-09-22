/// Maximum typed/pasted input, counted in Unicode scalar values (not bytes).
pub const MAX_INPUT_CHARS: usize = 100;

#[derive(Debug, Clone)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
    pub(crate) windows_paths: bool,
}
// On Unix this resembles a derived default, but Windows needs backslash support.
#[allow(clippy::derivable_impls)]
impl Default for Editor {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            windows_paths: cfg!(windows),
        }
    }
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
    fn segment_boundary(&self, right: bool) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        let mut target = self.cursor;
        let mut in_segment = false;
        let mut visit = |i: usize, grapheme: &str| {
            // Prepend characters can share a grapheme with a following slash.
            // Detect the separator scalar without splitting its cursor cluster.
            let separator =
                grapheme.contains('/') || (self.windows_paths && grapheme.contains('\\'));
            if separator && in_segment {
                return false;
            }
            in_segment |= !separator;
            target = if right { i + grapheme.len() } else { i };
            true
        };
        if right {
            for (i, grapheme) in self.text[self.cursor..].grapheme_indices(true) {
                if !visit(self.cursor + i, grapheme) {
                    break;
                }
            }
        } else {
            for (i, grapheme) in self.text[..self.cursor].grapheme_indices(true).rev() {
                if !visit(i, grapheme) {
                    break;
                }
            }
        }
        target
    }
    pub fn segment_left(&mut self) {
        self.cursor = self.segment_boundary(false);
    }
    pub fn segment_right(&mut self) {
        self.cursor = self.segment_boundary(true);
    }
    pub fn delete_segment_left(&mut self) {
        let start = self.segment_boundary(false);
        self.text.replace_range(start..self.cursor, "");
        self.snap_forward(start);
    }
    pub fn delete_segment_right(&mut self) {
        let end = self.segment_boundary(true);
        self.text.replace_range(self.cursor..end, "");
        self.snap_forward(self.cursor);
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
    fn segment_operations_follow_directional_grapheme_safe_boundaries() {
        use unicode_segmentation::UnicodeSegmentation;
        for (windows, before, left, right) in [
            (false, "foo/ba|r/baz", "foo/|bar/baz", "foo/bar|/baz"),
            (false, "foo|/bar", "|foo/bar", "foo/bar|"),
            (false, "foo/|bar", "|foo/bar", "foo/bar|"),
            (false, "foo/bar/|", "foo/|bar/", "foo/bar/|"),
            (false, "foo/|//bar", "|foo///bar", "foo///bar|"),
            (false, "|/foo", "|/foo", "/foo|"),
            (false, "/|foo", "|/foo", "/foo|"),
            (false, "///|", "|///", "///|"),
            (false, "|", "|", "|"),
            (false, "~|/notes", "|~/notes", "~/notes|"),
            (
                false,
                "|my notes.v2-old",
                "|my notes.v2-old",
                "my notes.v2-old|",
            ),
            (
                false,
                "修理/e\u{301}|👩🏽‍💻/🇬🇧",
                "修理/|e\u{301}👩🏽‍💻/🇬🇧",
                "修理/e\u{301}👩🏽‍💻|/🇬🇧",
            ),
            (false, "a/\u{301}|b", "|a/\u{301}b", "a/\u{301}b|"),
            (
                false,
                "foo\u{600}/bar|",
                "foo\u{600}/|bar",
                "foo\u{600}/bar|",
            ),
            (
                false,
                "|foo\u{600}/bar",
                "|foo\u{600}/bar",
                "foo|\u{600}/bar",
            ),
            (
                false,
                "foo|\u{600}/bar",
                "|foo\u{600}/bar",
                "foo\u{600}/bar|",
            ),
            (
                true,
                "foo\u{600}\\bar|",
                "foo\u{600}\\|bar",
                "foo\u{600}\\bar|",
            ),
            (
                false,
                "foo\u{600}\\bar|",
                "|foo\u{600}\\bar",
                "foo\u{600}\\bar|",
            ),
            (false, "|foo\\bar/baz", "|foo\\bar/baz", "foo\\bar|/baz"),
            (
                true,
                "C:\\foo\\ba|r/baz",
                "C:\\foo\\|bar/baz",
                "C:\\foo\\bar|/baz",
            ),
            (true, "C:|\\foo", "|C:\\foo", "C:\\foo|"),
            (
                true,
                "|\\\\server\\share",
                "|\\\\server\\share",
                "\\\\server|\\share",
            ),
            (true, "foo\\|/bar", "|foo\\/bar", "foo\\/bar|"),
        ] {
            let text = before.replace('|', "");
            let cursor = before.find('|').unwrap();
            let start = left.find('|').unwrap();
            let end = right.find('|').unwrap();
            assert_eq!(left.replace('|', ""), text);
            assert_eq!(right.replace('|', ""), text);
            for operation in 0..4 {
                let mut editor = Editor {
                    text: text.clone(),
                    cursor,
                    windows_paths: windows,
                };
                let (expected, expected_cursor) = match operation {
                    0 => {
                        editor.segment_left();
                        (text.clone(), start)
                    }
                    1 => {
                        editor.segment_right();
                        (text.clone(), end)
                    }
                    2 => {
                        editor.delete_segment_left();
                        (format!("{}{}", &text[..start], &text[cursor..]), start)
                    }
                    _ => {
                        editor.delete_segment_right();
                        (format!("{}{}", &text[..cursor], &text[end..]), cursor)
                    }
                };
                assert_eq!(editor.text, expected, "{before}: operation {operation}");
                assert_eq!(
                    editor.cursor, expected_cursor,
                    "{before}: operation {operation}"
                );
                assert!(
                    editor.cursor == editor.text.len()
                        || editor
                            .text
                            .grapheme_indices(true)
                            .any(|(i, _)| i == editor.cursor)
                );
            }
        }
    }
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

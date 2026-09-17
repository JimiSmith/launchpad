//! Terminal-cell geometry matching Ratatui's per-grapheme width model.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn width(s: &str) -> usize {
    s.graphemes(true)
        .filter(|g| !g.contains(char::is_control))
        .map(UnicodeWidthStr::width)
        .sum()
}
pub fn clip(s: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    let truncated = width(s) > budget;
    let mut remaining = budget - usize::from(truncated);
    let mut out = String::new();
    for g in s.graphemes(true).filter(|g| !g.contains(char::is_control)) {
        let w = width(g);
        if w == 0 {
            continue;
        }
        if w > remaining {
            break;
        }
        remaining -= w;
        out.push_str(g);
    }
    if truncated {
        out.push('…');
    }
    out
}
/// Horizontal scrolling reserves one visible cell for the cursor.
/// Cursor is a grapheme-boundary byte offset; geometry is always in cells.
pub fn input_window(s: &str, cursor: usize, budget: usize) -> (String, usize) {
    if budget == 0 {
        return (String::new(), 0);
    }
    let start = input_start(s, cursor, budget);
    let caret = width(&s[start..cursor]);
    let mut out = s[start..cursor].to_owned();
    out.push_str(&clip(&s[cursor..], budget - caret));
    (out, caret)
}

fn input_start(s: &str, cursor: usize, budget: usize) -> usize {
    let mut start = 0;
    let mut remaining = width(&s[..cursor]);
    for (i, g) in s[..cursor].grapheme_indices(true) {
        if remaining < budget {
            break;
        }
        remaining -= width(g);
        start = i + g.len();
    }
    start
}

/// Wide-cell continuations map before the whole grapheme. An ellipsis maps
/// to the first omitted grapheme, never into hidden text.
pub fn input_cursor(s: &str, cursor: usize, budget: usize, cell: usize) -> usize {
    let start = input_start(s, cursor, budget);
    let visible_budget = budget.saturating_sub(usize::from(width(&s[start..]) > budget));
    let mut x = 0;
    for (i, g) in s[start..].grapheme_indices(true) {
        let w = width(g);
        if w == 0 {
            continue;
        }
        if x + w > visible_budget || cell < x + w {
            return start + i;
        }
        x += w;
    }
    s.len()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipping_keeps_graphemes_and_reserves_ellipsis_cells() {
        for s in ["修理abc", "e\u{301}xyz", "👩🏽‍💻abc", "🇬🇧abc", "✈️abc"] {
            assert_eq!(clip(s, width(s)), s);
            for n in 0..width(s) {
                assert!(width(&clip(s, n)) <= n);
            }
        }
        assert_eq!(clip("修理ab", 4), "修…");
        assert_eq!(clip("修理ab", 1), "…");
        assert_eq!(clip("修理ab", 0), "");
        assert_eq!(clip("a\u{1b}b", 3), "ab");
        let s = "~/Projects/修理/e\u{301}👩🏽‍💻";
        let (visible, cursor) = input_window(s, s.len(), 8);
        assert!(visible.ends_with("e\u{301}👩🏽‍💻"));
        assert!(cursor < 8);
        assert_eq!(width(&visible), cursor);
        assert_eq!(input_window(s, 0, 0), (String::new(), 0));
    }
}

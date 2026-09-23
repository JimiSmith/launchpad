//! Keys screen content, laid out on the terminal-cell grid shared with the renderer.
use crate::{
    app::App,
    cells::{clip, width},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    Muted,
    Accent,
    Error,
}
/// One screen row of styled segments, exactly as wide as requested.
pub type Row = Vec<(String, Tone)>;

enum Item {
    Key(String, String),
    /// Wrapped under the action column.
    Note(String),
}
type Group = (&'static str, Vec<Item>);

const GUTTER: usize = 4;
/// Narrower content stacks the groups in one column.
const TWO_COLUMNS: usize = 72;

fn keys(rows: &[(&'static str, &str)]) -> Vec<Item> {
    rows.iter()
        .map(|&(key, action)| Item::Key(key.into(), action.into()))
        .collect()
}

fn groups(app: &App) -> (Vec<Group>, Vec<Group>) {
    let mut launch = vec![Item::Key("↵".into(), "Launch selected tool".into())];
    let mut any = false;
    for c in &app.commands.entries {
        if let Some(s) = c.shortcut {
            launch.push(Item::Key(s.label(), c.label.clone()));
            any = true;
        }
    }
    launch.push(Item::Note(if any {
        "Shortcuts use the typed path, or the selected recent row.".into()
    } else {
        r#"Add shortcut = "alt+c" to a command for one-key launches."#.into()
    }));
    let left = vec![
        ("Launch", launch),
        ("Tool", keys(&[("← →", "Choose tool")])),
        (
            "Directory",
            keys(&[
                ("↑ ↓", "Highlight suggestion"),
                ("↵", "Accept highlight"),
                ("← →", "Move cursor"),
                ("Ctrl+← →", "Move by segment"),
                ("Ctrl+Bksp", "Delete segment left"),
                ("Ctrl+Del", "Delete segment right"),
                ("Ctrl+A / E", "Start / end"),
                ("Ctrl+U", "Clear"),
            ]),
        ),
    ];
    let right = vec![
        (
            "Move",
            keys(&[
                ("Tab", "Next section"),
                ("Shift+Tab", "Previous section"),
                ("Ctrl+P", "Directory"),
                ("Ctrl+T", "Tool"),
                ("Ctrl+R", "Recent"),
            ]),
        ),
        (
            "Recent",
            keys(&[
                ("↑ ↓", "Choose entry"),
                ("Home / End", "First / last"),
                ("Delete", "Remove entry"),
                ("Ctrl+L ×2", "Clear all"),
            ]),
        ),
        (
            "Launchpad",
            keys(&[
                ("F1 / Esc", "Close keys"),
                ("F5", "Refresh and reset"),
                ("Esc", "Dismiss, then quit"),
                ("Ctrl+Q", "Quit"),
            ]),
        ),
    ];
    (left, right)
}

fn parts(item: &Item) -> Option<(&str, &str)> {
    match item {
        Item::Key(key, action) => Some((key, action)),
        Item::Note(_) => None,
    }
}

/// Word-wrap by terminal cells, breaking words longer than a line.
pub fn wrap(text: &str, budget: usize) -> Vec<String> {
    let budget = budget.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && width(&line) + 1 + width(word) > budget {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        for g in word.graphemes(true) {
            if width(&line) + width(g) > budget && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            line.push_str(g);
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Fill segments to exactly `budget` cells, clipping the last visible one.
fn pad(segments: Row, budget: usize) -> Row {
    let mut out = Vec::new();
    let mut used = 0;
    for (text, tone) in segments {
        let text = clip(&text, budget - used);
        if text.is_empty() {
            break;
        }
        used += width(&text);
        out.push((text, tone));
    }
    if used < budget {
        out.push((" ".repeat(budget - used), Tone::Text));
    }
    out
}

fn padded(text: &str, budget: usize) -> String {
    format!("{text}{}", " ".repeat(budget.saturating_sub(width(text))))
}

/// Rows for one column and the width of its key column.
fn column(groups: &[&Group], budget: usize) -> (Vec<Row>, usize) {
    let kw = groups
        .iter()
        .flat_map(|(_, items)| items.iter().filter_map(parts))
        .map(|(key, _)| width(key))
        .max()
        .unwrap_or(0)
        + 2;
    let mut rows = Vec::new();
    for (i, (title, items)) in groups.iter().enumerate() {
        if i > 0 {
            rows.push(Vec::new());
        }
        rows.push(vec![(title.to_string(), Tone::Accent)]);
        for item in items {
            match (item, parts(item)) {
                (Item::Note(note), _) => {
                    for line in wrap(note, budget.saturating_sub(kw)) {
                        rows.push(vec![(" ".repeat(kw), Tone::Text), (line, Tone::Muted)]);
                    }
                }
                (_, Some((key, action))) => rows.push(vec![
                    (padded(key, kw), Tone::Text),
                    (action.into(), Tone::Muted),
                ]),
                _ => {}
            }
        }
    }
    (rows.into_iter().map(|r| pad(r, budget)).collect(), kw)
}

/// The keys screen body for a content area `budget` cells wide.
pub fn rows(app: &App, budget: usize) -> Vec<Row> {
    let (left, right) = groups(app);
    let (mut body, kw) = if budget >= TWO_COLUMNS {
        let cw = (budget - GUTTER) / 2;
        let (a, kw) = column(&left.iter().collect::<Vec<_>>(), cw);
        let (b, _) = column(&right.iter().collect::<Vec<_>>(), cw);
        let rest = budget - cw * 2 - GUTTER;
        let rows = (0..a.len().max(b.len()))
            .map(|i| {
                let mut row = a.get(i).cloned().unwrap_or_else(|| pad(Vec::new(), cw));
                row.push((" ".repeat(GUTTER), Tone::Text));
                row.extend(b.get(i).cloned().unwrap_or_else(|| pad(Vec::new(), cw)));
                row.extend(pad(Vec::new(), rest));
                row
            })
            .collect();
        (rows, kw)
    } else {
        // One column follows the dashboard: launch, tool, moving, then the sections.
        let order = [
            &left[0], &left[1], &right[0], &left[2], &right[1], &right[2],
        ];
        column(&order, budget)
    };

    let mut status = vec![Vec::new(), vec![("Status".to_string(), Tone::Accent)]];
    let mut labelled = |key: &str, text: &str, key_tone, tone| {
        for (i, line) in wrap(text, budget.saturating_sub(kw))
            .into_iter()
            .enumerate()
        {
            let key = if i == 0 { key } else { "" };
            status.push(vec![(padded(key, kw), key_tone), (line, tone)]);
        }
    };
    labelled("Search", &app.search_status, Tone::Text, Tone::Muted);
    if app.simulate_launch {
        labelled(
            "Preview",
            "Directories are validated; nothing starts and recent launches stay in memory.",
            Tone::Text,
            Tone::Muted,
        );
    }
    for error in app.config_errors() {
        labelled("!", error, Tone::Error, Tone::Error);
    }
    body.extend(status.into_iter().map(|r| pad(r, budget)));
    body
}

/// Plain text of every row, for tests and diagnostics.
pub fn lines(app: &App, budget: usize) -> Vec<String> {
    rows(app, budget)
        .into_iter()
        .map(|row| row.into_iter().map(|(text, _)| text).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_exactly_the_requested_width() {
        let app = App::default();
        for budget in [36, 60, 71, 72, 92] {
            for line in lines(&app, budget) {
                assert_eq!(width(&line), budget, "{line:?}");
            }
        }
    }

    #[test]
    fn wrap_breaks_on_spaces_and_inside_long_words() {
        assert_eq!(wrap("aa bb cc", 5), ["aa bb", "cc"]);
        assert_eq!(wrap("abcdefg", 3), ["abc", "def", "g"]);
        assert_eq!(wrap("", 3), [""]);
    }
}

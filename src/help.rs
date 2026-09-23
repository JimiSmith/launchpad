//! Input help shared by navigation and renderer.
const LINES: &[&str] = &[
    "F1 / Esc  Back to the launcher",
    "Ctrl+Q / Ctrl+C  Quit Launchpad",
    "Ctrl+P / Ctrl+T / Ctrl+R  Path / tools / recent",
    "",
    "PATH   Type a home directory path or fuzzy query, e.g. notes / nts",
    "Tab / Shift+Tab  Next / previous section",
    "↑ / ↓  Highlight suggestions; Enter accepts",
    "Enter without a highlight  Validate and replace this pane",
    "← / →  Move by grapheme · Home / End  Beginning / end",
    "Ctrl+← / Ctrl+→  Move by path segment",
    "Ctrl+A / Ctrl+E  Home / end · Ctrl+U  Clear input",
    "Backspace / Delete  Remove a whole grapheme",
    "Ctrl+Backspace / Ctrl+Delete  Delete segment left / right",
    "Ctrl+H also deletes left (legacy Ctrl+Backspace)",
    "Segment edits skip separators; spaces stay within a segment.",
    "",
    "TOOLS  ← / → select · ↑ path · ↓ recent · Enter launch",
    "RECENT ↑ / ↓ select · Home / End · Enter replay",
    "Command shortcuts launch the typed path, or the selected recent",
    "directory, with that tool. Highlighted suggestions are not accepted.",
    "Delete removes selected recent entry · Ctrl+L clear (confirm)",
    "",
    "F5  Refresh HOME / reset form; loaded configuration is preserved",
    "Esc dismisses transient UI, then quits an untouched dashboard",
    "The dashboard closes on launch; it does not return on command exit.",
    "",
    "MOUSE Left click: place path cursor, accept a suggestion,",
    "select a tool or select a recent row. Selection never launches.",
    "Selecting a recent row fills Directory and Tool; Enter or Launch opens it.",
    "Wheel over suggestions / recent / help scrolls that section.",
    "Click F1 help or Esc back. F5 resets; Ctrl+Q quits.",
    "Drag, release, right click ignored; host handles modifiers.",
    "",
    "Search: HOME only; relative paths start at HOME, not CWD.",
    "Hidden/ignored directories can still be entered literally.",
    "Symlinks, non-UTF-8 and control-character names are skipped.",
    "",
    "Shell uses Zellij's default shell; configured commands use literal argv.",
    "No command availability checks. Recent validated attempts persist in shared history.",
];

pub fn lines(app: &crate::app::App) -> Vec<String> {
    let mut lines: Vec<String> = LINES.iter().map(|s| (*s).into()).collect();
    if app.simulate_launch {
        lines.push(String::new());
        lines.push("simulate_launch: directories are validated but no process".into());
        lines.push("is started, and recent launches stay in memory only.".into());
    }
    lines.push(String::new());
    lines.push(format!("Search: {}", app.search_status));
    lines.push(String::new());
    lines.push("Configured commands (Shell first):".into());
    for c in &app.commands.entries {
        match c.shortcut {
            Some(s) => lines.push(format!("{}: {}  {s}", c.id.as_str(), c.label)),
            None => lines.push(format!("{}: {}", c.id.as_str(), c.label)),
        }
    }
    for error in app.config_errors() {
        lines.push(format!("Config error: {error}"));
    }
    lines
}

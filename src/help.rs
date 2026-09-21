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
    "← / → / Home / End  Move by grapheme",
    "Ctrl+A / Ctrl+E  Home / end · Ctrl+U  Clear input",
    "Backspace / Delete  Remove a whole grapheme",
    "",
    "TOOLS  ← / → select · ↑ path · ↓ recent · Enter launch",
    "RECENT ↑ / ↓ select · Home / End · Enter replay",
    "Delete removes selected recent entry · Ctrl+L clear (confirm)",
    "",
    "F5  Refresh HOME / reset form; loaded configuration is preserved",
    "Esc dismisses transient UI, then quits an untouched dashboard",
    "The dashboard closes on launch; it does not return on command exit.",
    "",
    "MOUSE Left click: place path cursor, accept a suggestion,",
    "select a tool or select a recent row. Selection never launches.",
    "Click Enter ↵ to launch; Enter replays the selected recent entry.",
    "on the selected row. Launch replaces this pane.",
    "Wheel over suggestions / recent / help scrolls that section.",
    "Click F1 help, F5 reset, Ctrl+Q quit (^Q), or Esc back.",
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
    lines.push("Configured commands (Shell first):".into());
    for c in &app.commands.entries {
        lines.push(format!("{}: {}", c.id.as_str(), c.label));
    }
    for error in app.config_errors() {
        lines.push(format!("Config error: {error}"));
    }
    lines
}

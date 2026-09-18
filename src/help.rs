//! Input help shared by navigation and renderer.
pub const LINES: &[&str] = &[
    "F1 / Esc  Back to the launcher",
    "Ctrl+Q / Ctrl+C  Quit",
    "Ctrl+P / Ctrl+T / Ctrl+R  Path / tools / recent",
    "",
    "PATH   Type a home directory path or fuzzy query, e.g. notes / nts",
    "Tab / Shift+Tab  Accept next / previous completion",
    "↑ / ↓  Highlight suggestions; Enter accepts only",
    "Enter without a highlight  Validate and simulate launch",
    "← / → / Home / End  Move by grapheme",
    "Ctrl+A / Ctrl+E  Home / end · Ctrl+U  Clear input",
    "Backspace / Delete  Remove a whole grapheme",
    "",
    "TOOLS  ← / → select · ↑ path · ↓ recent · Enter launch",
    "RECENT ↑ / ↓ select · Home / End · Enter replay",
    "Tab copy path + tool · Delete remove · Ctrl+L clear (confirm)",
    "",
    "F5  Refresh HOME / reset form · F6  Toggle Copilot availability",
    "Esc dismisses transient UI, then closes an untouched pane",
    "Esc returns from the simulated terminal / closed pane",
    "",
    "MOUSE Left click: place path cursor, accept a suggestion,",
    "select a tool or select a recent row. Selection never launches.",
    "Click Enter ↵ to launch; recent Tab copy / Enter replay act",
    "on the selected row. All launches are still simulated.",
    "Wheel over suggestions / recent / help scrolls that section.",
    "Click F1 help, F5 reset, Ctrl+Q quit (^Q), or Esc back.",
    "Drag, release, right click ignored; host handles modifiers.",
    "",
    "Search: HOME only; relative paths start at HOME, not CWD.",
    "Hidden directories require a dot-prefixed query component.",
    "Symlinks, non-UTF-8 and control-character names are skipped.",
    "",
    "Directory search is real. No PATH discovery or processes.",
    "Launches, availability and history remain simulated; memory only.",
];

pub fn line(index: usize, simulation: &str, host_launch: bool) -> &str {
    if !host_launch {
        return simulation;
    }
    match index {
        7 => "Enter without a highlight  Validate and replace this pane",
        16 => "F5  Refresh HOME / reset form; all five tools are selectable",
        18 => "The dashboard closes on launch; it does not return on command exit.",
        23 => "on the selected row. Launch replaces this plugin pane.",
        29 => "Hidden/ignored directories can still be entered literally.",
        32 => "Shell uses Zellij's default shell; agents run with no flags.",
        33 => "No command availability checks. No persistent launch history.",
        _ => simulation,
    }
}

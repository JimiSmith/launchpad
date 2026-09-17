use crate::{
    editor::Editor,
    fixtures::{self, Directory},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Shell,
    Claude,
    Codex,
    Copilot,
    Hermes,
}
impl Tool {
    pub const ALL: [Self; 5] = [
        Self::Shell,
        Self::Claude,
        Self::Codex,
        Self::Copilot,
        Self::Hermes,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Shell => "Shell",
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Copilot => "Copilot",
            Self::Hermes => "Hermes",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Path,
    Tools,
    History,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub id: u64,
    pub path: String,
    pub tool: Tool,
    pub age: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Terminal(Launch),
    Closed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Scroll(ScrollTarget, bool),
    PathCursor(usize),
    AcceptSuggestion(usize),
    SelectTool(Tool),
    SelectHistory(u64),
    LaunchForm,
    Text(String),
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Backspace,
    Delete,
    Clear,
    Tab,
    BackTab,
    Enter,
    Escape,
    Focus(Focus),
    Help,
    Reset,
    ToggleCopilot,
    ClearHistory,
    Quit,
}
#[derive(Debug)]
pub struct App {
    pub editor: Editor,
    pub dirs: Vec<Directory>,
    pub suggestions: Vec<usize>,
    pub highlighted: Option<usize>,
    pub focus: Focus,
    pub tool: Tool,
    pub history: Vec<Launch>,
    pub recent: usize,
    pub screen: Screen,
    pub message: Option<String>,
    pub help: bool,
    pub help_scroll: usize,
    pub copilot_available: bool,
    pub quit: bool,
    pub compact: bool,
    pub touched: bool,
    pub confirm_clear: bool,
    cycle: Option<(Vec<usize>, usize)>,
    next_id: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTarget {
    Suggestions,
    History,
    Help,
}
impl Default for App {
    fn default() -> Self {
        let mut editor = Editor::default();
        editor.set("~/Projects/");
        let dirs = fixtures::directories();
        let suggestions = fixtures::matches(&editor.text, &dirs);
        Self {
            editor,
            dirs,
            suggestions,
            highlighted: None,
            focus: Focus::Path,
            tool: Tool::Shell,
            history: fixtures::history(),
            recent: 0,
            screen: Screen::Dashboard,
            message: None,
            help: false,
            help_scroll: 0,
            copilot_available: true,
            quit: false,
            compact: false,
            touched: false,
            confirm_clear: false,
            cycle: None,
            next_id: 10,
        }
    }
}
impl App {
    pub fn path_label(&self, path: &str) -> String {
        fixtures::short(path)
    }

    pub fn update(&mut self, action: Action) {
        if action == Action::Quit {
            self.quit = true;
            return;
        }
        if action == Action::Reset {
            let compact = self.compact;
            *self = Self::default();
            self.compact = compact;
            return;
        }
        if self.compact {
            return;
        }
        if action == Action::Help {
            self.help = !self.help;
            return;
        }
        if self.help {
            match action {
                Action::Scroll(ScrollTarget::Help, down) => {
                    self.update(if down { Action::Down } else { Action::Up })
                }
                Action::Escape => self.help = false,
                Action::Up => self.help_scroll = self.help_scroll.saturating_sub(1),
                Action::Down => {
                    self.help_scroll = (self.help_scroll + 1).min(crate::help::LINES.len() - 1)
                }
                Action::Home => self.help_scroll = 0,
                Action::End => self.help_scroll = crate::help::LINES.len() - 1,
                _ => {}
            }
            return;
        }
        if self.screen != Screen::Dashboard {
            if action == Action::Escape {
                self.screen = Screen::Dashboard;
                self.tool = Tool::Shell;
                self.focus = Focus::Path;
                self.message = None;
            }
            return;
        }
        if action == Action::Escape {
            if self.confirm_clear {
                self.confirm_clear = false;
                self.message = None;
            } else if !self.suggestions.is_empty() || self.cycle.is_some() {
                self.suggestions.clear();
                self.highlighted = None;
                self.cycle = None;
            } else if self.message.is_some() {
                self.message = None;
            } else if !self.touched {
                self.screen = Screen::Closed;
            } else {
                self.message = Some("Form kept. Ctrl+Q quits; F5 resets the demo.".into());
            }
            return;
        }
        if action != Action::ClearHistory && self.confirm_clear {
            self.confirm_clear = false;
            self.message = None;
        }
        if action == Action::ToggleCopilot {
            self.copilot_available = !self.copilot_available;
            if !self.available(self.tool) {
                self.tool = Tool::Shell;
                self.message =
                    Some("Copilot removed. Reset to Shell; review before launching.".into());
            } else {
                self.message = Some(
                    if self.copilot_available {
                        "Copilot restored (fixture)."
                    } else {
                        "Copilot removed (fixture). History replay will revalidate."
                    }
                    .into(),
                );
            }
            return;
        }
        if let Action::Focus(focus) = action {
            self.focus = focus;
            return;
        }
        if action == Action::LaunchForm {
            self.launch(self.editor.text.clone(), self.tool);
            return;
        }
        if let Action::Scroll(target, down) = action {
            match target {
                ScrollTarget::History if !self.history.is_empty() => {
                    self.focus = Focus::History;
                    self.update(if down { Action::Down } else { Action::Up });
                }
                ScrollTarget::Suggestions if !self.suggestions.is_empty() => {
                    self.focus = Focus::Path;
                    let last = self.suggestions.len() - 1;
                    self.highlighted =
                        Some(self.highlighted.map_or(if down { 0 } else { last }, |i| {
                            if down {
                                (i + 1).min(last)
                            } else {
                                i.saturating_sub(1)
                            }
                        }));
                }
                _ => {}
            }
            return;
        }
        if let Action::SelectHistory(id) = action {
            if let Some(index) = self.history.iter().position(|e| e.id == id) {
                self.recent = index;
                self.focus = Focus::History;
            }
            return;
        }
        if let Action::SelectTool(tool) = action {
            if self.available(tool) {
                self.tool = tool;
                self.focus = Focus::Tools;
                self.touched = true;
                self.message = None;
            }
            return;
        }
        if let Action::AcceptSuggestion(index) = action {
            if self.suggestions.contains(&index) && index < self.dirs.len() {
                self.accept(index);
                self.focus = Focus::Path;
                self.cycle = None;
            }
            return;
        }
        if let Action::PathCursor(cursor) = action {
            use unicode_segmentation::UnicodeSegmentation;
            if cursor == self.editor.text.len()
                || self
                    .editor
                    .text
                    .grapheme_indices(true)
                    .any(|(i, _)| i == cursor)
            {
                self.editor.cursor = cursor;
                self.focus = Focus::Path;
            }
            return;
        }
        match self.focus {
            Focus::Path => match action {
                Action::Text(text) => {
                    self.editor.insert(&text);
                    self.edited();
                }
                Action::Clear => {
                    self.editor.clear();
                    self.edited();
                }
                Action::Backspace => {
                    self.editor.backspace();
                    self.edited();
                }
                Action::Delete => {
                    self.editor.delete();
                    self.edited();
                }
                Action::Left => self.editor.left(),
                Action::Right => self.editor.right(),
                Action::Home => self.editor.home(),
                Action::End => self.editor.end(),
                Action::Down | Action::Up if !self.suggestions.is_empty() => {
                    let n = self.suggestions.len();
                    let down = action == Action::Down;
                    self.highlighted =
                        Some(self.highlighted.map_or(if down { 0 } else { n - 1 }, |i| {
                            (i + if down { 1 } else { n - 1 }) % n
                        }));
                }
                Action::Down => self.focus = Focus::Tools,
                Action::Tab | Action::BackTab => self.complete(action == Action::BackTab),
                Action::Enter => {
                    if let Some(i) = self.highlighted {
                        self.accept(self.suggestions[i]);
                    } else {
                        self.launch(self.editor.text.clone(), self.tool);
                    }
                }
                _ => {}
            },
            Focus::Tools => match action {
                Action::Left | Action::Right => {
                    let tools = self.visible_tools();
                    let i = tools.iter().position(|&t| t == self.tool);
                    let n = tools.len();
                    let next = i.map_or(0, |i| {
                        (i + if action == Action::Right { 1 } else { n - 1 }) % n
                    });
                    self.tool = tools[next];
                    self.touched = true;
                    self.message = None;
                }
                Action::Up => self.focus = Focus::Path,
                Action::Down => self.focus = Focus::History,
                Action::Enter => self.launch(self.editor.text.clone(), self.tool),
                _ => {}
            },
            Focus::History => match action {
                Action::Up => self.recent = self.recent.saturating_sub(1),
                Action::Down => {
                    self.recent = (self.recent + 1).min(self.history.len().saturating_sub(1))
                }
                Action::Home => self.recent = 0,
                Action::End => self.recent = self.history.len().saturating_sub(1),
                Action::Enter => {
                    if let Some(e) = self.history.get(self.recent).cloned() {
                        self.launch(e.path, e.tool);
                    }
                }
                Action::Tab => {
                    if let Some(e) = self.history.get(self.recent) {
                        self.editor.set(&fixtures::short(&e.path));
                        self.tool = e.tool;
                        self.focus = Focus::Path;
                        self.touched = true;
                        self.suggestions.clear();
                        self.highlighted = None;
                        self.cycle = None;
                        self.message = Some(if self.available(e.tool) {
                            "Copied to form. Enter launches; edit freely.".into()
                        } else {
                            format!(
                                "{} is unavailable. Choose a tool with Ctrl+T.",
                                e.tool.label()
                            )
                        });
                    }
                }
                Action::Delete => {
                    if !self.history.is_empty() {
                        self.history.remove(self.recent);
                    }
                    self.recent = self.recent.min(self.history.len().saturating_sub(1));
                }
                Action::ClearHistory => {
                    if self.confirm_clear {
                        self.history.clear();
                        self.recent = 0;
                        self.confirm_clear = false;
                        self.message = None;
                    } else {
                        self.confirm_clear = true;
                        self.message =
                            Some("Clear history? Ctrl+L again confirms; Esc cancels.".into());
                    }
                }
                _ => {}
            },
        }
    }
    pub fn visible_tools(&self) -> Vec<Tool> {
        Tool::ALL
            .into_iter()
            .filter(|&t| self.available(t))
            .collect()
    }
    pub fn available(&self, tool: Tool) -> bool {
        tool != Tool::Copilot || self.copilot_available
    }
    fn complete(&mut self, reverse: bool) {
        let (items, i) = if let Some((items, i)) = self.cycle.take() {
            let n = items.len();
            (items, (i + if reverse { n - 1 } else { 1 }) % n)
        } else if !self.suggestions.is_empty() {
            let n = self.suggestions.len();
            let i = self
                .highlighted
                .map_or(if reverse { n - 1 } else { 0 }, |i| {
                    (i + if reverse { n - 1 } else { 1 }) % n
                });
            (self.suggestions.clone(), i)
        } else {
            return;
        };
        self.accept(items[i]);
        self.cycle = Some((items, i));
    }
    fn edited(&mut self) {
        self.touched = true;
        self.message = None;
        self.highlighted = None;
        self.cycle = None;
        self.suggestions = fixtures::matches(&self.editor.text, &self.dirs);
    }
    fn accept(&mut self, index: usize) {
        self.editor.set(&fixtures::short(self.dirs[index].path));
        self.suggestions.clear();
        self.highlighted = None;
        self.message = None;
        self.touched = true;
    }
    fn launch(&mut self, raw: String, tool: Tool) {
        let path = fixtures::normalize(&raw);
        let Some(dir) = self.dirs.iter().find(|d| Some(d.path) == path.as_deref()) else {
            self.message =
                Some("Directory not found. Choose a suggestion or enter a fixture path.".into());
            return;
        };
        if let Some(error) = dir.error {
            self.message = Some(error.into());
            return;
        }
        if !self.available(tool) {
            self.message = Some(format!(
                "{} is unavailable. Tab copies; Ctrl+T chooses a tool.",
                tool.label()
            ));
            return;
        }
        self.next_id += 1;
        let event = Launch {
            id: self.next_id,
            path: dir.path.into(),
            tool,
            age: "Just now".into(),
        };
        self.history.insert(0, event.clone());
        self.history.truncate(10);
        self.recent = 0;
        self.message = None;
        self.screen = Screen::Terminal(event);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn query(a: &mut App, text: &str) {
        a.update(Action::Clear);
        a.update(Action::Text(text.into()));
    }
    #[test]
    fn hidden_controls_never_launch_and_escape_help_quit_are_safe() {
        let mut a = App {
            compact: true,
            ..App::default()
        };
        a.update(Action::Enter);
        assert_eq!(a.screen, Screen::Dashboard);
        a.update(Action::Reset);
        assert!(a.compact, "reset must not bypass compact guard");
        a.compact = false;
        a.update(Action::Help);
        assert!(a.help);
        a.update(Action::Enter);
        assert_eq!(a.screen, Screen::Dashboard);
        a.update(Action::Escape);
        assert!(!a.help);
        a.update(Action::Escape);
        assert!(a.suggestions.is_empty());
        a.update(Action::Escape);
        assert_eq!(a.screen, Screen::Closed);
        a.update(Action::Escape);
        assert_eq!(a.screen, Screen::Dashboard);
        a.update(Action::Quit);
        assert!(a.quit);
    }
    #[test]
    fn invalid_paths_preserve_form_and_literal_paths_launch_all_five_tools() {
        let mut a = App::default();
        for (path, error) in [
            ("~/restricted", "Permission"),
            ("~/broken-link", "symlink"),
            ("~/Projects/missing", "missing"),
            ("ntsx", "not found"),
        ] {
            query(&mut a, path);
            a.update(Action::Enter);
            assert_eq!(a.screen, Screen::Dashboard);
            assert_eq!(a.editor.text, path);
            assert!(a.message.as_ref().is_some_and(|s| s.contains(error)));
            assert_eq!(a.history.len(), 10);
        }
        for tool in Tool::ALL {
            query(&mut a, "~/Projects/it's literal; $HOME");
            a.tool = tool;
            a.update(Action::Enter);
            assert!(
                matches!(&a.screen, Screen::Terminal(e) if e.tool == tool && e.path.ends_with("it's literal; $HOME"))
            );
            a.update(Action::Escape);
        }
    }
    #[test]
    fn escape_cancels_history_clear_with_default_suggestions() {
        let mut a = App::default();
        let history = a.history.clone();
        let suggestions = a.suggestions.clone();
        assert!(!history.is_empty());
        assert!(!suggestions.is_empty());

        a.update(Action::Focus(Focus::History));
        a.update(Action::ClearHistory);
        assert!(a.confirm_clear);
        let confirmation = a.message.clone();
        assert!(confirmation.is_some());
        assert_eq!(a.history, history);

        a.update(Action::Escape);
        assert!(!a.confirm_clear);
        assert!(a.message.is_none());
        assert_eq!(a.suggestions, suggestions);
        assert_eq!(a.history, history);
        assert_eq!(a.screen, Screen::Dashboard);

        a.update(Action::ClearHistory);
        assert_eq!(a.history, history);
        assert!(a.confirm_clear);
        assert_eq!(a.message, confirmation);
    }
    #[test]
    fn history_replays_copies_prunes_duplicates_and_revalidates_tools() {
        let mut a = App::default();
        assert_eq!(a.history.len(), 10);
        a.update(Action::Focus(Focus::History));
        a.update(Action::Down);
        a.update(Action::Tab);
        assert_eq!(a.focus, Focus::Path);
        assert_eq!(a.editor.text, "~/Projects/notes");
        assert_eq!(a.screen, Screen::Dashboard);
        for _ in 0..12 {
            a.update(Action::Enter);
            a.update(Action::Escape);
        }
        assert_eq!(a.history.len(), 10);
        assert!(
            a.history
                .iter()
                .all(|e| e.path == "/home/demo/Projects/notes")
        );
        assert!(a.history.windows(2).all(|w| w[0].id > w[1].id));
        a.update(Action::Reset);
        a.update(Action::ToggleCopilot);
        assert!(!a.visible_tools().contains(&Tool::Copilot));
        a.update(Action::Focus(Focus::History));
        for _ in 0..5 {
            a.update(Action::Down);
        }
        a.update(Action::Enter);
        assert_eq!(a.screen, Screen::Dashboard);
        assert!(
            a.message
                .as_ref()
                .is_some_and(|m| m.contains("Copilot is unavailable"))
        );
        a.update(Action::Tab);
        assert_eq!(a.tool, Tool::Copilot, "no silent fallback on history copy");
        a.update(Action::Enter);
        assert_eq!(a.history.len(), 10);
        a.update(Action::Focus(Focus::Tools));
        a.update(Action::Right);
        assert_eq!(a.tool, Tool::Shell);
        a.update(Action::Enter);
        assert!(matches!(a.screen, Screen::Terminal(_)));
        a.update(Action::Escape);
        a.update(Action::Focus(Focus::History));
        a.update(Action::Delete);
        assert_eq!(a.history.len(), 9);
        a.update(Action::ClearHistory);
        assert_eq!(a.history.len(), 9);
        a.update(Action::ClearHistory);
        assert!(a.history.is_empty());
        a.update(Action::Up);
        a.update(Action::Down);
        a.update(Action::Tab);
        a.update(Action::Enter);
        assert_eq!(a.screen, Screen::Dashboard);
    }
    #[test]
    fn focus_navigation_completion_cycle_and_path_keys_are_distinct() {
        let mut a = App::default();
        query(&mut a, "notes");
        let options = a.suggestions.clone();
        a.update(Action::Tab);
        assert_eq!(a.editor.text, fixtures::short(a.dirs[options[0]].path));
        a.update(Action::Tab);
        assert_eq!(a.editor.text, fixtures::short(a.dirs[options[1]].path));
        a.update(Action::BackTab);
        assert_eq!(a.editor.text, "~/Projects/notes");
        assert_eq!(a.screen, Screen::Dashboard);
        a.update(Action::Down);
        assert_eq!(a.focus, Focus::Tools);
        a.update(Action::Right);
        assert_eq!(a.tool, Tool::Claude);
        a.update(Action::Down);
        assert_eq!(a.focus, Focus::History);
        let text = a.editor.text.clone();
        a.update(Action::Text("q".into()));
        assert_eq!(a.editor.text, text);
        a.update(Action::Focus(Focus::Path));
        a.update(Action::Home);
        a.update(Action::Delete);
        a.update(Action::Text("~".into()));
        a.update(Action::End);
        a.update(Action::Left);
        a.update(Action::Right);
        a.update(Action::Backspace);
        a.update(Action::Text("sq".into()));
        assert_eq!(a.editor.text, "~/Projects/notesq");
        assert!(!a.quit);
    }
    #[test]
    fn completion_accepts_only_then_validated_enter_simulates_launch() {
        let mut a = App::default();
        assert_eq!(a.focus, Focus::Path);
        assert_eq!(a.tool, Tool::Shell);
        query(&mut a, "nts");
        a.update(Action::Enter);
        assert_eq!(a.screen, Screen::Dashboard);
        assert!(a.message.as_ref().is_some_and(|s| s.contains("not found")));
        a.update(Action::Down);
        assert!(a.highlighted.is_some());
        a.update(Action::Enter);
        assert_eq!(a.editor.text, "~/Projects/notes");
        assert_eq!(a.screen, Screen::Dashboard);
        assert!(a.suggestions.is_empty());
        a.update(Action::Enter);
        assert!(matches!(a.screen, Screen::Terminal(_)));
        assert_eq!(a.history[0].path, "/home/demo/Projects/notes");
        let count = a.history.len();
        a.update(Action::Enter);
        assert_eq!(
            a.history.len(),
            count,
            "duplicate Enter ignored in terminal placeholder"
        );
    }
}

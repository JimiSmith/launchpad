use crate::{editor::Editor, fixtures, search::Directory};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMutation {
    Clear,
    Remove(u64),
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
    demo: bool,
    index: Option<crate::search::HomeIndex>,
    remote: Option<crate::remote::Remote>,
    pub search_status: String,
    /// Explicit host-adapter opt-in; native and demo remain simulations.
    pub host_launch: bool,
    host_request: Option<Launch>,
    history_request: Option<HistoryMutation>,
    host_launch_pending: bool,
    show_suggestions: bool,
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
        editor.set("~");
        let dirs = Vec::new();
        let suggestions = Vec::new();
        Self {
            editor,
            dirs,
            suggestions,
            highlighted: None,
            focus: Focus::Path,
            tool: Tool::Shell,
            history: Vec::new(),
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
            demo: false,
            index: None,
            remote: None,
            search_status: "Waiting for HOME access.".into(),
            host_launch: false,
            host_request: None,
            history_request: None,
            host_launch_pending: false,
            show_suggestions: true,
        }
    }
}
impl App {
    pub fn take_history_mutation(&mut self) -> Option<HistoryMutation> {
        self.history_request.take()
    }
    pub fn take_host_launch(&mut self) -> Option<Launch> {
        self.host_request.take()
    }
    pub fn host_launch_rejected(&mut self) {
        self.host_launch_pending = false;
        self.message =
            Some("Zellij did not accept the launch. Check permissions and try again.".into());
    }
    pub fn from_remote(home: std::path::PathBuf) -> Self {
        let mut app = Self::from_home(home, "/host".into());
        app.remote = Some(crate::remote::Remote {
            dirty: true,
            ..Default::default()
        });
        app
    }
    pub fn remote_refresh(&self) -> u64 {
        self.remote.as_ref().map_or(0, |r| r.refresh)
    }
    pub fn remote_progress(&mut self, revision: u64, status: String) {
        self.search_status = status;
        if let Some(remote) = &mut self.remote
            && revision > remote.revision
        {
            remote.dirty = true;
            remote.revision = revision;
        }
    }
    pub fn accept_remote_revision(&self, revision: u64) -> bool {
        self.remote
            .as_ref()
            .is_some_and(|r| !r.failed && revision >= r.revision)
    }
    pub fn take_remote_request(&mut self) -> Option<crate::remote::RemoteRequest> {
        let remote = self.remote.as_mut()?;
        if remote.failed {
            return None;
        }
        if remote.outbound.is_some() {
            return remote.outbound.take();
        }
        if !remote.dirty
            || self.help
            || !self.show_suggestions
            || self.cycle.is_some()
            || self.screen != Screen::Dashboard
            || self.quit
        {
            return None;
        }
        remote.dirty = false;
        Some(crate::remote::RemoteRequest::Query {
            generation: remote.generation,
            text: self.editor.text.clone(),
        })
    }
    fn request_validation(&mut self, raw: String, kind: crate::remote::Validation) {
        let remote = self.remote.as_mut().unwrap();
        if remote.failed {
            self.message = Some(self.search_status.clone());
            return;
        }
        remote.validation = Some(kind);
        remote.outbound = Some(crate::remote::RemoteRequest::Validate {
            generation: remote.generation,
            raw,
        });
        self.show_suggestions = false;
        self.suggestions.clear();
        self.highlighted = None;
        self.message = Some("Validating directory…".into());
    }
    pub fn remote_failed(&mut self, error: String) {
        if let Some(remote) = &mut self.remote {
            remote.failed = true;
            remote.generation += 1;
            remote.validation = None;
            remote.outbound = None;
        }
        self.search_status = error;
        self.message = None;
        self.suggestions.clear();
        self.highlighted = None;
        self.cycle = None;
    }
    pub fn finish_remote_validation(
        &mut self,
        generation: u64,
        result: Result<String, String>,
    ) -> bool {
        let Some(remote) = &mut self.remote else {
            return false;
        };
        if remote.failed
            || remote.generation != generation
            || self.screen != Screen::Dashboard
            || self.quit
        {
            return false;
        }
        let Some(kind) = remote.validation.take() else {
            return false;
        };
        match result {
            Err(error) => {
                self.message = Some(error);
                self.cycle = None;
            }
            Ok(path) => match kind {
                crate::remote::Validation::Accept => {
                    self.editor.set(&self.path_label(&path));
                    self.message = None;
                    self.touched = true;
                }
                crate::remote::Validation::Launch(tool) => self.finish_launch(path, tool),
            },
        }
        true
    }
    pub fn apply_remote_results(&mut self, generation: u64, paths: Vec<String>) -> bool {
        if self
            .remote
            .as_ref()
            .is_none_or(|r| r.generation != generation)
            || !self.show_suggestions
            || self.cycle.is_some()
            || self.screen != Screen::Dashboard
            || self.quit
        {
            return false;
        }
        let selected = self
            .highlighted
            .and_then(|i| self.suggestions.get(i))
            .and_then(|&i| self.dirs.get(i))
            .map(|d| d.path.clone());
        self.dirs = paths
            .into_iter()
            .take(100)
            .map(|path| Directory {
                path,
                note: "directory",
                error: None,
            })
            .collect();
        self.suggestions = (0..self.dirs.len()).collect();
        self.highlighted = selected.and_then(|path| self.dirs.iter().position(|d| d.path == path));
        true
    }
    pub fn from_home(home: std::path::PathBuf, root: std::path::PathBuf) -> Self {
        let mut app = Self::default();
        match crate::search::HomeIndex::new(home, root) {
            Ok(index) => {
                app.search_status = index.status();
                app.index = Some(index);
            }
            Err(error) => app.search_status = error,
        }
        app
    }
    pub fn is_demo(&self) -> bool {
        self.demo
    }
    pub fn is_indexing(&self) -> bool {
        self.remote.is_none() && self.index.as_ref().is_some_and(|i| i.is_scanning())
    }
    pub fn index_tick(&mut self) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let Some(index) = self.index.as_mut() else {
            return false;
        };
        if !index.is_scanning() {
            return false;
        }
        index.step(128);
        self.dirs.extend_from_slice(&index.dirs[self.dirs.len()..]);
        self.search_status = index.status();
        if self.show_suggestions && self.cycle.is_none() {
            let selected = self
                .highlighted
                .and_then(|i| self.suggestions.get(i))
                .copied();
            self.suggestions = self.matches();
            self.highlighted =
                selected.and_then(|id| self.suggestions.iter().position(|&i| i == id));
        }
        true
    }
    fn matches(&self) -> Vec<usize> {
        if self.remote.is_some() {
            return Vec::new();
        }
        if self.demo {
            return fixtures::matches(&self.editor.text, &self.dirs);
        }
        let Some(index) = &self.index else {
            return Vec::new();
        };
        let home = index.home.to_str().expect("validated HOME");
        let mut results = crate::search::matches_in(&self.editor.text, &self.dirs, home, home);
        results.truncate(100);
        results
    }
    pub fn demo() -> Self {
        let mut app = Self {
            demo: true,
            dirs: fixtures::directories(),
            history: fixtures::history(),
            ..Self::default()
        };
        app.editor.set("~/Projects/");
        app.suggestions = fixtures::matches(&app.editor.text, &app.dirs);
        app
    }
    pub fn path_label(&self, path: &str) -> String {
        if self.demo {
            return fixtures::short(path);
        }
        self.index
            .as_ref()
            .and_then(|i| std::path::Path::new(path).strip_prefix(&i.home).ok())
            .map(|rest| {
                if rest.as_os_str().is_empty() {
                    "~".into()
                } else {
                    format!("~/{}", rest.display())
                }
            })
            .unwrap_or_else(|| path.into())
    }

    pub fn update(&mut self, action: Action) {
        if self.host_launch_pending {
            return;
        }
        // Never turn a pending completion into a launch of the old editor.
        // Likewise, repeated submit must not replace an in-flight launch request.
        if self.host_launch
            && matches!(action, Action::Enter | Action::LaunchForm)
            && self.remote.as_ref().is_some_and(|r| r.validation.is_some())
        {
            return;
        }
        // Cursor motion keeps the validated path/tool intact. Focus, tool and
        // history changes instead abandon the pending completion or launch.
        let changes_validation_intent = match &action {
            Action::Focus(focus) => *focus != self.focus,
            Action::PathCursor(_) => self.focus != Focus::Path,
            Action::Left | Action::Right => self.focus == Focus::Tools,
            Action::Up => self.focus != Focus::Path,
            Action::Down => self.focus != Focus::Path || self.suggestions.is_empty(),
            Action::Home | Action::End => self.focus == Focus::History,
            Action::SelectTool(_)
            | Action::SelectHistory(_)
            | Action::Scroll(ScrollTarget::History, _)
            | Action::ToggleCopilot
            | Action::ClearHistory => true,
            _ => false,
        };
        if let Some(remote) = &mut self.remote
            && ((remote.validation.is_some() && changes_validation_intent)
                || matches!(
                    action,
                    Action::Text(_)
                        | Action::Clear
                        | Action::Backspace
                        | Action::Delete
                        | Action::Enter
                        | Action::Tab
                        | Action::BackTab
                        | Action::AcceptSuggestion(_)
                        | Action::LaunchForm
                        | Action::Escape
                        | Action::Reset
                        | Action::Help
                        | Action::Quit
                ))
        {
            remote.generation += 1;
            remote.dirty = true;
            if remote.validation.take().is_some() {
                self.message = None;
                self.show_suggestions = true;
                self.highlighted = None;
                // Repeated Tab replaces validation but keeps its candidate cycle.
                if self.focus != Focus::Path || !matches!(action, Action::Tab | Action::BackTab) {
                    self.cycle = None;
                }
            }
            remote.outbound = None;
        }
        if action == Action::Quit {
            self.quit = true;
            return;
        }
        if action == Action::Reset {
            let compact = self.compact;
            let host_launch = self.host_launch;
            let remote = self.remote.take();
            let index = self.index.as_ref().map(|i| i.restart());
            let status = self.search_status.clone();
            *self = if self.demo {
                Self::demo()
            } else {
                Self::default()
            };
            if let Some(index) = index {
                self.search_status = index.status();
                self.index = Some(index);
            } else if !self.demo {
                self.search_status = status;
            }
            self.compact = compact;
            self.host_launch = host_launch;
            self.remote = remote;
            if let Some(remote) = &mut self.remote {
                remote.revision = 0;
                remote.failed = false;
                remote.refresh += 1;
            }
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
            } else if !self.suggestions.is_empty()
                || self.cycle.is_some()
                || (self.remote.is_some() && self.show_suggestions)
            {
                self.suggestions.clear();
                self.show_suggestions = false;
                self.highlighted = None;
                self.cycle = None;
            } else if self.message.is_some() {
                self.message = None;
            } else if !self.touched {
                if self.host_launch {
                    self.quit = true;
                } else {
                    self.screen = Screen::Closed;
                }
            } else {
                self.message = Some("Form kept. Ctrl+Q quits; F5 resets the form.".into());
            }
            return;
        }
        if action != Action::ClearHistory && self.confirm_clear {
            self.confirm_clear = false;
            self.message = None;
        }
        if action == Action::ToggleCopilot {
            if self.host_launch {
                return;
            }
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
                    if let Some(&index) = self.highlighted.and_then(|i| self.suggestions.get(i)) {
                        self.accept(index);
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
                        self.editor.set(&self.path_label(&e.path));
                        self.tool = e.tool;
                        self.focus = Focus::Path;
                        self.touched = true;
                        self.suggestions.clear();
                        self.show_suggestions = false;
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
                    if let Some(row) = self.history.get(self.recent) {
                        if self.host_launch {
                            self.history_request = Some(HistoryMutation::Remove(row.id));
                        } else {
                            self.history.remove(self.recent);
                        }
                    }
                    self.recent = self.recent.min(self.history.len().saturating_sub(1));
                }
                Action::ClearHistory => {
                    if self.confirm_clear {
                        if self.host_launch {
                            self.history_request = Some(HistoryMutation::Clear);
                        } else {
                            self.history.clear();
                            self.recent = 0;
                        }
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
        self.host_launch || tool != Tool::Copilot || self.copilot_available
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
        self.show_suggestions = true;
        // Async replies replace the previous results atomically. Clearing here
        // would render an empty list between every keystroke and its reply.
        if self.remote.is_none() {
            self.suggestions = self.matches();
        }
    }
    fn accept(&mut self, index: usize) {
        if self.remote.is_some() {
            self.request_validation(
                self.dirs[index].path.clone(),
                crate::remote::Validation::Accept,
            );
            return;
        }
        if let Some(source) = &self.index
            && let Err(error) = source.validate(&self.dirs[index].path)
        {
            self.message = Some(error);
            self.suggestions.retain(|&i| i != index);
            self.highlighted = None;
            return;
        }
        self.editor.set(&self.path_label(&self.dirs[index].path));
        self.show_suggestions = false;
        self.suggestions.clear();
        self.highlighted = None;
        self.message = None;
        self.touched = true;
    }
    fn launch(&mut self, raw: String, tool: Tool) {
        if self.remote.is_some() {
            self.request_validation(raw, crate::remote::Validation::Launch(tool));
            return;
        }
        let path = if self.demo {
            let path = fixtures::normalize(&raw);
            let Some(dir) = self
                .dirs
                .iter()
                .find(|d| Some(d.path.as_str()) == path.as_deref())
            else {
                self.message = Some(
                    "Directory not found. Choose a suggestion or enter a fixture path.".into(),
                );
                return;
            };
            if let Some(error) = dir.error {
                self.message = Some(error.into());
                return;
            }
            dir.path.clone()
        } else if let Some(index) = &self.index {
            match index.validate(&raw) {
                Ok(path) => path,
                Err(error) => {
                    self.message = Some(error);
                    return;
                }
            }
        } else {
            self.message = Some(self.search_status.clone());
            return;
        };
        self.finish_launch(path, tool);
    }
    fn finish_launch(&mut self, path: String, tool: Tool) {
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
            path,
            tool,
            age: "Just now".into(),
        };
        if self.host_launch {
            self.host_request = Some(event);
            self.host_launch_pending = true;
            self.message = Some("Replacing this pane…".into());
            return;
        }
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
    fn remote_index_keeps_only_returned_rows_without_ui_scanning() {
        let mut a = App::from_remote("/home/example".into());
        assert!(!a.is_indexing());
        assert!(!a.index_tick());
        a.remote_progress(1, "HOME indexed".into());
        assert!(a.apply_remote_results(0, vec!["/home/example/research/notes".into()]));
        assert_eq!(a.suggestions, vec![0]);
        assert_eq!(a.path_label(&a.dirs[0].path), "~/research/notes");
        a.update(Action::Escape);
        a.remote_progress(2, "HOME indexed".into());
        assert!(a.suggestions.is_empty());
    }
    #[test]
    fn normal_startup_never_contains_demo_data() {
        let app = App::default();
        assert!(
            app.history.is_empty(),
            "real startup must not invent launch history"
        );
        assert!(app.dirs.is_empty());
        assert_eq!(app.editor.text, "~");
    }
    #[test]
    fn hidden_controls_never_launch_and_escape_help_quit_are_safe() {
        let mut a = App {
            compact: true,
            ..App::demo()
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
        let mut a = App::demo();
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
        let mut a = App::demo();
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
        let mut a = App::demo();
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
        let mut a = App::demo();
        query(&mut a, "notes");
        let options = a.suggestions.clone();
        a.update(Action::Tab);
        assert_eq!(a.editor.text, fixtures::short(&a.dirs[options[0]].path));
        a.update(Action::Tab);
        assert_eq!(a.editor.text, fixtures::short(&a.dirs[options[1]].path));
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
        let mut a = App::demo();
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

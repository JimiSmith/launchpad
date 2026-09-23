use crate::{editor::Editor, search::Directory};

pub use crate::commands::Tool;
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
pub enum Action {
    Scroll(ScrollTarget, bool),
    PathCursor(usize),
    AcceptSuggestion(usize),
    SelectTool(Tool),
    SelectHistory(u64),
    LaunchForm,
    /// A configured command shortcut: launch the typed path, or the selected
    /// recent row's directory, with this tool.
    Shortcut(Tool),
    Text(String),
    Left,
    Right,
    SegmentLeft,
    SegmentRight,
    DeleteSegmentLeft,
    DeleteSegmentRight,
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
    pub commands: crate::commands::Commands,
    pub theme: crate::theme::Theme,
    pub theme_errors: Vec<String>,
    pub ignore_errors: Vec<String>,
    pub history: Vec<Launch>,
    pub recent: usize,
    pub message: Option<String>,
    pub help: bool,
    pub help_scroll: usize,
    /// Rendered-row limit from the last help frame, not the logical line count.
    pub(crate) help_scroll_max: std::cell::Cell<usize>,
    pub quit: bool,
    pub compact: bool,
    pub touched: bool,
    pub confirm_clear: bool,
    next_id: u64,
    index: Option<crate::search::HomeIndex>,
    remote: Option<crate::remote::Remote>,
    pub search_status: String,
    /// `simulate_launch`: validate and record in memory, never spawn a process
    /// and never touch the shared history store. Development harness only.
    pub simulate_launch: bool,
    launch_request: Option<Launch>,
    history_request: Option<HistoryMutation>,
    launch_pending: bool,
    show_suggestions: bool,
    initial_cwd: Option<String>,
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
        Self {
            editor,
            dirs: Vec::new(),
            suggestions: Vec::new(),
            highlighted: None,
            focus: Focus::Path,
            tool: Tool::Shell,
            commands: Default::default(),
            theme: Default::default(),
            theme_errors: Vec::new(),
            ignore_errors: Vec::new(),
            history: Vec::new(),
            recent: 0,
            message: None,
            help: false,
            help_scroll: 0,
            help_scroll_max: std::cell::Cell::new(usize::MAX),
            quit: false,
            compact: false,
            touched: false,
            confirm_clear: false,
            next_id: 0,
            index: None,
            remote: None,
            search_status: "Waiting for HOME access.".into(),
            simulate_launch: false,
            launch_request: None,
            history_request: None,
            launch_pending: false,
            show_suggestions: true,
            initial_cwd: None,
        }
    }
}
impl App {
    /// Host-supplied identity, not typed input: never apply the insertion cap.
    pub fn set_initial_cwd(&mut self, cwd: String) {
        self.editor.set(&self.path_label(&cwd));
        self.initial_cwd = Some(cwd);
        self.show_suggestions = false;
        self.suggestions.clear();
        self.highlighted = None;
        self.tool = Tool::Shell;
    }
    pub fn configure(&mut self, configuration: &std::collections::BTreeMap<String, String>) {
        self.commands = crate::commands::Commands::parse(configuration);
        (self.theme, self.theme_errors) = crate::theme::Theme::parse(configuration);
    }
    pub fn config_errors(&self) -> impl Iterator<Item = &String> {
        self.commands
            .errors
            .iter()
            .chain(&self.theme_errors)
            .chain(&self.ignore_errors)
    }
    pub fn tool_label(&self, tool: Tool) -> String {
        self.commands
            .get(tool)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| tool.as_str().into())
    }
    pub fn take_history_mutation(&mut self) -> Option<HistoryMutation> {
        self.history_request.take()
    }
    pub fn take_launch(&mut self) -> Option<Launch> {
        self.launch_request.take()
    }
    pub fn launch_rejected(&mut self) {
        self.launch_pending = false;
        self.message =
            Some("Zellij did not accept the launch. Check the command and try again.".into());
    }
    pub fn from_remote(home: std::path::PathBuf) -> Self {
        // The UI never traverses this mapping; filesystem IO belongs to its
        // worker. Keep host path labels independent of the test platform.
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
        self.take_remote_request_with_search(true)
    }
    /// Defer search while the host's edit debounce is active. Explicit directory
    /// validation always bypasses the debounce.
    pub fn take_remote_request_with_search(
        &mut self,
        allow_search: bool,
    ) -> Option<crate::remote::RemoteRequest> {
        let remote = self.remote.as_mut()?;
        if remote.failed {
            return None;
        }
        if remote.outbound.is_some() {
            return remote.outbound.take();
        }
        if !allow_search || !remote.dirty || self.help || !self.show_suggestions || self.quit {
            return None;
        }
        remote.dirty = false;
        Some(crate::remote::RemoteRequest::Query {
            generation: remote.generation,
            text: self.editor.text.clone(),
            recent: self.recent_paths(),
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
        self.search_status = error.clone();
        self.message = Some(error);
        self.suggestions.clear();
        self.highlighted = None;
    }
    pub fn finish_remote_validation(
        &mut self,
        generation: u64,
        result: Result<String, String>,
    ) -> bool {
        let Some(remote) = &mut self.remote else {
            return false;
        };
        if remote.failed || remote.generation != generation || self.quit {
            return false;
        }
        let Some(kind) = remote.validation.take() else {
            return false;
        };
        match result {
            Err(error) => {
                self.message = Some(error);
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
        // `highlighted` indexes `suggestions`, not `dirs`. Map through both so
        // a future filtered suggestion list cannot silently shift the highlight.
        self.highlighted = selected
            .and_then(|path| self.dirs.iter().position(|d| d.path == path))
            .and_then(|dir| self.suggestions.iter().position(|&i| i == dir));
        true
    }
    pub fn from_home(home: std::path::PathBuf, root: std::path::PathBuf) -> Self {
        let mut app = Self::default();
        if let Some(path) = home.to_str().and_then(crate::host_path::HostPath::parse) {
            app.editor.windows_paths = path.is_windows();
        }
        match crate::search::HomeIndex::new(home, root) {
            Ok(index) => {
                app.search_status = index.status();
                app.index = Some(index);
            }
            Err(error) => app.search_status = error,
        }
        app
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
        if self.show_suggestions {
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
        let Some(index) = &self.index else {
            return Vec::new();
        };
        let home = index.home.to_str().expect("validated HOME");
        let mut results =
            crate::search::matches_in(&self.editor.text, &self.dirs, home, &self.recent_paths());
        results.truncate(100);
        results
    }
    /// Launched directories, most recent first, for search tie-breaks.
    fn recent_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = Vec::new();
        for event in &self.history {
            if !paths.contains(&event.path) {
                paths.push(event.path.clone());
            }
        }
        paths
    }
    pub fn path_label(&self, path: &str) -> String {
        self.index
            .as_ref()
            .and_then(|i| crate::host_path::label(path, i.home.to_str()?))
            .unwrap_or_else(|| path.into())
    }

    pub fn update(&mut self, action: Action) {
        if self.launch_pending {
            return;
        }
        if matches!(
            action,
            Action::SegmentLeft
                | Action::SegmentRight
                | Action::DeleteSegmentLeft
                | Action::DeleteSegmentRight
        ) && (self.focus != Focus::Path || self.help || self.compact)
        {
            return;
        }
        // Never turn a pending completion into a launch of the old editor.
        // Likewise, repeated submit must not replace an in-flight launch request.
        if matches!(
            action,
            Action::Enter | Action::LaunchForm | Action::Shortcut(_)
        ) && self.remote.as_ref().is_some_and(|r| r.validation.is_some())
        {
            return;
        }
        // Cursor motion keeps the validated path/tool intact. Focus, tool and
        // history changes instead abandon the pending selection or launch.
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
            | Action::ClearHistory
            | Action::Tab
            | Action::BackTab => true,
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
                        | Action::DeleteSegmentLeft
                        | Action::DeleteSegmentRight
                        | Action::Enter
                        | Action::AcceptSuggestion(_)
                        | Action::LaunchForm
                        | Action::Shortcut(_)
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
            }
            remote.outbound = None;
        }
        if action == Action::Quit {
            self.quit = true;
            return;
        }
        if action == Action::Reset {
            let commands = self.commands.clone();
            let theme = self.theme;
            let theme_errors = std::mem::take(&mut self.theme_errors);
            let ignore_errors = std::mem::take(&mut self.ignore_errors);
            let initial_cwd = self.initial_cwd.clone();
            let compact = self.compact;
            let simulate_launch = self.simulate_launch;
            let remote = self.remote.take();
            let index = self.index.as_ref().map(|i| i.restart());
            let status = self.search_status.clone();
            let windows_paths = self.editor.windows_paths;
            *self = Self::default();
            self.editor.windows_paths = windows_paths;
            if let Some(index) = index {
                self.search_status = index.status();
                self.index = Some(index);
            } else {
                self.search_status = status;
            }
            self.commands = commands;
            self.theme = theme;
            self.theme_errors = theme_errors;
            self.ignore_errors = ignore_errors;
            if let Some(cwd) = initial_cwd {
                self.set_initial_cwd(cwd);
            }
            self.compact = compact;
            self.simulate_launch = simulate_launch;
            self.remote = remote;
            if let Some(remote) = &mut self.remote {
                remote.revision = 0;
                remote.failed = false;
                remote.refresh += 1;
                // The worker learns about refresh epochs through requests. A
                // quiet initial directory still needs to trigger the rebuild.
                if !self.show_suggestions {
                    remote.outbound = Some(crate::remote::RemoteRequest::Query {
                        generation: remote.generation,
                        text: self.editor.text.clone(),
                        // Reset clears history until the host reloads it.
                        recent: Vec::new(),
                    });
                }
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
                Action::Up => {
                    self.help_scroll = self
                        .help_scroll
                        .min(self.help_scroll_max.get())
                        .saturating_sub(1)
                }
                Action::Down => {
                    self.help_scroll = self
                        .help_scroll
                        .saturating_add(1)
                        .min(self.help_scroll_max.get())
                }
                Action::Home => self.help_scroll = 0,
                // Resolve End against the next actual frame, including a resize.
                Action::End => self.help_scroll = usize::MAX,
                _ => {}
            }
            return;
        }
        if action == Action::Escape {
            if self.confirm_clear {
                self.confirm_clear = false;
                self.message = None;
            } else if !self.suggestions.is_empty()
                || (self.remote.is_some() && self.show_suggestions)
            {
                self.suggestions.clear();
                self.show_suggestions = false;
                self.highlighted = None;
            } else if self.message.is_some() {
                self.message = None;
            } else if !self.touched {
                self.quit = true;
            } else {
                self.message = Some("Form kept. Ctrl+Q quits; F5 resets the form.".into());
            }
            return;
        }
        if action != Action::ClearHistory && self.confirm_clear {
            self.confirm_clear = false;
            self.message = None;
        }
        if let Action::Focus(focus) = action {
            self.focus = focus;
            self.sync_recent();
            return;
        }
        if matches!(action, Action::Tab | Action::BackTab) {
            self.focus = match (self.focus, action) {
                (Focus::Path, Action::Tab) => Focus::Tools,
                (Focus::Tools, Action::Tab) => Focus::History,
                (Focus::History, Action::Tab) => Focus::Path,
                (Focus::Path, Action::BackTab) => Focus::History,
                (Focus::Tools, Action::BackTab) => Focus::Path,
                (Focus::History, Action::BackTab) => Focus::Tools,
                _ => unreachable!("only Tab actions reach this branch"),
            };
            self.sync_recent();
            return;
        }
        if action == Action::LaunchForm {
            if self.focus == Focus::History
                && let Some(event) = self.history.get(self.recent).cloned()
            {
                self.launch(event.path, event.tool);
                return;
            }
            self.launch(self.editor.text.clone(), self.tool);
            return;
        }
        if let Action::Shortcut(tool) = action {
            if !self.available(tool) {
                return;
            }
            self.tool = tool;
            self.touched = true;
            // Suggestions stay unaccepted: the shortcut uses the typed text.
            let path = (self.focus == Focus::History)
                .then(|| self.history.get(self.recent).map(|e| e.path.clone()))
                .flatten()
                .unwrap_or_else(|| self.editor.text.clone());
            self.launch(path, tool);
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
                self.sync_recent();
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
                Action::DeleteSegmentLeft => {
                    self.editor.delete_segment_left();
                    self.edited();
                }
                Action::DeleteSegmentRight => {
                    self.editor.delete_segment_right();
                    self.edited();
                }
                Action::SegmentLeft => self.editor.segment_left(),
                Action::SegmentRight => self.editor.segment_right(),
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
                Action::Down => {
                    self.focus = Focus::History;
                    self.sync_recent();
                }
                Action::Enter => self.launch(self.editor.text.clone(), self.tool),
                _ => {}
            },
            Focus::History => match action {
                Action::Up => {
                    self.recent = self.recent.saturating_sub(1);
                    self.sync_recent();
                }
                Action::Down => {
                    self.recent = (self.recent + 1).min(self.history.len().saturating_sub(1));
                    self.sync_recent();
                }
                Action::Home => {
                    self.recent = 0;
                    self.sync_recent();
                }
                Action::End => {
                    self.recent = self.history.len().saturating_sub(1);
                    self.sync_recent();
                }
                Action::Enter => self.update(Action::LaunchForm),
                Action::Delete => {
                    if let Some(row) = self.history.get(self.recent) {
                        if self.simulate_launch {
                            self.history.remove(self.recent);
                        } else {
                            self.history_request = Some(HistoryMutation::Remove(row.id));
                        }
                    }
                    self.recent = self.recent.min(self.history.len().saturating_sub(1));
                    self.sync_recent();
                }
                Action::ClearHistory => {
                    if self.confirm_clear {
                        if self.simulate_launch {
                            self.history.clear();
                            self.recent = 0;
                        } else {
                            self.history_request = Some(HistoryMutation::Clear);
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
    // A recent selection fills the form, so mouse Launch and Enter agree.
    // Keep removed command IDs intact: validation must never substitute Shell.
    fn sync_recent(&mut self) {
        if self.focus != Focus::History {
            return;
        }
        if let Some(event) = self.history.get(self.recent) {
            self.editor.set(&self.path_label(&event.path));
            self.tool = event.tool;
            self.show_suggestions = false;
            self.suggestions.clear();
            self.highlighted = None;
            self.touched = true;
            self.message = None;
        }
    }

    pub(crate) fn empty_search(&self) -> bool {
        self.focus == Focus::Path
            && self.touched
            && self.show_suggestions
            && self.suggestions.is_empty()
            && self.message.is_none()
    }

    /// Replace host history and keep the selected row consistent with the form.
    pub fn replace_history(&mut self, history: Vec<Launch>) {
        let message = self.message.take();
        self.history = history;
        self.recent = self.recent.min(self.history.len().saturating_sub(1));
        self.sync_recent();
        self.message = message;
    }

    pub fn visible_tools(&self) -> Vec<Tool> {
        self.commands.entries.iter().map(|c| c.id).collect()
    }
    pub fn available(&self, tool: Tool) -> bool {
        self.commands.get(tool).is_some()
    }
    fn edited(&mut self) {
        self.touched = true;
        self.message = None;
        self.highlighted = None;
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
        let Some(index) = &self.index else {
            self.message = Some(self.search_status.clone());
            return;
        };
        match index.validate(&raw) {
            Ok(path) => self.finish_launch(path, tool),
            Err(error) => self.message = Some(error),
        }
    }
    fn finish_launch(&mut self, path: String, tool: Tool) {
        if !self.available(tool) {
            self.message = Some(format!(
                "{} is unavailable. Choose a tool with Ctrl+T.",
                self.tool_label(tool)
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
        if self.simulate_launch {
            self.message = Some(format!(
                "Launch suppressed (simulate_launch): {} · {}",
                self.tool_label(tool),
                self.path_label(&event.path)
            ));
            self.history
                .retain(|row| row.tool != event.tool || row.path != event.path);
            self.history.insert(0, event);
            self.history.truncate(10);
            self.recent = 0;
            return;
        }
        self.launch_request = Some(event);
        self.launch_pending = true;
        self.message = Some("Replacing this pane…".into());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::RemoteRequest;

    const HOME: &str = "/home/example";

    fn tool(id: &str) -> Tool {
        Tool::new(id).expect("valid command ID")
    }
    /// A worker-backed App, as the native adapter builds one. No fixtures.
    fn app() -> App {
        let mut app = App::from_remote(HOME.into());
        app.configure(&std::collections::BTreeMap::from([
            ("commands".into(), "claude,codex".into()),
            ("command_claude".into(), "claude".into()),
            ("command_codex".into(), "codex".into()),
            ("label_codex".into(), "Codex CLI".into()),
        ]));
        app
    }
    /// Answer the outstanding worker query with these absolute paths.
    fn results(app: &mut App, paths: &[&str]) -> u64 {
        let Some(RemoteRequest::Query { generation, .. }) = app.take_remote_request() else {
            panic!("expected a pending query");
        };
        assert!(app.apply_remote_results(generation, paths.iter().map(|p| (*p).into()).collect()));
        generation
    }
    fn validate(app: &mut App, result: Result<String, String>) {
        let Some(RemoteRequest::Validate { generation, .. }) = app.take_remote_request() else {
            panic!("expected a pending validation");
        };
        assert!(app.finish_remote_validation(generation, result));
    }
    fn query(app: &mut App, text: &str) {
        app.update(Action::Clear);
        app.update(Action::Text(text.into()));
    }

    #[test]
    fn normal_startup_never_invents_directories_or_history() {
        let app = App::default();
        assert!(app.history.is_empty());
        assert!(app.dirs.is_empty());
        assert_eq!(app.editor.text, "~");
        assert_eq!(app.visible_tools(), vec![Tool::Shell]);
        assert!(!app.simulate_launch, "real launching is the default");
    }

    #[test]
    fn remote_index_keeps_only_returned_rows_without_ui_scanning() {
        let mut a = app();
        assert!(!a.is_indexing());
        assert!(!a.index_tick());
        a.remote_progress(1, "HOME indexed".into());
        query(&mut a, "notes");
        results(&mut a, &["/home/example/research/notes"]);
        assert_eq!(a.suggestions, vec![0]);
        assert_eq!(a.path_label(&a.dirs[0].path), "~/research/notes");
        a.update(Action::Escape);
        a.remote_progress(2, "HOME indexed".into());
        assert!(a.suggestions.is_empty());
    }

    #[test]
    fn highlight_follows_the_selected_path_across_a_later_reply() {
        let mut a = app();
        query(&mut a, "notes");
        let generation = results(&mut a, &["/home/example/a/notes", "/home/example/b/notes"]);
        a.update(Action::Down);
        a.update(Action::Down);
        assert_eq!(a.highlighted, Some(1));
        // A later reply for the same query reorders the rows as the scan grows;
        // the highlight follows the selected path, not its former row number.
        assert!(a.apply_remote_results(
            generation,
            vec![
                "/home/example/b/notes".into(),
                "/home/example/a/notes".into(),
            ],
        ));
        assert_eq!(a.highlighted, Some(0));
        assert_eq!(a.dirs[a.suggestions[0]].path, "/home/example/b/notes");
    }

    #[test]
    fn completion_accepts_only_then_validated_enter_requests_one_launch() {
        let mut a = app();
        assert_eq!(a.focus, Focus::Path);
        assert_eq!(a.tool, Tool::Shell);
        query(&mut a, "nts");
        results(&mut a, &["/home/example/Projects/notes"]);
        a.update(Action::Down);
        assert_eq!(a.highlighted, Some(0));
        a.update(Action::Enter);
        validate(&mut a, Ok("/home/example/Projects/notes".into()));
        assert_eq!(a.editor.text, "~/Projects/notes");
        assert!(a.suggestions.is_empty());
        assert!(a.take_launch().is_none(), "accepting never launches");

        a.update(Action::Enter);
        a.update(Action::Enter);
        validate(&mut a, Ok("/home/example/Projects/notes".into()));
        let launch = a.take_launch().expect("one launch request");
        assert_eq!(launch.path, "/home/example/Projects/notes");
        assert_eq!(launch.tool, Tool::Shell);
        assert!(a.history.is_empty(), "the store owns real history");
        a.update(Action::Enter);
        assert!(
            a.take_launch().is_none(),
            "the pane is already being replaced"
        );
    }

    #[test]
    fn failed_validation_preserves_the_form_and_launches_nothing() {
        let mut a = app();
        for (text, error) in [
            ("~/restricted", "Permission denied"),
            ("~/broken-link", "symlinks and files are not supported"),
            ("~/Projects/missing", "Directory unavailable"),
        ] {
            query(&mut a, text);
            a.update(Action::Enter);
            validate(&mut a, Err(error.into()));
            assert_eq!(a.editor.text, text);
            assert_eq!(a.message.as_deref(), Some(error));
            assert!(a.take_launch().is_none());
        }
    }

    #[test]
    fn literal_shell_looking_paths_reach_the_host_untouched() {
        let mut a = app();
        let literal = "/home/example/Projects/it's literal; $HOME";
        for id in ["shell", "claude", "codex"] {
            query(&mut a, "~/Projects/it's literal; $HOME");
            a.tool = tool(id);
            a.update(Action::Enter);
            validate(&mut a, Ok(literal.into()));
            let launch = a.take_launch().expect("launch request");
            assert_eq!(launch.path, literal);
            assert_eq!(launch.tool, tool(id));
            a.launch_rejected();
        }
    }

    #[test]
    fn simulated_launch_records_in_memory_and_never_reaches_the_host() {
        let mut a = app();
        a.simulate_launch = true;
        query(&mut a, "notes");
        a.update(Action::Enter);
        validate(&mut a, Ok("/home/example/notes".into()));
        assert!(a.take_launch().is_none(), "no host launch when simulating");
        assert_eq!(a.history.len(), 1);
        assert_eq!(a.history[0].path, "/home/example/notes");
        assert!(
            a.message
                .as_ref()
                .is_some_and(|m| m.contains("Launch suppressed"))
        );
        for i in 0..12 {
            query(&mut a, "x");
            a.update(Action::Enter);
            validate(&mut a, Ok(format!("/home/example/d{i}")));
        }
        assert_eq!(a.history.len(), 10, "in-memory history keeps the same cap");
        assert!(a.history.windows(2).all(|w| w[0].id > w[1].id));
    }

    #[test]
    fn simulated_history_deduplicates_tool_directory_pairs() {
        let mut a = app();
        a.simulate_launch = true;
        for id in ["shell", "codex", "shell"] {
            a.finish_launch("/home/example/notes".into(), tool(id));
        }
        assert_eq!(a.history.len(), 2);
        assert_eq!(a.history[0].tool, Tool::Shell);
        assert_eq!(a.history[0].id, 3);
        assert_eq!(a.history[1].tool, tool("codex"));
        assert_eq!(a.history[1].id, 2);
    }

    #[test]
    fn history_mutations_wait_for_the_store_unless_simulating() {
        let mut a = app();
        a.history = vec![
            Launch {
                id: 0,
                path: "/home/example/a".into(),
                tool: Tool::Shell,
                age: "1h ago".into(),
            },
            Launch {
                id: 1,
                path: "/home/example/b".into(),
                tool: tool("claude"),
                age: "2h ago".into(),
            },
        ];
        a.update(Action::Focus(Focus::History));
        a.update(Action::Down);
        a.update(Action::Delete);
        assert_eq!(a.history.len(), 2, "the store decides, not the UI");
        assert_eq!(a.take_history_mutation(), Some(HistoryMutation::Remove(1)));

        a.update(Action::ClearHistory);
        assert!(a.confirm_clear);
        assert!(a.take_history_mutation().is_none(), "one Ctrl+L only arms");
        a.update(Action::Escape);
        assert!(!a.confirm_clear);
        a.update(Action::ClearHistory);
        a.update(Action::ClearHistory);
        assert_eq!(a.history.len(), 2);
        assert_eq!(a.take_history_mutation(), Some(HistoryMutation::Clear));
    }

    #[test]
    fn shortcut_launches_the_typed_text_not_the_highlighted_suggestion() {
        let mut a = app();
        let claude = tool("claude");
        query(&mut a, "nts");
        results(&mut a, &["/home/example/Projects/notes"]);
        a.update(Action::Down);
        assert_eq!(a.highlighted, Some(0));
        a.update(Action::Shortcut(claude));
        let Some(RemoteRequest::Validate { raw, .. }) = a.remote.as_mut().unwrap().outbound.clone()
        else {
            panic!("expected a pending validation");
        };
        assert_eq!(raw, "nts");
        // A second shortcut or Enter never replaces the in-flight launch.
        a.update(Action::Shortcut(Tool::Shell));
        a.update(Action::Enter);
        validate(&mut a, Ok("/home/example/nts".into()));
        let launch = a.take_launch().expect("one launch request");
        assert_eq!(
            (launch.path.as_str(), launch.tool),
            ("/home/example/nts", claude)
        );
    }

    #[test]
    fn shortcut_on_a_recent_row_replaces_its_tool() {
        let mut a = app();
        a.history = vec![Launch {
            id: 0,
            path: "/home/example/Projects/notes".into(),
            tool: Tool::Shell,
            age: "12m ago".into(),
        }];
        a.update(Action::Focus(Focus::History));
        a.update(Action::Shortcut(tool("codex")));
        validate(&mut a, Err("gone".into()));
        assert_eq!(a.tool, tool("codex"), "a failed shortcut shows its tool");
        a.update(Action::Shortcut(tool("codex")));
        validate(&mut a, Ok("/home/example/Projects/notes".into()));
        let launch = a.take_launch().expect("one launch request");
        assert_eq!(
            (launch.path.as_str(), launch.tool),
            ("/home/example/Projects/notes", tool("codex"))
        );
    }

    #[test]
    fn shortcuts_are_ignored_under_help_and_for_unknown_tools() {
        let mut a = app();
        let validates = |a: &mut App| {
            matches!(
                a.take_remote_request(),
                Some(RemoteRequest::Validate { .. })
            )
        };
        a.update(Action::Help);
        a.update(Action::Shortcut(tool("claude")));
        assert!(!validates(&mut a));
        a.update(Action::Help);
        a.update(Action::Shortcut(tool("retired")));
        assert!(!validates(&mut a));
        assert_eq!(a.tool, Tool::Shell);
    }

    #[test]
    fn history_replay_never_substitutes_a_removed_command() {
        let mut a = app();
        let removed = tool("retired");
        a.history = vec![Launch {
            id: 0,
            path: "/home/example/Projects/notes".into(),
            tool: removed,
            age: "12m ago".into(),
        }];
        a.update(Action::Focus(Focus::History));
        a.update(Action::Enter);
        validate(&mut a, Ok("/home/example/Projects/notes".into()));
        assert!(a.take_launch().is_none(), "the command no longer exists");
        assert!(
            a.message
                .as_ref()
                .is_some_and(|m| m.contains("retired is unavailable"))
        );
        a.update(Action::Tab);
        assert_eq!(a.focus, Focus::Path);
        assert_eq!(a.editor.text, "~/Projects/notes");
        assert_eq!(
            a.tool, removed,
            "recent selection keeps an unavailable tool explicit"
        );
        a.update(Action::SelectTool(Tool::Shell));
        a.update(Action::Enter);
        validate(&mut a, Ok("/home/example/Projects/notes".into()));
        assert_eq!(a.take_launch().map(|l| l.tool), Some(Tool::Shell));
    }

    #[test]
    fn hidden_controls_and_help_never_launch_and_escape_closes_once() {
        let mut a = App {
            compact: true,
            ..app()
        };
        a.update(Action::Enter);
        assert!(a.take_launch().is_none());
        a.update(Action::Reset);
        assert!(a.compact, "reset must not bypass the compact guard");
        a.compact = false;
        a.update(Action::Help);
        assert!(a.help);
        a.update(Action::Enter);
        assert!(a.take_launch().is_none());
        a.update(Action::Escape);
        assert!(!a.help);
        a.update(Action::Escape);
        assert!(a.suggestions.is_empty());
        assert!(!a.quit);
        a.update(Action::Escape);
        assert!(a.quit, "an untouched dashboard closes its own pane");
    }

    #[test]
    fn tab_cycles_sections_and_path_keys_are_distinct() {
        let mut a = app();
        query(&mut a, "notes");
        results(
            &mut a,
            &["/home/example/Projects/notes", "/home/example/notes"],
        );
        a.update(Action::Tab);
        assert_eq!(a.focus, Focus::Tools);
        a.update(Action::Tab);
        assert_eq!(a.focus, Focus::History);
        a.update(Action::BackTab);
        assert_eq!(a.focus, Focus::Tools);
        a.update(Action::BackTab);
        assert_eq!(a.focus, Focus::Path);
        assert_eq!(a.editor.text, "notes", "Tab does not alter the path");

        a.update(Action::Down);
        a.update(Action::Down);
        assert_eq!(a.focus, Focus::Path, "Down cycles suggestions, not focus");
        a.update(Action::Escape);
        a.update(Action::Down);
        assert_eq!(a.focus, Focus::Tools);
        a.update(Action::Right);
        assert_eq!(a.tool, tool("claude"));
        a.update(Action::Down);
        assert_eq!(a.focus, Focus::History);
        let text = a.editor.text.clone();
        a.update(Action::Text("q".into()));
        assert_eq!(a.editor.text, text, "typing is path-focus only");
    }

    #[test]
    fn reset_restores_the_invoking_cwd_and_shell_but_keeps_configuration() {
        let mut a = app();
        a.set_initial_cwd("/home/example/Projects".into());
        assert_eq!(a.editor.text, "~/Projects");
        query(&mut a, "elsewhere");
        a.update(Action::Focus(Focus::Tools));
        a.update(Action::Right);
        assert_eq!(a.tool, tool("claude"));
        let refresh = a.remote_refresh();

        a.update(Action::Reset);
        assert_eq!(a.editor.text, "~/Projects");
        assert_eq!(a.tool, Tool::Shell);
        assert_eq!(a.focus, Focus::Path);
        assert!(!a.touched);
        assert_eq!(a.tool_label(tool("codex")), "Codex CLI", "config survives");
        assert_eq!(a.remote_refresh(), refresh + 1, "reset rebuilds the index");
    }
}

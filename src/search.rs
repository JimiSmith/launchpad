//! Home-only cached directory index. No shell, subprocess, or render-time IO.
use crate::editor::MAX_INPUT_CHARS;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
mod ignore_rules;

pub fn normalize_in(raw: &str, home: &str, cwd: &str) -> Option<String> {
    if raw.is_empty()
        || raw.chars().any(char::is_control)
        || (raw.starts_with('~') && raw != "~" && !raw.starts_with("~/"))
    {
        return None;
    }
    let path = if raw == "~" {
        home.to_owned()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else if raw.starts_with('/') {
        raw.to_owned()
    } else {
        format!("{cwd}/{raw}")
    };
    let mut parts = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

/// Embedded Frizbee ranks basename and full path; path order breaks ties.
pub fn matches_in(raw: &str, dirs: &[Directory], home: &str, cwd: &str) -> Vec<usize> {
    use neo_frizbee::{Config, Matcher};
    if raw.chars().count() > MAX_INPUT_CHARS {
        return Vec::new();
    }
    let config = Config {
        max_typos: Some(0),
        ..Config::default()
    };
    let explicit = raw.starts_with('/')
        || raw.starts_with('~')
        || raw.starts_with("./")
        || raw.starts_with("../");
    let query = if explicit {
        normalize_in(raw, home, cwd).unwrap_or_else(|| raw.into())
    } else {
        raw.to_owned()
    };
    // HOME expansion can exceed the editor limit. Bound Frizbee's scratch
    // matrices too; never truncate a path into a different search/launch target.
    if query.chars().count() > MAX_INPUT_CHARS {
        return Vec::new();
    }
    let allow_hidden = raw
        .split('/')
        .any(|p| p.starts_with('.') && p != "." && p != "..");
    let candidates: Vec<_> = dirs
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            allow_hidden
                || !d
                    .path
                    .strip_prefix(home)
                    .unwrap_or(&d.path)
                    .split('/')
                    .any(|p| p.starts_with('.'))
        })
        .collect();
    let paths: Vec<_> = candidates.iter().map(|(_, d)| d.path.as_str()).collect();
    let names: Vec<_> = candidates
        .iter()
        .map(|(_, d)| d.path.rsplit('/').next().unwrap_or(&d.path))
        .collect();
    let mut scores = vec![None; candidates.len()];
    let mut matcher = Matcher::new(&query, &config);
    for m in matcher
        .match_list(&paths)
        .into_iter()
        .chain(matcher.match_list(&names))
    {
        let score = &mut scores[m.index as usize];
        *score = Some(score.unwrap_or(0).max(m.score));
    }
    let mut ranked: Vec<_> = scores
        .into_iter()
        .enumerate()
        .filter_map(|(i, score)| score.map(|s| (candidates[i].0, s)))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| dirs[a.0].path.cmp(&dirs[b.0].path))
    });
    ranked.into_iter().map(|(i, _)| i).collect()
}

#[derive(Debug, Clone)]
pub struct Directory {
    pub path: String,
    pub note: &'static str,
    pub error: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Conservative retained path/queue/allocation accounting, not RSS.
    pub max_bytes: usize,
    pub max_directories: usize,
    pub max_entries: usize,
    pub max_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 6 * 1024 * 1024,
            max_directories: 20_000,
            max_entries: 200_000,
            max_depth: 64,
        }
    }
}
pub struct HomeIndex {
    pub dirs: Vec<Directory>,
    pub home: PathBuf,
    root: PathBuf,
    pub limits: Limits,
    visited: usize,
    retained_bytes: usize,
    rule_bytes: Arc<AtomicUsize>,
    limited: bool,
    error: Option<String>,
    walker: Option<ignore::Walk>,
    started: bool,
}
impl std::fmt::Debug for HomeIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HomeIndex")
            .field("home", &self.home)
            .field("limits", &self.limits)
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}
impl HomeIndex {
    /// `home` is the host path; `root` is its filesystem mapping (/host in WASI).
    pub fn new(home: PathBuf, root: PathBuf) -> Result<Self, String> {
        if !home.is_absolute()
            || !root.is_absolute()
            || home.parent().is_none()
            || home
                .to_str()
                .is_none_or(|p| p.chars().any(char::is_control))
            || home
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("HOME must be an absolute directory.".into());
        }
        Ok(Self {
            dirs: Vec::new(),
            home,
            root,
            limits: Limits::default(),
            visited: 0,
            retained_bytes: 0,
            rule_bytes: Arc::new(AtomicUsize::new(0)),
            limited: false,
            error: None,
            walker: None,
            started: false,
        })
    }
    fn relative(&self, raw: &str) -> Result<PathBuf, String> {
        if raw.is_empty() || raw.chars().any(char::is_control) {
            return Err("Enter a directory under HOME.".into());
        }
        let path = if raw == "~" {
            PathBuf::new()
        } else if let Some(rest) = raw.strip_prefix("~/") {
            PathBuf::from(rest)
        } else if raw.starts_with('~') {
            return Err("Only ~ and ~/ are supported.".into());
        } else if raw.starts_with('/') {
            std::path::Path::new(raw)
                .strip_prefix(&self.home)
                .map_err(|_| "Only directories under HOME are allowed.".to_string())?
                .to_owned()
        } else {
            PathBuf::from(raw)
        };
        Ok(path)
    }
    /// Check each component before reducing '..': symlinks are never followed.
    pub fn validate(&self, raw: &str) -> Result<String, String> {
        let relative = self.relative(raw)?;
        let mut checked = PathBuf::new();
        for component in relative.components() {
            match component {
                std::path::Component::Normal(name) => {
                    checked.push(name);
                    let metadata = fs::symlink_metadata(self.root.join(&checked))
                        .map_err(|e| format!("Directory unavailable: {e}"))?;
                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                        return Err(
                            "Choose a directory; symlinks and files are not supported.".into()
                        );
                    }
                }
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !checked.pop() {
                        return Err("Only directories under HOME are allowed.".into());
                    }
                }
                _ => return Err("Only directories under HOME are allowed.".into()),
            }
        }
        fs::read_dir(self.root.join(&checked).join("."))
            .map_err(|e| format!("Directory unavailable: {e}"))?;
        self.home
            .join(checked)
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| "Directory is not valid UTF-8.".into())
    }
    pub fn status(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        let state = if self.is_scanning() {
            "Indexing HOME"
        } else if self.limited {
            "HOME index limit reached"
        } else {
            "HOME indexed"
        };
        format!("{state} · {} dirs · F5 refresh", self.dirs.len())
    }
    pub fn restart(&self) -> Self {
        Self::new(self.home.clone(), self.root.clone()).expect("validated HOME")
    }
    pub fn is_scanning(&self) -> bool {
        !self.started || self.walker.is_some()
    }
    /// Bound returned entries and cooperate between iterator calls. One next()
    /// can consume many ignored entries or block in IO; this is not a syscall
    /// or hard time budget. The plugin calls this only inside its serial worker.
    pub fn step(&mut self, budget: usize) {
        if budget == 0 {
            return;
        }
        let started = std::time::Instant::now();
        if !self.started {
            self.started = true;
            if let Err(error) = self.validate("~") {
                self.error = Some(error);
                return;
            }
            let rules = Mutex::new(ignore_rules::Rules::new(
                &self.root,
                self.limits.max_bytes,
                self.rule_bytes.clone(),
            ));
            let mut builder = ignore::WalkBuilder::new(&self.root);
            builder
                .standard_filters(false)
                .follow_links(false)
                .parents(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .max_depth(Some(self.limits.max_depth))
                .filter_entry(move |entry| {
                    entry.depth() == 0
                        || entry.file_name().to_str().is_some_and(|name| {
                            name != "node_modules"
                                && !name.starts_with('.')
                                && !name.chars().any(char::is_control)
                        }) && rules.lock().expect("serial rule matcher").allows(entry)
                });
            self.walker = Some(builder.build());
        }
        for _ in 0..budget {
            if started.elapsed() >= std::time::Duration::from_millis(5) {
                break;
            }
            if self.dirs.len() >= self.limits.max_directories
                || self.visited >= self.limits.max_entries
                || self
                    .retained_bytes
                    .saturating_add(self.rule_bytes.load(Ordering::Relaxed))
                    > self.limits.max_bytes
            {
                self.limited = true;
                self.walker = None;
                break;
            }
            let Some(walker) = self.walker.as_mut() else {
                break;
            };
            let entry = match walker.next() {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    if error.depth() == Some(0) {
                        self.error = Some(format!("Directory unavailable: {error}"));
                        self.walker = None;
                        break;
                    }
                    continue;
                }
                None => {
                    self.walker = None;
                    break;
                }
            };
            if self
                .retained_bytes
                .saturating_add(self.rule_bytes.load(Ordering::Relaxed))
                > self.limits.max_bytes
            {
                self.limited = true;
                self.walker = None;
                break;
            }
            if entry.depth() == 0 {
                continue;
            }
            self.visited += 1;
            if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            if let Some(path) = self.home.join(relative).to_str() {
                // Keep the conservative path/allocation allowance even though
                // the serial DFS walker no longer retains a breadth-first queue.
                let cost = path.len() + relative.as_os_str().len() + 128;
                if path.len() > 4096
                    || self
                        .retained_bytes
                        .saturating_add(cost)
                        .saturating_add(self.rule_bytes.load(Ordering::Relaxed))
                        > self.limits.max_bytes
                {
                    self.limited = true;
                    self.walker = None;
                    break;
                }
                self.retained_bytes += cost;
                self.dirs.push(Directory {
                    path: path.into(),
                    note: "directory",
                    error: None,
                });
                if entry.depth() >= self.limits.max_depth {
                    self.limited = true;
                }
            }
        }
    }
}

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

/// Lexical only: never consults the host, a shell or the invoking directory.
/// Relative input resolves against HOME, which is also the only search root.
pub fn normalize_in(raw: &str, home: &str) -> Option<String> {
    crate::host_path::normalize(raw, home)
}

/// Embedded Frizbee ranks basename and full path; path order breaks ties.
pub fn matches_in(raw: &str, dirs: &[Directory], home: &str) -> Vec<usize> {
    use neo_frizbee::{Config, Matcher};
    if raw.chars().count() > MAX_INPUT_CHARS {
        return Vec::new();
    }
    let config = Config {
        max_typos: Some(0),
        ..Config::default()
    };
    let windows = crate::host_path::HostPath::parse(home).is_some_and(|p| p.is_windows());
    let raw = if windows {
        raw.replace('\\', "/")
    } else {
        raw.to_owned()
    };
    let raw = raw.as_str();
    let explicit = crate::host_path::HostPath::parse(raw).is_some()
        || raw.starts_with('/')
        || raw.starts_with('~')
        || raw.starts_with("./")
        || raw.starts_with("../");
    let query = if explicit {
        normalize_in(raw, home).unwrap_or_else(|| raw.into())
    } else {
        raw.to_owned()
    };
    // HOME expansion can exceed the editor limit. Bound Frizbee's scratch
    // matrices too; never truncate a path into a different search/launch target.
    if query.chars().count() > MAX_INPUT_CHARS {
        return Vec::new();
    }
    let query = if windows {
        query.replace('\\', "/")
    } else {
        query
    };
    let paths: Vec<_> = dirs
        .iter()
        .map(|d| {
            if windows {
                std::borrow::Cow::Owned(d.path.replace('\\', "/"))
            } else {
                std::borrow::Cow::Borrowed(d.path.as_str())
            }
        })
        .collect();
    let names: Vec<_> = paths
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p))
        .collect();
    let mut scores = vec![None; dirs.len()];
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
        .filter_map(|(i, score)| score.map(|s| (i, s)))
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
            max_bytes: 60 * 1024 * 1024,
            max_directories: 200_000,
            max_entries: 2_000_000,
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
    /// `home` is the displayed path; `root` is its filesystem root (normally identical).
    pub fn new(home: PathBuf, root: PathBuf) -> Result<Self, String> {
        if home.to_str().and_then(crate::host_path::HostPath::parse)
            .is_none_or(|p| !p.is_home())
            // /host is a WASI mount, including in native adapter tests.
            || !(root.is_absolute() || root == std::path::Path::new("/host"))
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
    fn host_path(&self) -> crate::host_path::HostPath {
        crate::host_path::HostPath::parse(self.home.to_str().expect("validated HOME")).unwrap()
    }
    fn relative(&self, raw: &str) -> Result<Vec<String>, String> {
        self.host_path()
            .relative(raw)
            .ok_or_else(|| "Enter a directory under HOME; only ~ and ~/ are supported.".into())
    }
    /// Check each component before reducing '..': symlinks are never followed.
    pub fn validate(&self, raw: &str) -> Result<String, String> {
        let relative = self.relative(raw)?;
        let mut checked = Vec::new();
        let mut sandbox = self.root.clone();
        for component in relative {
            if component == ".." {
                if checked.pop().is_none() {
                    return Err("Only directories under HOME are allowed.".into());
                }
                sandbox.pop();
            } else {
                // Host components must stay single, relative sandbox components.
                let path = std::path::Path::new(&component);
                if path.components().count() != 1
                    || !matches!(
                        path.components().next(),
                        Some(std::path::Component::Normal(_))
                    )
                {
                    return Err("Unsupported directory component.".into());
                }
                sandbox.push(&component);
                let metadata = fs::symlink_metadata(&sandbox)
                    .map_err(|e| format!("Directory unavailable: {e}"))?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err("Choose a directory; symlinks and files are not supported.".into());
                }
                checked.push(component);
            }
        }
        fs::read_dir(sandbox.join(".")).map_err(|e| format!("Directory unavailable: {e}"))?;
        Ok(self.host_path().join(&checked))
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
    /// or hard time budget. The native adapter calls this inside its background worker.
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
            let parts: Option<Vec<_>> = relative
                .components()
                .map(|c| c.as_os_str().to_str().map(str::to_owned))
                .collect();
            if let Some(parts) = parts {
                let path = self.host_path().join(&parts);
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
                    path,
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

//! Home-only cached directory index. No shell, subprocess, or render-time IO.
use crate::editor::MAX_INPUT_CHARS;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    time::{Duration, Instant},
};

#[cfg(windows)]
fn is_hidden(entry: &ignore::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    if entry.file_name().as_encoded_bytes().starts_with(b".") {
        return true;
    }
    // If metadata is unavailable, do not descend into an unclassified entry.
    entry.metadata().map_or(true, |metadata| {
        metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
    })
}

#[cfg(not(windows))]
fn is_hidden(entry: &ignore::DirEntry) -> bool {
    entry.file_name().as_encoded_bytes().starts_with(b".")
}

/// Lexical only: never consults the host, a shell or the invoking directory.
/// Relative input resolves against HOME, which is also the only search root.
pub fn normalize_in(raw: &str, home: &str) -> Option<String> {
    crate::host_path::normalize(raw, home)
}

/// Embedded Frizbee ranks basename and full path. Launch frecency raises the score:
/// the total is `score + score * frecency / 100`, as in fff-search. Equal totals prefer
/// higher frecency, then a directory whose own name contains the query's last `/`
/// segment, then shallower paths, then path order. These only break ties: Frizbee's local
/// alignment ignores trailing text, so a project and its subdirectories often score the same.
/// Typed text allows typos up to the limit from `max_typos`. Explicit paths allow none.
pub fn matches_in(
    raw: &str,
    dirs: &[Directory],
    home: &str,
    frecency: &crate::frecency::Frecency,
) -> Vec<usize> {
    use neo_frizbee::{Config, Matcher, Matching};
    if raw.chars().count() > MAX_INPUT_CHARS {
        return Vec::new();
    }
    let config = Config::default();
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
    // The last typed segment must appear in a directory's own name to win ties,
    // so `repo/branch` prefers the checkout over launched folders inside it.
    // Read it before HOME expansion: `~`, `./` and `..` name no folder.
    let mut last_segment = raw
        .rsplit('/')
        .next()
        .filter(|segment| !matches!(*segment, "" | "~" | "." | ".."))
        .map(|segment| Matcher::new(segment, &config.matching(Matching::Substring)));
    let mut scores = vec![None; dirs.len()];
    // An explicit path names one directory, so a typo must not match others.
    let typos = if explicit { 0 } else { max_typos(raw) };
    let mut matcher = Matcher::new(&query, &config.max_typos(Some(typos)));
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
        .filter_map(|(i, score)| {
            let score = u32::from(score?);
            let frecency_score = frecency.get(&dirs[i].path);
            let total = score + score.saturating_mul(frecency_score) / 100;
            let named = last_segment
                .as_mut()
                .is_none_or(|m| m.match_one(names[i], 0).is_some());
            let depth = paths[i].split('/').filter(|p| !p.is_empty()).count();
            Some((i, total, frecency_score, named, depth))
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(b.2.cmp(&a.2))
            .then(b.3.cmp(&a.3))
            .then(a.4.cmp(&b.4))
            .then_with(|| dirs[a.0].path.cmp(&dirs[b.0].path))
    });
    ranked.into_iter().map(|(i, ..)| i).collect()
}

/// The fff-search rule: about one typo for each four typed bytes, from 2 to 6.
fn max_typos(query: &str) -> u16 {
    (query.len() / 4).clamp(2, 6) as u16
}

#[derive(Debug, Clone)]
pub struct Directory {
    pub path: String,
    pub note: &'static str,
    pub error: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Conservative retained index accounting, excluding walker internals, not RSS.
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
    ignore: Vec<crate::host_path::HostPath>,
    pub limits: Limits,
    limited: bool,
    error: Option<String>,
    scan: Scan,
}
enum Scan {
    NotStarted,
    /// The walk starts after the coordinating thread of the cancelled walk exits.
    Waiting(std::thread::JoinHandle<()>),
    Running(ParallelScan),
    Finished,
}
/// How `HomeIndex::receive` waits for the walk threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receive {
    /// Return at once if no message is ready.
    Poll,
    /// Wait until the next batch arrives or the walk ends. Also wait for a cancelled walk.
    Block,
}
/// Only completed scans are eligible to replace a persistent snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanState {
    Scanning,
    Complete { limited: bool },
    Failed(String),
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
        Self::new_with_ignore(home, root, Vec::new())
    }
    /// Exclusions are absolute displayed host paths, independent of `root`.
    pub fn new_with_ignore(
        home: PathBuf,
        root: PathBuf,
        ignore: Vec<String>,
    ) -> Result<Self, String> {
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
            ignore: ignore
                .iter()
                .map(|raw| {
                    crate::host_path::normalize_absolute(raw)
                        .and_then(|path| crate::host_path::HostPath::parse(&path))
                        .ok_or_else(|| "Ignore entries must be supported absolute paths.".into())
                })
                .collect::<Result<_, String>>()?,
            limits: Limits::default(),
            limited: false,
            error: None,
            scan: Scan::NotStarted,
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
    /// Cancels the walk of `self`, and `self` stops scanning. The new index starts its walk
    /// after the cancelled walk exits.
    pub fn restart(&mut self) -> Self {
        let mut index = Self::new(self.home.clone(), self.root.clone()).expect("validated HOME");
        index.ignore = self.ignore.clone();
        index.limits = self.limits;
        index.scan = match std::mem::replace(&mut self.scan, Scan::Finished) {
            Scan::Running(mut scan) => scan.walk.take().map_or(Scan::NotStarted, Scan::Waiting),
            Scan::Waiting(walk) => Scan::Waiting(walk),
            Scan::NotStarted | Scan::Finished => Scan::NotStarted,
        };
        index
    }
    pub fn scan_state(&self) -> ScanState {
        if self.is_scanning() {
            ScanState::Scanning
        } else if let Some(error) = &self.error {
            ScanState::Failed(error.clone())
        } else {
            ScanState::Complete {
                limited: self.limited,
            }
        }
    }
    pub fn is_scanning(&self) -> bool {
        !matches!(self.scan, Scan::Finished)
    }
    /// Starts the walk after the cancelled walk exits. Adds at most one batch of directories.
    /// Returns true if the index changed. The completed index is sorted by path.
    pub fn receive(&mut self, mode: Receive) -> bool {
        if let Scan::Waiting(previous) = &self.scan {
            if mode == Receive::Poll && !previous.is_finished() {
                return false;
            }
            if let Scan::Waiting(previous) = std::mem::replace(&mut self.scan, Scan::NotStarted) {
                let _ = previous.join();
            }
        }
        if matches!(self.scan, Scan::NotStarted) {
            self.scan = self.start();
            if matches!(self.scan, Scan::Finished) {
                return true;
            }
        }
        let Scan::Running(scan) = &self.scan else {
            return false;
        };
        let message = match mode {
            Receive::Poll => match scan.messages.try_recv() {
                Ok(message) => Some(message),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => None,
            },
            Receive::Block => scan.messages.recv().ok(),
        };
        match message {
            Some(WalkMessage::Batch(dirs)) => {
                self.dirs.extend(dirs);
                return true;
            }
            Some(WalkMessage::Finished { limited, error }) => {
                self.limited = limited;
                self.error = error;
            }
            None => self.error = Some("HOME scan stopped unexpectedly.".into()),
        }
        self.scan = Scan::Finished;
        self.dirs.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        true
    }
    fn start(&mut self) -> Scan {
        let home = self.host_path();
        if self.ignore.iter().any(|path| path.contains(&home)) {
            return Scan::Finished;
        }
        if let Err(error) = self.validate("~") {
            self.error = Some(error);
            return Scan::Finished;
        }
        let mut builder = ignore::WalkBuilder::new(&self.root);
        let root = self.root.clone();
        let ignore = self.ignore.clone();
        let filter_home = home.clone();
        builder
            .follow_links(false)
            .max_depth(Some(self.limits.max_depth))
            .filter_entry(move |entry| {
                if entry.depth() == 0 {
                    return true;
                }
                if !ignore.is_empty() {
                    let Some(parts) = entry.path().strip_prefix(&root).ok().and_then(|path| {
                        path.components()
                            .map(|part| part.as_os_str().to_str().map(str::to_owned))
                            .collect::<Option<Vec<_>>>()
                    }) else {
                        return false;
                    };
                    let Some(path) = crate::host_path::HostPath::parse(&filter_home.join(&parts))
                    else {
                        return false;
                    };
                    if ignore.iter().any(|ignored| ignored.contains(&path)) {
                        return false;
                    }
                }
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| !name.chars().any(char::is_control))
                    && !is_hidden(entry)
            });
        let walk = builder.build_parallel();
        let shared = Arc::new(WalkShared {
            root: self.root.clone(),
            home,
            limits: self.limits,
            cancel: AtomicBool::new(false),
            entries: AtomicUsize::new(0),
            directories: AtomicUsize::new(0),
            retained_bytes: AtomicUsize::new(0),
            limited: AtomicBool::new(false),
            error: OnceLock::new(),
        });
        let (sender, messages) = mpsc::sync_channel(BATCH_CHANNEL_CAPACITY);
        let thread_shared = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("home-walk".into())
            .spawn(move || {
                walk.visit(&mut VisitorBuilder {
                    shared: Arc::clone(&thread_shared),
                    sender: sender.clone(),
                });
                // The walk joins its threads before it returns, so every batch is sent.
                let _ = sender.send(WalkMessage::Finished {
                    limited: thread_shared.limited.load(Ordering::Relaxed),
                    error: thread_shared.error.get().cloned(),
                });
            });
        match spawned {
            Ok(walk) => Scan::Running(ParallelScan {
                messages,
                shared,
                walk: Some(walk),
            }),
            Err(error) => {
                self.error = Some(format!("Directory scan unavailable: {error}"));
                Scan::Finished
            }
        }
    }
}

/// Directories in one batch. A smaller batch gives results sooner.
const BATCH_LEN: usize = 128;
/// A walk thread sends a partial batch at its next entry after this time.
const BATCH_AGE: Duration = Duration::from_millis(10);
/// Batches that wait for the receiver. When the channel is full, the walk threads wait.
const BATCH_CHANNEL_CAPACITY: usize = 16;

enum WalkMessage {
    Batch(Vec<Directory>),
    Finished {
        limited: bool,
        error: Option<String>,
    },
}

/// A running walk. Drop tells the walk threads to stop and discards their late batches.
struct ParallelScan {
    messages: Receiver<WalkMessage>,
    shared: Arc<WalkShared>,
    /// The coordinating thread. It exits after all walk threads exit.
    walk: Option<std::thread::JoinHandle<()>>,
}
impl Drop for ParallelScan {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }
}

/// State that all walk threads share. The counters record the use of each `Limits` budget.
struct WalkShared {
    root: PathBuf,
    home: crate::host_path::HostPath,
    limits: Limits,
    cancel: AtomicBool,
    entries: AtomicUsize,
    directories: AtomicUsize,
    retained_bytes: AtomicUsize,
    limited: AtomicBool,
    error: OnceLock<String>,
}
impl WalkShared {
    fn stop_at_limit(&self) -> ignore::WalkState {
        self.limited.store(true, Ordering::Relaxed);
        ignore::WalkState::Quit
    }
}

/// Adds `amount` to `counter` and returns the new total, or `None` if the total exceeds `limit`.
fn claim(counter: &AtomicUsize, amount: usize, limit: usize) -> Option<usize> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(amount).filter(|total| *total <= limit)
        })
        .ok()
        .map(|used| used + amount)
}

struct VisitorBuilder {
    shared: Arc<WalkShared>,
    sender: SyncSender<WalkMessage>,
}
impl<'s> ignore::ParallelVisitorBuilder<'s> for VisitorBuilder {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 's> {
        Box::new(Visitor {
            shared: Arc::clone(&self.shared),
            sender: self.sender.clone(),
            batch: Vec::new(),
            batch_started: Instant::now(),
        })
    }
}

/// One walk thread. Drop sends the last partial batch.
struct Visitor {
    shared: Arc<WalkShared>,
    sender: SyncSender<WalkMessage>,
    batch: Vec<Directory>,
    batch_started: Instant,
}
impl Visitor {
    /// Applies the budgets to one entry below HOME and keeps it if it is a directory.
    fn add(&mut self, entry: &ignore::DirEntry) -> ignore::WalkState {
        use ignore::WalkState;
        let shared = &*self.shared;
        let limits = shared.limits;
        let Some(entries) = claim(&shared.entries, 1, limits.max_entries) else {
            return shared.stop_at_limit();
        };
        let state = if entries == limits.max_entries {
            shared.stop_at_limit()
        } else {
            WalkState::Continue
        };
        if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
            return state;
        }
        let Ok(relative) = entry.path().strip_prefix(&shared.root) else {
            return state;
        };
        let Some(parts) = relative
            .components()
            .map(|c| c.as_os_str().to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
        else {
            return state;
        };
        let path = shared.home.join(&parts);
        // Estimate the retained size of the path and its allocation.
        let cost = path.len() + relative.as_os_str().len() + 128;
        if path.len() > 4096 || claim(&shared.retained_bytes, cost, limits.max_bytes).is_none() {
            return shared.stop_at_limit();
        }
        let Some(directories) = claim(&shared.directories, 1, limits.max_directories) else {
            return shared.stop_at_limit();
        };
        if self.batch.is_empty() {
            self.batch_started = Instant::now();
        }
        self.batch.push(Directory {
            path,
            note: "directory",
            error: None,
        });
        if entry.depth() >= limits.max_depth {
            shared.limited.store(true, Ordering::Relaxed);
        }
        if directories == limits.max_directories {
            return shared.stop_at_limit();
        }
        state
    }
    /// Returns false if the receiver is gone.
    fn send_batch(&mut self) -> bool {
        self.batch.is_empty()
            || self
                .sender
                .send(WalkMessage::Batch(std::mem::take(&mut self.batch)))
                .is_ok()
    }
}
impl ignore::ParallelVisitor for Visitor {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> ignore::WalkState {
        use ignore::WalkState;
        if self.shared.cancel.load(Ordering::Relaxed) {
            return WalkState::Quit;
        }
        let state = match entry {
            Ok(entry) if entry.depth() == 0 => WalkState::Continue,
            Ok(entry) => self.add(&entry),
            Err(error) if error.depth() == Some(0) => {
                let _ = self
                    .shared
                    .error
                    .set(format!("Directory unavailable: {error}"));
                WalkState::Quit
            }
            // Inaccessible subdirectories are skipped.
            Err(_) => WalkState::Continue,
        };
        let full = self.batch.len() >= BATCH_LEN
            || (!self.batch.is_empty() && self.batch_started.elapsed() >= BATCH_AGE);
        if full && !self.send_batch() {
            return WalkState::Quit;
        }
        state
    }
}
impl Drop for Visitor {
    fn drop(&mut self) {
        self.send_batch();
    }
}

#[cfg(test)]
mod walk_tests {
    use super::*;

    fn wait_until(mut done: impl FnMut() -> bool) {
        let started = Instant::now();
        while !done() {
            assert!(started.elapsed() < Duration::from_secs(10), "timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_new_walk_waits_until_the_cancelled_walk_exits() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/search-unit-tests")
            .join(format!("{}-held-walk", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("one/two")).unwrap();
        let home = home.canonicalize().unwrap();
        let mut first = HomeIndex::new(home.clone(), home.clone()).unwrap();
        first.receive(Receive::Poll);
        let Scan::Running(scan) = &mut first.scan else {
            panic!("the first walk starts");
        };
        // This thread replaces the coordinating thread and waits like a hung filesystem call.
        let (release, gate) = mpsc::channel::<()>();
        let held = std::thread::spawn(move || {
            let _ = gate.recv();
        });
        let _ = scan.walk.replace(held);
        let shared = Arc::clone(&scan.shared);

        let mut second = first.restart();
        assert!(shared.cancel.load(Ordering::Relaxed));
        for _ in 0..20 {
            assert!(!second.receive(Receive::Poll));
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(second.scan, Scan::Waiting(_)), "no second walk");
        let mut third = second.restart();
        assert!(
            matches!(second.scan, Scan::Finished),
            "the requests collapse into one"
        );
        assert!(!third.receive(Receive::Poll));
        assert!(matches!(third.scan, Scan::Waiting(_)));

        release.send(()).unwrap();
        wait_until(|| {
            third.receive(Receive::Poll);
            !third.is_scanning()
        });
        assert_eq!(third.scan_state(), ScanState::Complete { limited: false });
        assert_eq!(third.dirs.len(), 2);
    }
}

//! One bounded native worker owns persistence, traversal, matching and validation.
use launchpad_core::{
    editor::MAX_INPUT_CHARS,
    remote::RemoteRequest,
    search::{HomeIndex, ScanState},
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::index_store::{Snapshot, Store};

pub struct Request {
    pub epoch: u64,
    pub request: RemoteRequest,
}
#[derive(Debug)]
pub enum Reply {
    Progress {
        epoch: u64,
        revision: u64,
        status: String,
    },
    Results {
        epoch: u64,
        generation: u64,
        revision: u64,
        paths: Vec<String>,
    },
    Validated {
        epoch: u64,
        generation: u64,
        result: Result<String, String>,
    },
    Failed(String),
}
pub struct Worker {
    pub requests: SyncSender<Request>,
    pub replies: Receiver<Reply>,
}
impl Worker {
    pub fn start(home: PathBuf, cwd: String) -> std::io::Result<Self> {
        Self::start_with_ignore(home, cwd, Vec::new())
    }
    pub fn start_with_ignore(
        home: PathBuf,
        cwd: String,
        ignore: Vec<String>,
    ) -> std::io::Result<Self> {
        Self::start_with_cache(home, cwd, ignore, None)
    }
    /// Load and publish snapshots under `cache_root`; `None` keeps IO limited to
    /// traversal and validation, as used by the uncached constructors and tests.
    pub fn start_with_cache(
        home: PathBuf,
        cwd: String,
        ignore: Vec<String>,
        cache_root: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let (requests, demand) = mpsc::sync_channel::<Request>(1);
        let (responses, replies) = mpsc::sync_channel(4);
        thread::Builder::new()
            .name("home-search".into())
            .spawn(move || {
                if let Err(error) = run(home, cwd, ignore, cache_root, demand, &responses) {
                    let _ = responses.send(Reply::Failed(error));
                }
            })?;
        Ok(Self { requests, replies })
    }
}
fn run(
    home: PathBuf,
    cwd: String,
    ignore: Vec<String>,
    cache_root: Option<PathBuf>,
    demand: Receiver<Request>,
    responses: &SyncSender<Reply>,
) -> Result<(), String> {
    let index = HomeIndex::new_with_ignore(home.clone(), home.clone(), ignore.clone())?;
    let store = cache_root.map(|root| Store::new(root, &home, &ignore, index.limits));
    let mut state = SearchIndex::new(index, store);
    // Loading precedes traversal and the first query. The UI itself never waits
    // synchronously for disk IO.
    if responses
        .send(Reply::Progress {
            epoch: 0,
            revision: state.revision,
            status: state.status(),
        })
        .is_err()
    {
        return Ok(());
    }
    let mut epoch = 0;
    let mut progress = Instant::now() - Duration::from_secs(1);
    loop {
        let request = if state.building.is_scanning() {
            match demand.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => break,
            }
        } else {
            match demand.recv() {
                Ok(r) => Some(r),
                Err(_) => break,
            }
        };
        if let Some(Request {
            epoch: requested,
            request,
        }) = request
        {
            if requested != epoch {
                epoch = requested;
                state.restart();
            }
            let reply = match request {
                RemoteRequest::Query {
                    generation,
                    text,
                    recent,
                } => {
                    let mut bytes = 0;
                    let paths = if text.chars().count() > MAX_INPUT_CHARS {
                        Vec::new()
                    } else {
                        launchpad_core::search::matches_in(
                            &text,
                            state.dirs(),
                            home.to_str().unwrap(),
                            &recent,
                        )
                        .into_iter()
                        .take(100)
                        .map(|i| &state.dirs()[i].path)
                        .take_while(|p| {
                            bytes += p.len();
                            bytes <= 64 * 1024
                        })
                        .cloned()
                        .collect()
                    };
                    Reply::Results {
                        epoch,
                        generation,
                        revision: state.revision,
                        paths,
                    }
                }
                RemoteRequest::Validate { generation, raw } => Reply::Validated {
                    epoch,
                    generation,
                    result: validate(&state.building, &cwd, &raw),
                },
            };
            if responses.send(reply).is_err() {
                break;
            }
        }
        if state.building.is_scanning() {
            state.step(128);
            if progress.elapsed() >= Duration::from_millis(100) || !state.building.is_scanning() {
                if responses
                    .send(Reply::Progress {
                        epoch,
                        revision: state.revision,
                        status: state.status(),
                    })
                    .is_err()
                {
                    break;
                }
                progress = Instant::now();
            }
        }
    }
    Ok(())
}
/// The active snapshot never changes while a replacement is traversed.
struct SearchIndex {
    building: HomeIndex,
    active: Option<Snapshot>,
    store: Option<Store>,
    revision: u64,
    warning: Option<String>,
}
impl SearchIndex {
    fn new(building: HomeIndex, store: Option<Store>) -> Self {
        let (active, warning) = match store.as_ref().map(Store::load) {
            Some(Ok(snapshot)) => (snapshot, None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, None),
        };
        Self {
            building,
            revision: u64::from(active.is_some()),
            active,
            store,
            warning,
        }
    }
    fn dirs(&self) -> &[launchpad_core::search::Directory] {
        self.active
            .as_ref()
            .map_or(&self.building.dirs, |s| &s.dirs)
    }
    fn restart(&mut self) {
        // Even during a cold scan, F5 keeps the currently searchable results.
        if self.active.is_none() && !self.building.dirs.is_empty() {
            self.active = Some(Snapshot {
                dirs: std::mem::take(&mut self.building.dirs),
                limited: true,
            });
        }
        self.building = self.building.restart();
    }
    fn step(&mut self, budget: usize) {
        if !self.building.is_scanning() {
            return;
        }
        let before = self.building.dirs.len();
        self.building.step(budget);
        if self.active.is_none() && before != self.building.dirs.len() {
            self.revision += 1;
        }
        if let ScanState::Complete { limited } = self.building.scan_state() {
            let snapshot = Snapshot {
                dirs: std::mem::take(&mut self.building.dirs),
                limited,
            };
            if let Some(store) = &self.store {
                self.warning = store.save(&snapshot).err();
            }
            self.active = Some(snapshot);
            self.revision += 1;
        }
    }
    fn status(&self) -> String {
        let mut status = match self.building.scan_state() {
            ScanState::Scanning if self.active.is_some() => format!(
                "Reindexing HOME · {} dirs available · {} scanned · F5 refresh",
                self.dirs().len(),
                self.building.dirs.len()
            ),
            ScanState::Scanning => self.building.status(),
            ScanState::Failed(error) => {
                format!("{error} · {} dirs retained · F5 refresh", self.dirs().len())
            }
            ScanState::Complete { .. } => format!(
                "{} · {} dirs · F5 refresh",
                if self.active.as_ref().is_some_and(|s| s.limited) {
                    "HOME index limit reached"
                } else {
                    "HOME indexed"
                },
                self.dirs().len()
            ),
        };
        if let Some(warning) = &self.warning {
            status.push_str(" · ");
            status.push_str(warning);
        }
        status
    }
}

pub fn validate(index: &HomeIndex, cwd: &str, raw: &str) -> Result<String, String> {
    if raw.len() > 4096 {
        return Err("Path exceeds 4096 bytes".into());
    }
    let label = index
        .home
        .to_str()
        .and_then(|home| launchpad_core::host_path::label(cwd, home));
    if raw == cwd || label.as_deref() == Some(raw) {
        std::fs::read_dir(cwd)
            .map_err(|e| format!("Invoking directory unavailable; no fallback: {e}"))?;
        Ok(cwd.into())
    } else {
        index.validate(raw)
    }
}
pub fn initial_cwd() -> Result<String, String> {
    let physical =
        std::env::current_dir().map_err(|e| format!("Invoking directory unavailable: {e}"))?;
    // Keep the shell's logical symlink spelling only if it names this actual cwd.
    let path = std::env::var_os("PWD")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.canonicalize().ok().as_ref() == Some(&physical))
        .unwrap_or(physical);
    let text = path.to_str().ok_or("Invoking directory is not UTF-8")?;
    if text.len() > 4096
        || text.chars().any(char::is_control)
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err("Invoking directory unavailable or unsupported; no fallback".into());
    }
    Ok(text.into())
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use std::fs;
    struct Fixture {
        root: PathBuf,
        home: PathBuf,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../target/index-worker-tests")
                .join(format!("{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let home = root.join("home");
            fs::create_dir_all(&home).unwrap();
            let home = home.canonicalize().unwrap();
            Self { root, home }
        }
        fn state(&self) -> SearchIndex {
            let index = HomeIndex::new(self.home.clone(), self.home.clone()).unwrap();
            let store = Store::new(self.root.join("cache"), &self.home, &[], index.limits);
            SearchIndex::new(index, Some(store))
        }
        fn add(&self, name: &str) {
            fs::create_dir(self.home.join(name)).unwrap();
        }
    }
    fn finish(state: &mut SearchIndex) {
        while state.building.is_scanning() {
            state.step(1);
        }
    }
    fn names(state: &SearchIndex) -> Vec<String> {
        state
            .dirs()
            .iter()
            .map(|d| {
                d.path
                    .replace('\\', "/")
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }
    #[test]
    fn cold_progress_then_cached_startup_and_equal_count_replacement() {
        let f = Fixture::new("startup");
        for n in 0..5 {
            f.add(&format!("old-{n}"));
        }
        let mut first = f.state();
        while first.dirs().is_empty() {
            first.step(1);
        }
        assert!(first.building.is_scanning());
        assert!(first.store.as_ref().unwrap().load().unwrap().is_none());
        finish(&mut first);
        let old = names(&first);
        for name in &old {
            fs::remove_dir(f.home.join(name)).unwrap();
        }
        for n in 0..5 {
            f.add(&format!("new-{n}"));
        }
        let mut reopened = f.state();
        assert_eq!(names(&reopened), old, "cache loaded before any traversal");
        assert!(validate(&reopened.building, f.home.to_str().unwrap(), "~/old-0").is_err());
        let revision = reopened.revision;
        reopened.step(1);
        assert!(reopened.building.is_scanning());
        assert_eq!(names(&reopened), old);
        assert_eq!(reopened.revision, revision);
        finish(&mut reopened);
        assert_eq!(reopened.dirs().len(), old.len());
        assert!(names(&reopened).iter().all(|name| name.starts_with("new-")));
        assert!(reopened.revision > revision);
        assert_eq!(names(&f.state()), names(&reopened));
    }
    #[test]
    fn refresh_interruptions_shrinking_and_fatal_failure_keep_active_index() {
        let f = Fixture::new("refresh");
        f.add("keep");
        f.add("remove");
        let mut state = f.state();
        finish(&mut state);
        let old = names(&state);
        state.restart();
        state.step(1);
        state.restart();
        assert_eq!(names(&state), old);
        fs::remove_dir(f.home.join("remove")).unwrap();
        let revision = state.revision;
        finish(&mut state);
        assert_eq!(names(&state), ["keep"]);
        assert!(state.revision > revision);
        fs::remove_dir_all(&f.home).unwrap();
        state.restart();
        finish(&mut state);
        assert!(matches!(state.building.scan_state(), ScanState::Failed(_)));
        assert_eq!(names(&state), ["keep"]);
        assert_eq!(names(&f.state()), ["keep"]);
    }
    #[test]
    fn save_failure_is_nonfatal_and_limited_scans_roundtrip() {
        let f = Fixture::new("save-failure");
        f.add("one");
        f.add("two");
        let mut state = f.state();
        fs::write(f.root.join("cache"), "blocked").unwrap();
        finish(&mut state);
        assert_eq!(state.dirs().len(), 2);
        assert!(state.warning.is_some());
        fs::remove_file(f.root.join("cache")).unwrap();
        state.restart();
        state.building.limits.max_directories = 1;
        state.store = Some(Store::new(
            f.root.join("cache"),
            &f.home,
            &[],
            state.building.limits,
        ));
        finish(&mut state);
        assert!(state.active.as_ref().unwrap().limited);
        assert!(
            state
                .store
                .as_ref()
                .unwrap()
                .load()
                .unwrap()
                .unwrap()
                .limited
        );
        assert!(state.warning.is_none());
    }
}

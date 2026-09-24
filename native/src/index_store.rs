//! Disposable, versioned directory snapshots. Only complete scans are published.
use launchpad_core::{
    host_path::{HostPath, normalize_absolute},
    search::{Directory, Limits},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const VERSION: u32 = 1;
static SERIAL: AtomicU64 = AtomicU64::new(0);

pub fn cache_root(home: &Path) -> PathBuf {
    cache_root_for(std::env::consts::OS, home, |key| std::env::var_os(key))
}

fn cache_root_for(
    os: &str,
    home: &Path,
    get: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> PathBuf {
    let absolute = |key| {
        get(key).map(PathBuf::from).filter(|p| {
            p.to_str()
                .and_then(HostPath::parse)
                .is_some_and(|p| !p.has_parent() && p.is_windows() == (os == "windows"))
        })
    };
    let root = match os {
        "macos" => home.join("Library/Caches"),
        "windows" => absolute("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local")),
        _ => absolute("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")),
    };
    root.join("launchpad")
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    home: String,
    ignore: Vec<String>,
    limits: [usize; 4],
}
#[derive(Deserialize)]
struct DiskSnapshot {
    version: u32,
    identity: Identity,
    limited: bool,
    paths: Vec<String>,
}
#[derive(Serialize)]
struct DiskWrite<'a> {
    version: u32,
    identity: &'a Identity,
    limited: bool,
    paths: Vec<&'a str>,
}

pub(crate) struct Snapshot {
    pub dirs: Vec<Directory>,
    pub limited: bool,
}
pub(crate) struct Store {
    root: PathBuf,
    identity: Identity,
}
impl Store {
    pub(crate) fn new(root: PathBuf, home: &Path, ignore: &[String], limits: Limits) -> Self {
        let mut ignore: Vec<_> = ignore
            .iter()
            .filter_map(|p| normalize_absolute(p))
            .collect();
        ignore.sort();
        ignore.dedup();
        Self {
            root,
            identity: Identity {
                home: normalize_absolute(home.to_str().expect("validated HOME"))
                    .expect("validated HOME"),
                ignore,
                limits: [
                    limits.max_bytes,
                    limits.max_directories,
                    limits.max_entries,
                    limits.max_depth,
                ],
            },
        }
    }
    pub(crate) fn load(&self) -> Result<Option<Snapshot>, String> {
        self.load_inner()
            .map_err(|e| format!("Index cache unavailable: {e}"))
    }
    fn load_inner(&self) -> io::Result<Option<Snapshot>> {
        safe_path(&self.root, true)?;
        let path = self.root.join("index.json");
        safe_path(&path, false)?;
        let file = match retry(|| fs::File::open(&path)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        // JSON can escape each path byte into six bytes. Never read beyond this
        // finite limit, including when the file grows after metadata inspection.
        let max = self.identity.limits[0]
            .saturating_mul(6)
            .saturating_add(1024 * 1024) as u64;
        if file.metadata()?.len() > max {
            return Ok(None);
        }
        let snapshot: DiskSnapshot =
            match serde_json::from_reader(io::BufReader::new(file.take(max + 1))) {
                Ok(snapshot) => snapshot,
                Err(_) => return Ok(None),
            };
        if snapshot.version != VERSION
            || snapshot.identity != self.identity
            || snapshot.paths.len() > self.identity.limits[1]
        {
            return Ok(None);
        }
        let home = HostPath::parse(&self.identity.home).unwrap();
        let ignored: Vec<_> = self
            .identity
            .ignore
            .iter()
            .filter_map(|p| HostPath::parse(p))
            .collect();
        let mut cost = 0usize;
        let mut seen = std::collections::HashSet::new();
        for path in &snapshot.paths {
            let Some(parsed) = HostPath::parse(path) else {
                return Ok(None);
            };
            let Some(parts) = home.relative(path) else {
                return Ok(None);
            };
            if parsed.has_parent()
                || parts.is_empty()
                || parts.len() > self.identity.limits[3]
                || ignored.iter().any(|p| p.contains(&parsed))
                || !seen.insert(path)
            {
                return Ok(None);
            }
            let relative_bytes = parts.iter().map(String::len).sum::<usize>() + parts.len() - 1;
            cost = cost.saturating_add(path.len() + relative_bytes + 128);
            if cost > self.identity.limits[0] {
                return Ok(None);
            }
        }
        Ok(Some(Snapshot {
            dirs: snapshot
                .paths
                .into_iter()
                .map(|path| Directory {
                    path,
                    note: "directory",
                    error: None,
                })
                .collect(),
            limited: snapshot.limited,
        }))
    }
    pub(crate) fn save(&self, snapshot: &Snapshot) -> Result<(), String> {
        self.save_inner(snapshot)
            .map_err(|e| format!("Index cache unavailable: {e}"))
    }
    fn save_inner(&self, snapshot: &Snapshot) -> io::Result<()> {
        safe_path(&self.root, true)?;
        fs::create_dir_all(&self.root)?;
        let target = self.root.join("index.json");
        safe_path(&target, false)?;
        let (temp, mut file) = loop {
            let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
            let temp = self
                .root
                .join(format!("index-{}-{serial}.tmp", std::process::id()));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
            {
                Ok(file) => break (temp, file),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        let result = (|| {
            {
                let mut writer = io::BufWriter::new(&mut file);
                serde_json::to_writer(
                    &mut writer,
                    &DiskWrite {
                        version: VERSION,
                        identity: &self.identity,
                        limited: snapshot.limited,
                        paths: snapshot.dirs.iter().map(|d| d.path.as_str()).collect(),
                    },
                )?;
                writer.flush()?;
            }
            file.sync_all()?;
            // Close before replacement, including on Windows.
            drop(file);
            retry(|| fs::rename(&temp, &target))
        })();
        let _ = fs::remove_file(temp);
        result
    }
}
fn safe_path(path: &Path, directory: bool) -> io::Result<()> {
    match retry(|| fs::symlink_metadata(path)) {
        Ok(m) if (directory && m.is_dir()) || (!directory && m.is_file()) => Ok(()),
        Ok(_) => Err(io::Error::other("Unsafe index cache path")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
fn retry<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    for delay in [1, 2, 4, 8] {
        match operation() {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            result => return result,
        }
    }
    operation()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> Store {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/index-store-tests")
            .join(format!("{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Store::new(root, Path::new("/home/ada"), &[], Limits::default())
    }
    fn snapshot(paths: &[&str]) -> Snapshot {
        Snapshot {
            dirs: paths
                .iter()
                .map(|path| Directory {
                    path: (*path).into(),
                    note: "directory",
                    error: None,
                })
                .collect(),
            limited: false,
        }
    }
    #[test]
    fn locations_use_platform_defaults_and_absolute_overrides() {
        let home = Path::new("/home/ada");
        assert_eq!(
            cache_root_for("linux", home, |_| None),
            home.join(".cache/launchpad")
        );
        assert_eq!(
            cache_root_for("linux", home, |_| Some("relative".into())),
            home.join(".cache/launchpad")
        );
        assert_eq!(
            cache_root_for("linux", home, |_| Some("/cache".into())),
            Path::new("/cache/launchpad")
        );
        assert_eq!(
            cache_root_for("macos", home, |_| Some("/ignored".into())),
            home.join("Library/Caches/launchpad")
        );
        let windows = Path::new(r"C:\Users\Ada");
        assert_eq!(
            cache_root_for("windows", windows, |_| None),
            windows.join("AppData/Local/launchpad")
        );
        assert_eq!(
            cache_root_for("windows", windows, |_| Some(r"D:\Cache".into())),
            Path::new(r"D:\Cache").join("launchpad")
        );
    }
    #[test]
    fn roundtrip_replacement_unicode_and_windows_paths() {
        for home in ["/home/ada", r"C:\Users\Ada", r"\\server\share\Ada"] {
            let mut store = fixture(&format!("roundtrip-{}", home.len()));
            store.identity.home = normalize_absolute(home).unwrap();
            let path = HostPath::parse(home)
                .unwrap()
                .join(&["修理 it's literal".into()]);
            store.save(&snapshot(&[&path])).unwrap();
            assert_eq!(store.load().unwrap().unwrap().dirs[0].path, path);
            let mut empty = snapshot(&[]);
            empty.limited = true;
            store.save(&empty).unwrap();
            let reopened = Store::new(store.root.clone(), Path::new(home), &[], Limits::default());
            let loaded = reopened.load().unwrap().unwrap();
            assert!(loaded.dirs.is_empty());
            assert!(loaded.limited);
        }
    }
    #[test]
    fn incompatible_malformed_and_untrusted_snapshots_are_cache_misses() {
        let store = fixture("invalid");
        assert!(store.load().unwrap().is_none());
        store.save(&snapshot(&["/home/ada/project"])).unwrap();
        let saved = fs::read(store.root.join("index.json")).unwrap();
        for home in ["/home/other", "/home/ada"] {
            let ignore = if home.ends_with("ada") {
                vec!["/home/ada/project".into()]
            } else {
                vec![]
            };
            let other = Store::new(
                store.root.clone(),
                Path::new(home),
                &ignore,
                Limits::default(),
            );
            assert!(other.load().unwrap().is_none());
        }
        let mut changed = Store::new(
            store.root.clone(),
            Path::new("/home/ada"),
            &[],
            Limits::default(),
        );
        changed.identity.limits[3] -= 1;
        assert!(changed.load().unwrap().is_none());
        for paths in [
            vec!["/outside"],
            vec!["/home/ada/../escape"],
            vec!["relative"],
            vec!["/home/ada/a", "/home/ada/a"],
            vec!["/home/ada/\n"],
        ] {
            let mut json: serde_json::Value = serde_json::from_slice(&saved).unwrap();
            json["paths"] = serde_json::json!(paths);
            fs::write(
                store.root.join("index.json"),
                serde_json::to_vec(&json).unwrap(),
            )
            .unwrap();
            assert!(store.load().unwrap().is_none());
        }
        let mut json: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        json["version"] = 99.into();
        fs::write(
            store.root.join("index.json"),
            serde_json::to_vec(&json).unwrap(),
        )
        .unwrap();
        assert!(store.load().unwrap().is_none());
        fs::write(store.root.join("index.json"), b"{broken").unwrap();
        assert!(store.load().unwrap().is_none());
        // A sparse oversized cache is rejected before JSON allocation.
        fs::File::create(store.root.join("index.json"))
            .unwrap()
            .set_len((Limits::default().max_bytes * 6 + 1024 * 1024 + 1) as u64)
            .unwrap();
        assert!(store.load().unwrap().is_none());
    }
    #[test]
    fn concurrent_readers_only_observe_complete_snapshots() {
        let store = fixture("concurrent");
        store.save(&snapshot(&["/home/ada/initial"])).unwrap();
        std::thread::scope(|scope| {
            for n in 0..4 {
                let store = &store;
                scope.spawn(move || {
                    for _ in 0..20 {
                        store
                            .save(&snapshot(&[&format!("/home/ada/writer-{n}")]))
                            .unwrap();
                        let loaded = store.load().unwrap().expect("published file always exists");
                        assert_eq!(loaded.dirs.len(), 1);
                    }
                });
            }
        });
        assert_eq!(fs::read_dir(&store.root).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn denied_publication_preserves_disk_snapshot_and_cleans_temporary_file() {
        use std::os::unix::fs::PermissionsExt;
        let store = fixture("denied");
        store.save(&snapshot(&["/home/ada/original"])).unwrap();
        let saved = fs::read(store.root.join("index.json")).unwrap();
        let permissions = fs::metadata(&store.root).unwrap().permissions();
        fs::set_permissions(&store.root, fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.save(&snapshot(&["/home/ada/new"]));
        fs::set_permissions(&store.root, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(store.root.join("index.json")).unwrap(), saved);
        assert_eq!(fs::read_dir(&store.root).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn symlink_target_is_not_read_or_replaced() {
        let store = fixture("symlink");
        let sentinel = store.root.join("sentinel");
        fs::write(&sentinel, "KEEP").unwrap();
        std::os::unix::fs::symlink(&sentinel, store.root.join("index.json")).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&snapshot(&[])).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "KEEP");
    }
}

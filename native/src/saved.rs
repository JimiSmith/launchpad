//! Explicitly saved directories, shared across panes independently of history.
use launchpad_core::{app::SavedMutation, host_path::HostPath};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    directories: Vec<String>,
}
pub struct Store {
    root: PathBuf,
}
fn safe(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(format!("Unsafe saved-directory file: {}", path.display())),
        Err(e) => Err(e.to_string()),
    }
}
impl Store {
    pub fn new(root: &Path) -> Self {
        Self { root: root.into() }
    }
    fn lock(&self) -> Result<File, String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let path = self.root.join("saved.lock");
        safe(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.try_lock()
            .map_err(|e| format!("Saved directories busy or unavailable; retry: {e}"))?;
        Ok(file)
    }
    fn read(&self) -> Result<Vec<String>, String> {
        let path = self.root.join("saved.json");
        safe(&path)?;
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Saved directories exceed 4 MiB".into());
        }
        let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if snapshot.version != 1 {
            return Err("Unsupported saved directories version".into());
        }
        if snapshot.directories.iter().any(|p| !valid(p)) {
            return Err("Invalid saved directory".into());
        }
        let mut paths = snapshot.directories;
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
    pub fn load(&self) -> Result<Vec<String>, String> {
        let _lock = self.lock()?;
        self.read()
    }
    /// Merge against the latest file under an OS lock, released even after a crash.
    pub fn mutate(&self, mutation: SavedMutation) -> Result<(Vec<String>, &'static str), String> {
        let _lock = self.lock()?;
        let mut paths = self.read()?;
        let message = match mutation {
            SavedMutation::Add(path) => {
                if !valid(&path) {
                    return Err("Invalid saved directory".into());
                }
                if paths.contains(&path) {
                    return Ok((paths, "Already saved"));
                }
                paths.push(path);
                paths.sort();
                "Directory saved"
            }
            SavedMutation::Remove(path) => {
                paths.retain(|p| p != &path);
                "Directory removed from Saved"
            }
        };
        let bytes = serde_json::to_vec(&Snapshot {
            version: 1,
            directories: paths.clone(),
        })
        .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Saved directories exceed 4 MiB".into());
        }
        let temp = self.root.join("saved.tmp");
        safe(&temp)?;
        // A prior crashed writer may have left a partial temp; the lock owns it.
        let result = (|| {
            let mut file = File::create(&temp).map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            fs::rename(&temp, self.root.join("saved.json")).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result?;
        Ok((paths, message))
    }
}
fn valid(path: &str) -> bool {
    path.len() <= 4096
        && !path.chars().any(char::is_control)
        && HostPath::parse(path).is_some_and(|p| !p.has_parent())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("launchpad-saved-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn instances_merge_and_saved_entries_never_expire_with_history_limit() {
        let root = fixture("merge");
        let first = Store::new(&root);
        let second = Store::new(&root);
        for i in (0..20).rev() {
            first
                .mutate(SavedMutation::Add(format!("/home/test/d{i:02}")))
                .unwrap();
        }
        second
            .mutate(SavedMutation::Add("/home/test/other".into()))
            .unwrap();
        let (paths, message) = first
            .mutate(SavedMutation::Add("/home/test/d00".into()))
            .unwrap();
        assert_eq!(message, "Already saved");
        assert_eq!(paths.len(), 21);
        assert_eq!(paths[0], "/home/test/d00");
        first
            .mutate(SavedMutation::Remove("/home/test/d00".into()))
            .unwrap();
        assert_eq!(second.load().unwrap().len(), 20);
        assert_eq!(Store::new(&root).load().unwrap(), second.load().unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn busy_or_corrupt_store_preserves_existing_data() {
        let root = fixture("failure");
        let store = Store::new(&root);
        let lock = store.lock().unwrap();
        assert!(
            store
                .mutate(SavedMutation::Add("/home/test/a".into()))
                .is_err()
        );
        drop(lock);
        fs::write(root.join("saved.json"), b"broken").unwrap();
        assert!(
            store
                .mutate(SavedMutation::Add("/home/test/a".into()))
                .is_err()
        );
        assert_eq!(fs::read(root.join("saved.json")).unwrap(), b"broken");
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_files_are_rejected_without_touching_targets() {
        use std::os::unix::fs::symlink;
        for name in ["saved.json", "saved.lock", "saved.tmp"] {
            let root = fixture(name);
            let sentinel = root.join("sentinel");
            fs::write(&sentinel, b"KEEP").unwrap();
            symlink(&sentinel, root.join(name)).unwrap();
            assert!(
                Store::new(&root)
                    .mutate(SavedMutation::Add("/home/test/a".into()))
                    .is_err()
            );
            assert_eq!(fs::read(&sentinel).unwrap(), b"KEEP");
            fs::remove_dir_all(root).unwrap();
        }
    }
}

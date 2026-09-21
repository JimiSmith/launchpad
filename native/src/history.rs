//! Shared persistent history in the native XDG state directory.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

mod tool {
    use super::*;
    pub fn serialize<S: serde::Serializer>(value: &Tool, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(value.as_str())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Tool, D::Error> {
        let value = String::deserialize(d)?;
        Tool::new(&value).ok_or_else(|| serde::de::Error::custom("invalid command ID"))
    }
}
#[derive(Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    entries: Vec<Entry>,
}
use zellij_launchpad_core::app::Tool;

pub struct Store {
    root: PathBuf,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub path: String,
    #[serde(with = "tool")]
    pub tool: Tool,
    pub opened_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Event {
    version: u32,
    id: String,
    entry: Option<Entry>,
}
pub fn age(opened_at: u64, now: u64) -> String {
    let seconds = now.saturating_sub(opened_at);
    match seconds {
        0..60 => "Just now".into(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    }
}
fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 100
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn valid_entry(entry: &Entry) -> bool {
    valid_token(&entry.id)
        && entry.path.len() <= 4096
        && Path::new(&entry.path).is_absolute()
        && !entry.path.chars().any(char::is_control)
        && !Path::new(&entry.path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}
fn safe_path(path: &Path, directory: bool) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(m) if (directory && m.is_dir()) || (!directory && m.is_file()) => Ok(()),
        Ok(_) => Err("Unsafe history path (symlink or non-regular file)".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
impl Store {
    pub fn new(root: &Path) -> Self {
        Self { root: root.into() }
    }
    pub fn record(&self, entry: Entry) -> Result<(), String> {
        if !valid_entry(&entry) {
            return Err("Invalid history entry".into());
        }
        self.events()?;
        let journal = self.root.join("history.d");
        fs::create_dir_all(&journal).map_err(|e| e.to_string())?;
        self.atomic(
            &journal.join(format!("{}.json", entry.id)),
            &entry.id,
            &serde_json::to_vec(&Event {
                version: 2,
                id: entry.id.clone(),
                entry: Some(entry.clone()),
            })
            .unwrap(),
        )?;
        self.materialize(&entry.id)
    }
    fn cleanup_temps(&self) -> Result<(), String> {
        let now = std::time::SystemTime::now();
        for (n, file) in fs::read_dir(&self.root)
            .map_err(|e| e.to_string())?
            .enumerate()
        {
            if n >= 128 {
                return Err(
                    "History state exceeds 128 files; move the state directory aside to recover"
                        .into(),
                );
            }
            let file = file.map_err(|e| e.to_string())?;
            let name = file.file_name();
            let Some(token) = name
                .to_str()
                .and_then(|s| s.strip_prefix("history-"))
                .and_then(|s| s.strip_suffix(".tmp"))
            else {
                continue;
            };
            if !valid_token(token) {
                continue;
            }
            let path = file.path();
            safe_path(&path, false)?;
            if fs::symlink_metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age.as_secs() > 60)
            {
                // Unlinking a paused writer's private temp can only make its own
                // rename fail; it cannot publish over or unlock another writer.
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }
    fn atomic(&self, target: &Path, token: &str, bytes: &[u8]) -> Result<(), String> {
        self.cleanup_temps()?;
        use std::io::Write;
        let temp = self.root.join(format!("history-{token}.tmp"));
        safe_path(target, false)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temp, target)
        })();
        let _ = fs::remove_file(&temp);
        result.map_err(|e| e.to_string())
    }
    fn materialize(&self, token: &str) -> Result<(), String> {
        let events = self.events()?;
        let kept = Self::select(&events);
        self.atomic(
            &self.root.join("history.json"),
            token,
            &serde_json::to_vec(&Snapshot {
                version: 2,
                entries: kept.iter().filter_map(|e| e.entry.clone()).collect(),
            })
            .unwrap(),
        )?;
        for event in &events {
            if !kept.iter().any(|e| e.id == event.id) {
                match fs::remove_file(
                    self.root
                        .join("history.d")
                        .join(format!("{}.json", event.id)),
                ) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        Ok(())
    }
    pub fn remove(&self, token: &str, operation: &str) -> Result<(), String> {
        if !valid_token(token) || !valid_token(operation) {
            return Err("Invalid history token".into());
        }
        self.events()?;
        match fs::remove_file(self.root.join("history.d").join(format!("{token}.json"))) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
        self.materialize(operation)
    }
    pub fn clear(&self, token: &str) -> Result<(), String> {
        if !valid_token(token) {
            return Err("Invalid history token".into());
        }
        self.events()?;
        let journal = self.root.join("history.d");
        fs::create_dir_all(&journal).map_err(|e| e.to_string())?;
        self.atomic(
            &journal.join(format!("{token}.json")),
            token,
            &serde_json::to_vec(&Event {
                version: 2,
                id: token.into(),
                entry: None,
            })
            .unwrap(),
        )?;
        self.materialize(token)
    }
    pub fn refresh(&self, token: &str) -> Result<Vec<Entry>, String> {
        if !valid_token(token) {
            return Err("Invalid history token".into());
        }
        self.materialize(token)?;
        self.read()
    }
    pub fn read(&self) -> Result<Vec<Entry>, String> {
        Ok(Self::select(&self.events()?)
            .into_iter()
            .filter_map(|e| e.entry)
            .collect())
    }
    fn events(&self) -> Result<Vec<Event>, String> {
        safe_path(&self.root, true)?;
        safe_path(&self.root.join("history.json"), false)?;
        safe_path(&self.root.join("history.d"), true)?;
        let dir = match fs::read_dir(self.root.join("history.d")) {
            Ok(dir) => dir,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let mut entries = Vec::new();

        for (n, file) in dir.enumerate() {
            if n >= 128 {
                return Err(
                    "History journal exceeds 128 files; move the state directory aside to recover"
                        .into(),
                );
            }
            let path = file.map_err(|e| e.to_string())?.path();
            // Another writer may garbage-collect a redundant immutable record.
            safe_path(&path, false)?;
            use std::io::Read;
            let file = match fs::File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.to_string()),
            };
            let mut bytes = Vec::new();
            file.take(32769)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() <= 32768
                && let Ok(event) = serde_json::from_slice::<Event>(&bytes)
                && event.version == 2
                && valid_token(&event.id)
                && path.file_name().and_then(|s| s.to_str()) == Some(&format!("{}.json", event.id))
                && event
                    .entry
                    .as_ref()
                    .is_none_or(|entry| entry.id == event.id && valid_entry(entry))
            {
                entries.push(event);
            }
        }
        Ok(entries)
    }
    fn select(events: &[Event]) -> Vec<Event> {
        let mut events = events.to_vec();
        events.sort_by(|a, b| b.id.cmp(&a.id));
        let mut kept = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for event in events {
            if let Some(entry) = &event.entry {
                if seen.len() < 10 && seen.insert(entry.path.clone()) {
                    kept.push(event);
                }
            } else {
                kept.push(event);
                break;
            }
        }
        kept
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> Store {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/history-persistence/unit")
            .join(format!("{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Store::new(&root)
    }
    fn entry(n: u64, path: &str, tool: Tool) -> Entry {
        Entry {
            id: format!("{n:020}-test"),
            path: path.into(),
            tool,
            opened_at: n,
        }
    }
    #[test]
    fn arbitrary_stable_command_identity_roundtrips_without_executable_data() {
        let store = fixture("configured-id");
        let row = entry(1, "/fixture/a", Tool::new("Variant_2.worktree").unwrap());
        store.record(row.clone()).unwrap();
        assert_eq!(Store::new(&store.root).read().unwrap(), vec![row]);
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.root.join("history.json")).unwrap()).unwrap();
        assert_eq!(json["version"], 2);
        assert_eq!(json["entries"][0]["tool"], "Variant_2.worktree");
        assert!(json["entries"][0].get("executable").is_none());
    }
    #[test]
    fn unsupported_history_is_ignored_and_current_writes_work() {
        for version in [1, 99] {
            for journal in [false, true] {
                let store = fixture(&format!("unsupported-{version}-{journal}"));
                let old = entry(1, "/fixture/old", Tool::new("Hermes").unwrap());
                fs::write(
                    store.root.join("history.json"),
                    serde_json::to_vec(&Snapshot {
                        version,
                        entries: vec![old.clone()],
                    })
                    .unwrap(),
                )
                .unwrap();
                if journal {
                    fs::create_dir(store.root.join("history.d")).unwrap();
                    fs::write(
                        store
                            .root
                            .join("history.d")
                            .join(format!("{}.json", old.id)),
                        serde_json::to_vec(&Event {
                            version,
                            id: old.id.clone(),
                            entry: Some(old.clone()),
                        })
                        .unwrap(),
                    )
                    .unwrap();
                    fs::write(
                        store.root.join("history.d/zz-unsupported.json"),
                        serde_json::to_vec(&Event {
                            version,
                            id: "zz-unsupported".into(),
                            entry: None,
                        })
                        .unwrap(),
                    )
                    .unwrap();
                }
                assert!(
                    store.refresh("ignore-unsupported").unwrap().is_empty(),
                    "version {version}, journal {journal}"
                );
                let current = entry(2, "/fixture/new", Tool::new("Hermes").unwrap());
                store.record(current.clone()).unwrap();
                assert_eq!(Store::new(&store.root).read().unwrap(), vec![current]);
                let projection: Snapshot =
                    serde_json::from_slice(&fs::read(store.root.join("history.json")).unwrap())
                        .unwrap();
                assert_eq!(projection.version, 2);
                assert_eq!(projection.entries.len(), 1);
                assert_eq!(projection.entries[0].tool.as_str(), "Hermes");
            }
        }
    }
    #[test]
    fn abandoned_temp_is_reclaimed_without_touching_an_active_writer() {
        let store = fixture("orphan-temp");
        let stale = store.root.join("history-abandoned.tmp");
        let active = store.root.join("history-active.tmp");
        fs::write(&stale, "partial").unwrap();
        fs::write(&active, "live").unwrap();
        fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        store.record(entry(1, "/fixture/a", Tool::Shell)).unwrap();
        assert!(
            !stale.exists(),
            "crash leftovers should be bounded/recoverable"
        );
        assert_eq!(fs::read_to_string(active).unwrap(), "live");
    }
    #[test]
    fn refresh_repairs_a_missing_or_stale_projection_from_the_journal() {
        let store = fixture("repair");
        let row = entry(1, "/fixture/a", Tool::Shell);
        store.record(row.clone()).unwrap();
        fs::write(
            store.root.join("history.json"),
            br#"{"version":1,"entries":[]}"#,
        )
        .unwrap();
        assert_eq!(store.refresh("repair1").unwrap(), vec![row.clone()]);
        let json: Snapshot =
            serde_json::from_slice(&fs::read(store.root.join("history.json")).unwrap()).unwrap();
        assert_eq!(json.entries, vec![row.clone()]);
        fs::remove_file(store.root.join("history.json")).unwrap();
        assert_eq!(store.refresh("repair2").unwrap(), vec![row]);
        assert!(store.root.join("history.json").is_file());
    }
    #[cfg(unix)]
    #[test]
    fn unreadable_record_fails_refresh_without_replacing_good_projection() {
        use std::os::unix::fs::PermissionsExt;
        let store = fixture("unreadable-record");
        let row = entry(1, "/fixture/a", Tool::new("hermes").unwrap());
        store.record(row.clone()).unwrap();
        let projection = store.root.join("history.json");
        let saved = fs::read(&projection).unwrap();
        let record = store
            .root
            .join("history.d")
            .join(format!("{}.json", row.id));
        let permissions = fs::metadata(&record).unwrap().permissions();
        fs::set_permissions(&record, fs::Permissions::from_mode(0o000)).unwrap();
        let open_error = fs::File::open(&record).err();
        let result = store.refresh("unreadable");
        let after = fs::read(&projection);
        fs::set_permissions(&record, permissions).unwrap();

        assert_eq!(
            open_error.unwrap().kind(),
            std::io::ErrorKind::PermissionDenied,
            "fixture must produce an actual open failure (run as an unprivileged user)"
        );
        assert!(
            result.is_err(),
            "unreadable authority must not become empty history: {result:?}"
        );
        assert_eq!(
            after.unwrap(),
            saved,
            "failed scan must not replace the good projection"
        );
        assert_eq!(store.refresh("restored").unwrap(), vec![row]);
        assert_eq!(fs::read(projection).unwrap(), saved);
    }
    #[cfg(unix)]
    #[test]
    fn prune_failure_is_reported_without_losing_authoritative_records() {
        use std::os::unix::fs::PermissionsExt;
        let store = fixture("prune-denied");
        let newest = entry(3, "/fixture/a", Tool::new("hermes").unwrap());
        let other = entry(2, "/fixture/b", Tool::Shell);
        store.record(other.clone()).unwrap();
        store.record(newest.clone()).unwrap();
        let journal = store.root.join("history.d");
        let old = entry(1, "/fixture/a", Tool::Shell);
        let obsolete = journal.join(format!("{}.json", old.id));
        // A published record left behind before another writer's compaction.
        fs::write(
            &obsolete,
            serde_json::to_vec(&Event {
                version: 2,
                id: old.id.clone(),
                entry: Some(old),
            })
            .unwrap(),
        )
        .unwrap();
        let saved: Vec<_> = fs::read_dir(&journal)
            .unwrap()
            .map(|file| {
                let path = file.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        let permissions = fs::metadata(&journal).unwrap().permissions();
        fs::set_permissions(&journal, fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.refresh("prune-denied");
        fs::set_permissions(&journal, permissions).unwrap();

        assert!(
            result.is_err(),
            "failed pruning must not report success: {result:?}"
        );
        for (path, bytes) in saved {
            assert_eq!(fs::read(path).unwrap(), bytes, "authority survives failure");
        }
        assert_eq!(
            store.refresh("prune-restored").unwrap(),
            vec![newest, other]
        );
        assert!(
            !obsolete.exists(),
            "restored permissions allow pending compaction"
        );
        assert_eq!(fs::read_dir(journal).unwrap().count(), 2);
    }
    #[test]
    fn ages_use_persisted_time_instead_of_resetting_when_reopened() {
        assert_eq!(age(100, 100), "Just now");
        assert_eq!(age(100, 220), "2m ago");
        assert_eq!(age(100, 7300), "2h ago");
        assert_eq!(age(100, 172900), "2d ago");
        assert_eq!(age(500, 100), "Just now");
    }
    #[cfg(unix)]
    #[test]
    fn symlink_history_journal_records_and_temp_are_rejected_without_touching_targets() {
        use std::os::unix::fs::symlink;
        for kind in ["history", "journal", "record", "temp"] {
            let store = fixture(&format!("symlink-{kind}"));
            let sentinel = store.root.join("sentinel");
            fs::write(&sentinel, "KEEP").unwrap();
            let row = entry(1, "/fixture/a", Tool::Shell);
            let journal = store.root.join("history.d");
            fs::create_dir(&journal).unwrap();
            let link = match kind {
                "history" => store.root.join("history.json"),
                "journal" => {
                    fs::remove_dir(&journal).unwrap();
                    journal
                }
                "record" => journal.join(format!("{}.json", row.id)),
                _ => store.root.join(format!("history-{}.tmp", row.id)),
            };
            symlink(&sentinel, &link).unwrap();
            assert!(store.record(row).is_err(), "reject {kind}");
            assert_eq!(fs::read_to_string(&sentinel).unwrap(), "KEEP");
            assert!(
                fs::symlink_metadata(link).unwrap().is_symlink(),
                "do not replace/remove unsafe {kind}"
            );
        }
    }
    #[test]
    fn untrusted_records_are_bounded_versioned_and_cannot_escape_cache() {
        let store = fixture("untrusted");
        for path in [
            "relative".into(),
            "/fixture/../escape".into(),
            format!("/{}", "a".repeat(4096)),
            "/fixture/\ncontrol".into(),
        ] {
            assert!(
                store.record(entry(1, &path, Tool::Shell)).is_err(),
                "reject {path:?}"
            );
        }
        let mut bad = entry(1, "/fixture/okay", Tool::Shell);
        bad.id = "../escape".into();
        assert!(store.record(bad).is_err());
        store
            .record(entry(2, "/fixture/okay", Tool::Shell))
            .unwrap();
        let journal = store.root.join("history.d");
        for (name, bytes) in [
            ("bad.json", b"{".to_vec()),
            ("big.json", vec![b'x'; 33000]),
            (
                "future.json",
                br#"{"version":99,"id":"future","entry":null}"#.to_vec(),
            ),
        ] {
            fs::write(journal.join(name), bytes).unwrap();
        }
        fs::write(store.root.join("history.json"), b"{ corrupt projection").unwrap();
        assert_eq!(
            store.read().unwrap(),
            vec![entry(2, "/fixture/okay", Tool::Shell)]
        );
        for n in 0..130 {
            fs::write(journal.join(format!("junk{n}")), b"").unwrap();
        }
        assert!(
            store.read().is_err(),
            "scan must have a finite entry budget"
        );
    }
    #[test]
    fn journal_stays_bounded_after_repeated_launches_and_clears() {
        let store = fixture("journal-bound");
        for n in 1..=200 {
            if n % 17 == 0 {
                store.clear(&entry(n, "", Tool::Shell).id).unwrap();
            } else {
                store
                    .record(entry(n, &format!("/fixture/{}", n % 13), Tool::Shell))
                    .unwrap();
            }
            assert!(fs::read_dir(store.root.join("history.d")).unwrap().count() <= 11);
        }
    }
    #[test]
    fn rejection_removes_only_its_exact_attempt_not_concurrent_updates() {
        let store = fixture("rollback");
        let rejected = entry(1, "/fixture/a", Tool::new("claude").unwrap());
        store.record(rejected.clone()).unwrap();
        store.remove(&rejected.id, "rollback1").unwrap();
        assert!(store.read().unwrap().is_empty());
        store
            .record(entry(2, "/fixture/a", Tool::new("claude").unwrap()))
            .unwrap();
        let newer = entry(3, "/fixture/a", Tool::new("codex").unwrap());
        let other = entry(4, "/fixture/b", Tool::Shell);
        store.record(newer.clone()).unwrap();
        store.record(other.clone()).unwrap();
        store
            .remove(&entry(2, "", Tool::Shell).id, "rollback2")
            .unwrap();
        assert_eq!(store.read().unwrap(), vec![other, newer]);
    }
    #[test]
    fn clear_persists_and_does_not_erase_a_newer_concurrent_launch() {
        let store = fixture("clear");
        store.record(entry(1, "/fixture/old", Tool::Shell)).unwrap();
        let newer = entry(3, "/fixture/new", Tool::new("hermes").unwrap());
        store.record(newer.clone()).unwrap();
        store.clear(&entry(2, "", Tool::Shell).id).unwrap();
        assert_eq!(store.read().unwrap(), vec![newer]);
        store.clear(&entry(4, "", Tool::Shell).id).unwrap();
        assert!(Store::new(&store.root).read().unwrap().is_empty());
    }
    #[test]
    fn concurrent_writers_merge_without_a_lock_or_lost_directories() {
        let store = fixture("concurrent");
        for round in 0..20 {
            let gate = std::sync::Barrier::new(8);
            std::thread::scope(|scope| {
                for n in 1..=8 {
                    let store = &store;
                    let gate = &gate;
                    scope.spawn(move || {
                        gate.wait();
                        store
                            .record(entry(
                                round * 8 + n,
                                &format!("/fixture/{n}"),
                                Tool::new("claude").unwrap(),
                            ))
                            .unwrap();
                    });
                }
            });
            let rows = store.read().unwrap();
            assert_eq!(
                rows.len(),
                8,
                "all independent concurrent directories survive"
            );
        }
    }
    #[test]
    fn ten_unique_directories_keep_the_latest_tool_and_order() {
        let store = fixture("bounded");
        for n in 1..=12 {
            store
                .record(entry(n, &format!("/fixture/{n}"), Tool::Shell))
                .unwrap();
        }
        store
            .record(entry(13, "/fixture/8", Tool::new("codex").unwrap()))
            .unwrap();
        let rows = store.read().unwrap();
        assert_eq!(rows.len(), 10);
        assert_eq!(
            rows[0],
            entry(13, "/fixture/8", Tool::new("codex").unwrap())
        );
        assert_eq!(
            rows.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec![
                "/fixture/8",
                "/fixture/12",
                "/fixture/11",
                "/fixture/10",
                "/fixture/9",
                "/fixture/7",
                "/fixture/6",
                "/fixture/5",
                "/fixture/4",
                "/fixture/3"
            ]
        );
    }
    #[test]
    fn real_store_survives_a_new_reader_with_timestamp_and_tool() {
        let store = fixture("reopen");
        assert!(store.read().unwrap().is_empty());
        let expected = entry(
            100,
            "/fixture/修理 it's literal; $HOME",
            Tool::new("hermes").unwrap(),
        );
        store.record(expected.clone()).unwrap();
        assert_eq!(Store::new(&store.root).read().unwrap(), vec![expected]);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(store.root.join("history.json")).unwrap())
                .unwrap();
        assert_eq!(json["version"], 2);
        assert_eq!(json["entries"][0]["opened_at"], 100);
    }
}

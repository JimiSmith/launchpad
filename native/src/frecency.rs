//! Shared launch times for search frecency, in the native XDG state directory.
//! The journal uses the history protocol: immutable records, rename, no lock.
use crate::history::{concurrent_io, publish, safe_path, valid_path, valid_token};
use launchpad_core::frecency::{MAX_AGE_SECONDS, MAX_LAUNCHES};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: u32 = 1;
const JOURNAL: &str = "frecency.d";
/// A compacted record holds every kept launch, so it is larger than a history record.
const MAX_RECORD_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECORDS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// The operation ID of the launch. The same launch in two records counts once.
    pub id: String,
    pub path: String,
    /// Unix seconds.
    pub at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Body {
    /// One new launch, or a compaction of the launches that a refresh kept.
    Launches { launches: Vec<Launch> },
    /// Removes all launches with a lower operation ID.
    Clear,
}

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    version: u32,
    id: String,
    #[serde(flatten)]
    body: Body,
}

pub struct Store {
    root: PathBuf,
}

pub struct Refreshed {
    pub launches: Vec<Launch>,
    /// An error from the removal of replaced records.
    pub warning: Option<String>,
}

impl Store {
    pub fn new(root: &Path) -> Self {
        Self { root: root.into() }
    }

    /// Publishes one launch. It does not compact, so that the launch path stays short.
    pub fn record(&self, launch: Launch) -> Result<(), String> {
        if !valid_token(&launch.id) || !valid_path(&launch.path) {
            return Err("Invalid frecency launch".into());
        }
        let id = launch.id.clone();
        self.write(Record {
            version: VERSION,
            id,
            body: Body::Launches {
                launches: vec![launch],
            },
        })
    }

    /// Removes the record of one rejected launch. A refresh that already compacted
    /// the launch keeps it.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        if !valid_token(id) {
            return Err("Invalid frecency token".into());
        }
        safe_path(&self.root, true)?;
        safe_path(&self.journal(), true)?;
        let record = self.journal().join(format!("{id}.json"));
        safe_path(&record, false)?;
        self.unlink(id)
    }

    /// Removes the launches that are older than this operation ID.
    pub fn clear(&self, token: &str) -> Result<(), String> {
        if !valid_token(token) {
            return Err("Invalid frecency token".into());
        }
        self.write(Record {
            version: VERSION,
            id: token.into(),
            body: Body::Clear,
        })
    }

    /// Compacts the kept launches into record `token` and returns them.
    pub fn refresh(&self, token: &str, now: u64) -> Result<Refreshed, String> {
        if !valid_token(token) {
            return Err("Invalid frecency token".into());
        }
        let records = self.records()?;
        let kept = select(&records, now);
        let clear = latest_clear(&records);
        let obsolete: Vec<_> = records
            .iter()
            .filter(|r| Some(&r.id) != clear)
            .map(|r| r.id.clone())
            .collect();
        let compacted = match records.iter().find(|r| Some(&r.id) != clear) {
            Some(Record {
                body: Body::Launches { launches },
                ..
            }) => obsolete.len() == 1 && *launches == kept,
            _ => obsolete.is_empty(),
        };
        if compacted {
            return Ok(Refreshed {
                launches: kept,
                warning: None,
            });
        }
        if !kept.is_empty() {
            self.write(Record {
                version: VERSION,
                id: token.into(),
                body: Body::Launches {
                    launches: kept.clone(),
                },
            })?;
        }
        // Only observed records are removed. A concurrent launch stays in its own record.
        // The compaction is already published, so a failed removal does not lose the launches.
        let warning = obsolete
            .iter()
            .filter(|id| *id != token)
            .find_map(|id| self.unlink(id).err());
        Ok(Refreshed {
            launches: kept,
            warning,
        })
    }

    fn journal(&self) -> PathBuf {
        self.root.join(JOURNAL)
    }

    fn write(&self, record: Record) -> Result<(), String> {
        safe_path(&self.root, true)?;
        safe_path(&self.journal(), true)?;
        fs::create_dir_all(self.journal()).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(&record).unwrap();
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err("Frecency record exceeds 8 MiB".into());
        }
        publish(
            &self.root,
            "frecency",
            &self.journal().join(format!("{}.json", record.id)),
            &record.id,
            &bytes,
        )
    }

    fn unlink(&self, id: &str) -> Result<(), String> {
        match concurrent_io(|| fs::remove_file(self.journal().join(format!("{id}.json")))) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Remove frecency record {id}: {e}")),
        }
    }

    fn records(&self) -> Result<Vec<Record>, String> {
        safe_path(&self.root, true)?;
        safe_path(&self.journal(), true)?;
        let dir = match fs::read_dir(self.journal()) {
            Ok(dir) => dir,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let mut records = Vec::new();
        for (n, file) in dir.enumerate() {
            if n >= MAX_RECORDS {
                return Err(format!(
                    "Frecency journal exceeds {MAX_RECORDS} files; move {} aside to recover",
                    self.journal().display()
                ));
            }
            let path = file.map_err(|e| e.to_string())?.path();
            safe_path(&path, false)?;
            use std::io::Read;
            // Another writer can remove a record that its compaction replaced.
            let file = match concurrent_io(|| fs::File::open(&path)) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("Read {}: {e}", path.display())),
            };
            let mut bytes = Vec::new();
            file.take(MAX_RECORD_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            // Unknown and corrupt files stay in place for diagnosis and are ignored.
            if bytes.len() as u64 <= MAX_RECORD_BYTES
                && let Ok(record) = serde_json::from_slice::<Record>(&bytes)
                && record.version == VERSION
                && valid_token(&record.id)
                && path.file_name().and_then(|s| s.to_str()) == Some(&format!("{}.json", record.id))
                && valid_body(&record.body)
            {
                records.push(record);
            }
        }
        Ok(records)
    }
}

fn valid_body(body: &Body) -> bool {
    match body {
        Body::Launches { launches } => launches
            .iter()
            .all(|l| valid_token(&l.id) && valid_path(&l.path)),
        Body::Clear => true,
    }
}

fn latest_clear(records: &[Record]) -> Option<&String> {
    records
        .iter()
        .filter(|r| matches!(r.body, Body::Clear))
        .map(|r| &r.id)
        .max()
}

/// Launches after the latest clear, of the last 30 days, at most 128 for each directory.
fn select(records: &[Record], now: u64) -> Vec<Launch> {
    let clear = latest_clear(records);
    let cutoff = now.saturating_sub(MAX_AGE_SECONDS);
    let mut seen = HashSet::new();
    // Sort by directory, then newest first, so that the cap keeps the newest launches.
    let mut launches: Vec<Launch> = records
        .iter()
        .filter_map(|r| match &r.body {
            Body::Launches { launches } => Some(launches),
            Body::Clear => None,
        })
        .flatten()
        .filter(|l| l.at >= cutoff && clear.is_none_or(|c| l.id > *c))
        .filter(|l| seen.insert(l.id.clone()))
        .cloned()
        .collect();
    launches.sort_by(|a, b| a.path.cmp(&b.path).then(b.id.cmp(&a.id)));
    let mut count: HashMap<String, usize> = HashMap::new();
    launches.retain(|l| {
        let n = count.entry(l.path.clone()).or_default();
        *n += 1;
        *n <= MAX_LAUNCHES
    });
    launches
}

#[cfg(test)]
mod tests {
    use super::*;
    const DAY: u64 = 86_400;
    const NOW: u64 = 1_000 * DAY;

    fn fixture(name: &str) -> Store {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/frecency-persistence/unit")
            .join(format!("{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Store::new(&root)
    }
    fn launch(n: u64, path: &str, at: u64) -> Launch {
        Launch {
            id: format!("{n:020}-test"),
            path: path.into(),
            at,
        }
    }
    fn token(n: u64) -> String {
        format!("{n:020}-refresh")
    }
    fn files(store: &Store) -> usize {
        fs::read_dir(store.journal()).unwrap().count()
    }

    #[test]
    fn launches_roundtrip_and_compact_into_one_record() {
        let store = fixture("roundtrip");
        let rows = [
            launch(1, "/fixture/a", NOW - DAY),
            launch(2, "/fixture/b", NOW),
            launch(3, "/fixture/a", NOW),
        ];
        for row in &rows {
            store.record(row.clone()).unwrap();
        }
        let expected = vec![rows[2].clone(), rows[0].clone(), rows[1].clone()];
        assert_eq!(store.refresh(&token(4), NOW).unwrap().launches, expected);
        assert_eq!(files(&store), 1);
        let reopened = Store::new(&store.root);
        assert_eq!(reopened.refresh(&token(5), NOW).unwrap().launches, expected);
        assert!(
            store.journal().join(format!("{}.json", token(4))).is_file(),
            "an unchanged journal is not written again"
        );
    }

    #[test]
    fn launches_older_than_30_days_and_their_directories_are_removed() {
        let store = fixture("cutoff");
        store
            .record(launch(1, "/fixture/old", NOW - 30 * DAY - 1))
            .unwrap();
        store
            .record(launch(2, "/fixture/a", NOW - 30 * DAY))
            .unwrap();
        store.record(launch(3, "/fixture/a", NOW)).unwrap();
        assert_eq!(
            store.refresh(&token(4), NOW).unwrap().launches,
            [
                launch(3, "/fixture/a", NOW),
                launch(2, "/fixture/a", NOW - 30 * DAY)
            ]
        );
        assert_eq!(
            store.refresh(&token(5), NOW + 1).unwrap().launches,
            [launch(3, "/fixture/a", NOW)]
        );
        assert!(
            store
                .refresh(&token(6), NOW + 31 * DAY)
                .unwrap()
                .launches
                .is_empty()
        );
        assert_eq!(files(&store), 0, "no record is kept for no launches");
    }

    #[test]
    fn each_directory_keeps_its_newest_128_launches() {
        let store = fixture("cap");
        for n in 1..=130 {
            store.record(launch(n, "/fixture/a", NOW)).unwrap();
        }
        store.record(launch(131, "/fixture/b", NOW)).unwrap();
        let kept = store.refresh(&token(200), NOW).unwrap().launches;
        let a: Vec<_> = kept.iter().filter(|l| l.path == "/fixture/a").collect();
        assert_eq!(a.len(), 128);
        assert_eq!(a[0].id, launch(130, "", 0).id);
        assert_eq!(a[127].id, launch(3, "", 0).id);
        assert_eq!(kept.iter().filter(|l| l.path == "/fixture/b").count(), 1);
        assert_eq!(files(&store), 1);
    }

    #[test]
    fn clear_removes_older_launches_but_not_a_newer_concurrent_launch() {
        let store = fixture("clear");
        store.record(launch(1, "/fixture/old", NOW)).unwrap();
        store.refresh(&token(2), NOW).unwrap();
        store
            .record(launch(3, "/fixture/older-pending", NOW))
            .unwrap();
        store.record(launch(5, "/fixture/new", NOW)).unwrap();
        store.clear(&launch(4, "", 0).id).unwrap();
        assert_eq!(
            store.refresh(&token(6), NOW).unwrap().launches,
            [launch(5, "/fixture/new", NOW)]
        );
        assert_eq!(files(&store), 2, "the compaction and the latest clear");
        store.clear(&launch(7, "", 0).id).unwrap();
        assert!(
            Store::new(&store.root)
                .refresh(&token(8), NOW)
                .unwrap()
                .launches
                .is_empty()
        );
        // A launch that a paused writer publishes late, with an ID before the clear.
        store.record(launch(6, "/fixture/late", NOW)).unwrap();
        assert!(store.refresh(&token(9), NOW).unwrap().launches.is_empty());
        assert_eq!(files(&store), 1, "only the latest clear");
    }

    #[test]
    fn remove_rolls_back_only_its_own_launch() {
        let store = fixture("remove");
        store.record(launch(1, "/fixture/a", NOW)).unwrap();
        store.record(launch(2, "/fixture/a", NOW)).unwrap();
        store.remove(&launch(2, "", 0).id).unwrap();
        store.remove(&launch(9, "", 0).id).unwrap();
        assert_eq!(
            store.refresh(&token(3), NOW).unwrap().launches,
            [launch(1, "/fixture/a", NOW)]
        );
    }

    #[test]
    fn concurrent_writers_and_compactions_lose_no_launches() {
        let store = fixture("concurrent");
        for round in 0..10u64 {
            let gate = std::sync::Barrier::new(8);
            std::thread::scope(|scope| {
                for n in 1..=8u64 {
                    let store = &store;
                    let gate = &gate;
                    scope.spawn(move || {
                        gate.wait();
                        let id = round * 100 + n * 10;
                        store
                            .record(launch(id, &format!("/fixture/{n}"), NOW))
                            .unwrap();
                        // Half of the writers compact while the others publish.
                        if n % 2 == 0 {
                            store.refresh(&token(id + 1), NOW).unwrap();
                        }
                    });
                }
            });
            let kept = store
                .refresh(&token(round * 100 + 99), NOW)
                .unwrap()
                .launches;
            assert_eq!(kept.len() as u64, (round + 1) * 8, "round {round}");
        }
        assert_eq!(files(&store), 1);
    }

    #[test]
    fn a_concurrent_clear_removes_only_older_launches() {
        let store = fixture("concurrent-clear");
        let clear = launch(500, "", 0).id;
        let gate = std::sync::Barrier::new(9);
        std::thread::scope(|scope| {
            for n in 1..=8u64 {
                let (store, gate) = (&store, &gate);
                scope.spawn(move || {
                    gate.wait();
                    // Each writer publishes one launch before and one after the clear ID.
                    for id in [n * 10, 900 + n * 10] {
                        store
                            .record(launch(id, &format!("/fixture/{n}"), NOW))
                            .unwrap();
                        store.refresh(&token(id + 1), NOW).unwrap();
                    }
                });
            }
            let (store, gate) = (&store, &gate);
            scope.spawn(move || {
                gate.wait();
                store.clear(&clear).unwrap();
            });
        });
        let mut ids: Vec<_> = store
            .refresh(&token(999), NOW)
            .unwrap()
            .launches
            .into_iter()
            .map(|l| l.id)
            .collect();
        ids.sort();
        let expected: Vec<_> = (1..=8).map(|n| launch(900 + n * 10, "", 0).id).collect();
        assert_eq!(ids, expected);
    }

    #[test]
    fn invalid_and_unknown_records_are_rejected_or_ignored() {
        let store = fixture("untrusted");
        for path in ["relative", "/fixture/../escape", "/fixture/\ncontrol"] {
            assert!(store.record(launch(1, path, NOW)).is_err(), "{path:?}");
        }
        let mut bad = launch(1, "/fixture/a", NOW);
        bad.id = "../escape".into();
        assert!(store.record(bad).is_err());
        store.record(launch(2, "/fixture/a", NOW)).unwrap();
        for (name, bytes) in [
            ("bad.json", b"{".to_vec()),
            (
                "future.json",
                br#"{"version":99,"id":"future","kind":"clear"}"#.to_vec(),
            ),
            (
                "renamed.json",
                br#"{"version":1,"id":"other","kind":"clear"}"#.to_vec(),
            ),
        ] {
            fs::write(store.journal().join(name), bytes).unwrap();
        }
        assert_eq!(
            store.refresh(&token(3), NOW).unwrap().launches,
            [launch(2, "/fixture/a", NOW)]
        );
        assert_eq!(files(&store), 4, "unknown files stay for diagnosis");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_journal_is_rejected_without_touching_the_target() {
        let store = fixture("symlink");
        let target = store.root.join("target");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, store.journal()).unwrap();
        assert!(store.record(launch(1, "/fixture/a", NOW)).is_err());
        assert!(store.refresh(&token(2), NOW).is_err());
        assert_eq!(fs::read_dir(target).unwrap().count(), 0);
    }
}

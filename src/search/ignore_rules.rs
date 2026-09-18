//! HOME-local ignore loading for the serial ignore walker.
//!
//! ignore 0.4.33's automatic Git discovery opens parent ignore files even with
//! parents(false). Disable that discovery and use its parser/matcher here. This
//! stack holds only rules for the current DFS ancestry, never skipped paths.
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Level {
    path: PathBuf,
    ignore: Gitignore,
    git: Gitignore,
}

pub(super) struct Rules {
    levels: Vec<Level>,
    budget: usize,
    bytes: Arc<AtomicUsize>,
}

impl Rules {
    pub fn new(root: &Path, budget: usize, bytes: Arc<AtomicUsize>) -> Self {
        let mut rules = Self {
            levels: Vec::new(),
            budget,
            bytes,
        };
        rules.push(root);
        rules
    }

    pub fn allows(&mut self, entry: &ignore::DirEntry) -> bool {
        let path = entry.path();
        while self
            .levels
            .last()
            .is_some_and(|level| !path.starts_with(&level.path))
        {
            self.levels.pop();
        }
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        // .ignore has precedence over .gitignore, even across directory levels.
        // Within each type, the nearest matching rule wins, including negation.
        for git in [false, true] {
            for level in self.levels.iter().rev() {
                let rules = if git { &level.git } else { &level.ignore };
                let matched = rules.matched(path, is_dir);
                if !matched.is_none() {
                    if matched.is_ignore() {
                        return false;
                    }
                    if is_dir {
                        self.push(path);
                    }
                    return true;
                }
            }
        }
        if is_dir {
            self.push(path);
        }
        true
    }

    fn push(&mut self, path: &Path) {
        self.levels.push(Level {
            path: path.to_owned(),
            ignore: load(path, ".ignore", self.budget, &self.bytes),
            git: load(path, ".gitignore", self.budget, &self.bytes),
        });
    }
}

fn load(directory: &Path, name: &str, budget: usize, bytes: &AtomicUsize) -> Gitignore {
    let path = directory.join(name);
    // Do not follow symlinked configs or open FIFOs/devices. Like validation,
    // this check is not an atomic defence against hostile concurrent replacement.
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return Gitignore::empty();
    };
    if !metadata.is_file() {
        return Gitignore::empty();
    }
    let remaining = budget.saturating_sub(bytes.load(Ordering::Relaxed));
    let cap = (remaining / 16).min(64 * 1024);
    if metadata.len() > cap as u64 {
        bytes.store(usize::MAX, Ordering::Relaxed);
        return Gitignore::empty();
    }
    let mut source = String::new();
    if fs::File::open(&path)
        .and_then(|file| file.take(cap as u64 + 1).read_to_string(&mut source))
        .is_err()
    {
        return Gitignore::empty();
    }
    // Conservative cumulative allowance for source, compiled globs and scratch.
    // This is not an RSS guarantee; stop visibly instead of silently losing rules.
    let cost = source
        .len()
        .saturating_mul(16)
        .saturating_add(source.lines().count().saturating_mul(2048));
    if source.len() > cap || cost > remaining {
        bytes.store(usize::MAX, Ordering::Relaxed);
        return Gitignore::empty();
    }
    bytes.fetch_add(cost, Ordering::Relaxed);
    let mut builder = GitignoreBuilder::new(directory);
    for line in source.trim_start_matches('\u{feff}').lines() {
        let _ = builder.add_line(Some(path.clone()), line);
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

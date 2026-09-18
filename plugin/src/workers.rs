//! Bounded, serial worker protocol. State lives inside the worker instance.
use serde::{Deserialize, Serialize};
use zellij_launchpad_prototype::{editor::MAX_INPUT_CHARS, search::HomeIndex};
#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Start {
        epoch: u64,
        home: String,
    },
    Step {
        epoch: u64,
    },
    Query {
        epoch: u64,
        generation: u64,
        text: String,
    },
    Validate {
        epoch: u64,
        generation: u64,
        raw: String,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    Ready {
        epoch: u64,
        cwd: String,
    },
    Progress {
        epoch: u64,
        revision: u64,
        status: String,
        scanning: bool,
    },
    Failed {
        epoch: u64,
        error: String,
    },
    Ignored,
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
}
#[derive(Default, Serialize, Deserialize)]
pub struct Engine {
    epoch: u64,
    #[serde(skip)]
    index: Option<HomeIndex>,
    #[serde(skip)]
    continuation: Option<u64>,
    #[serde(skip)]
    last_progress: Option<std::time::Instant>,
}
impl Engine {
    /// One queued continuation survives refresh; when consumed it wakes the current epoch.
    /// The host's serial worker queue can therefore interleave queries and validation.
    pub fn dispatch(
        &mut self,
        request: Request,
        initial_cwd: &str,
    ) -> (Option<Reply>, Option<Request>) {
        self.dispatch_at(request, initial_cwd, std::time::Instant::now())
    }
    fn dispatch_at(
        &mut self,
        request: Request,
        initial_cwd: &str,
        now: std::time::Instant,
    ) -> (Option<Reply>, Option<Request>) {
        if let Request::Step { epoch } = &request {
            if self.continuation != Some(*epoch) {
                return (None, None);
            }
            self.continuation = None;
        }
        let reply = self.handle(request, initial_cwd);
        let next = if self.continuation.is_none()
            && self.index.as_ref().is_some_and(HomeIndex::is_scanning)
        {
            self.continuation = Some(self.epoch);
            Some(Request::Step { epoch: self.epoch })
        } else {
            None
        };
        let reply = match reply {
            Reply::Ready { .. } => {
                self.last_progress = Some(now);
                Some(reply)
            }
            Reply::Progress { scanning, .. } => {
                if !scanning
                    || self.last_progress.is_none_or(|last| {
                        now.duration_since(last) >= std::time::Duration::from_millis(100)
                    })
                {
                    self.last_progress = Some(now);
                    Some(reply)
                } else {
                    None
                }
            }
            Reply::Ignored => None,
            _ => Some(reply),
        };
        (reply, next)
    }
    pub fn handle(&mut self, request: Request, initial_cwd: &str) -> Reply {
        match request {
            Request::Start { epoch, home } => {
                self.epoch = epoch;
                self.index = None;
                if initial_cwd != home {
                    return Reply::Failed {
                        epoch,
                        error: "Worker HOME mapping disagrees. Reopen the plugin.".into(),
                    };
                }
                match HomeIndex::new(home.into(), "/host".into()) {
                    Ok(index) => {
                        self.index = Some(index);
                        Reply::Ready {
                            epoch,
                            cwd: initial_cwd.into(),
                        }
                    }
                    Err(error) => Reply::Failed { epoch, error },
                }
            }
            Request::Step { epoch } if epoch == self.epoch => {
                let Some(index) = &mut self.index else {
                    return Reply::Ignored;
                };
                index.step(128);
                Reply::Progress {
                    epoch,
                    revision: index.dirs.len() as u64,
                    status: index.status(),
                    scanning: index.is_scanning(),
                }
            }
            Request::Query {
                epoch,
                generation,
                text,
            } if epoch == self.epoch => {
                let Some(index) = &self.index else {
                    return Reply::Ignored;
                };
                let home = index.home.to_str().unwrap();
                let mut bytes = 0;
                let paths = if text.chars().count() > MAX_INPUT_CHARS {
                    Vec::new()
                } else {
                    zellij_launchpad_prototype::search::matches_in(&text, &index.dirs, home, home)
                        .into_iter()
                        .take(100)
                        .map(|i| &index.dirs[i].path)
                        .take_while(|path| {
                            bytes += path.len();
                            bytes <= 64 * 1024
                        })
                        .cloned()
                        .collect()
                };
                Reply::Results {
                    epoch,
                    generation,
                    revision: index.dirs.len() as u64,
                    paths,
                }
            }
            Request::Validate {
                epoch,
                generation,
                raw,
            } if epoch == self.epoch => {
                let Some(index) = &self.index else {
                    return Reply::Ignored;
                };
                let result = if raw.len() > 4096 {
                    Err("Path exceeds 4096 bytes.".into())
                } else {
                    index.validate(&raw)
                };
                Reply::Validated {
                    epoch,
                    generation,
                    result,
                }
            }
            _ => Reply::Ignored,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../target/autonomous-indexing")
                .join(format!(
                    "unit-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
            std::fs::create_dir_all(&root).unwrap();
            for i in 0..600 {
                std::fs::create_dir_all(root.join(format!("needle-{i:03}"))).unwrap();
            }
            Self(root.canonicalize().unwrap())
        }
        fn start(&self, worker: &mut Engine, epoch: u64) -> (Option<Reply>, Option<Request>) {
            let home = self.0.to_str().unwrap();
            let effects = worker.dispatch(
                Request::Start {
                    epoch,
                    home: home.into(),
                },
                home,
            );
            // Native tests supply the disposable mapping that WASI mounts at /host.
            worker.index = Some(HomeIndex::new(self.0.clone(), self.0.clone()).unwrap());
            effects
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn scan_finishes_from_worker_continuations_without_ui_requests() {
        let fixture = Fixture::new();
        let mut worker = Engine::default();
        let (ready, mut continuation) = fixture.start(&mut worker, 1);
        assert!(matches!(ready, Some(Reply::Ready { epoch: 1, .. })));
        assert!(
            continuation.is_some(),
            "start must schedule autonomous scan work"
        );
        let mut finished = false;
        let mut slices = 0;
        while let Some(request) = continuation {
            slices += 1;
            assert!(slices < 1000, "continuations must terminate");
            let (reply, next) = worker.dispatch(request, "unused after handshake");
            if let Some(Reply::Progress {
                revision,
                scanning: false,
                ..
            }) = reply
            {
                assert_eq!(revision, 600);
                finished = true;
            }
            continuation = next;
        }
        assert!(slices > 1, "scan must yield rather than block one handler");
        assert!(
            finished,
            "completion must reach UI without a timer or acknowledgement"
        );
    }
    #[test]
    fn progress_is_periodic_but_completion_is_immediate() {
        use std::time::{Duration, Instant};
        let fixture = Fixture::new();
        let mut worker = Engine::default();
        let (_, next) = fixture.start(&mut worker, 1);
        let now = Instant::now() + Duration::from_millis(100);
        let (reply, next) = worker.dispatch_at(next.unwrap(), "", now);
        assert!(matches!(
            reply,
            Some(Reply::Progress { scanning: true, .. })
        ));
        let (reply, mut next) = worker.dispatch_at(next.unwrap(), "", now);
        assert!(reply.is_none(), "each scan slice must not publish progress");
        let mut final_seen = false;
        while let Some(request) = next {
            let (reply, following) = worker.dispatch_at(request, "", now);
            if let Some(Reply::Progress {
                scanning, revision, ..
            }) = reply
            {
                assert!(!scanning, "no periodic update before 100ms");
                assert_eq!(revision, 600);
                final_seen = true;
            }
            next = following;
        }
        assert!(final_seen, "final status bypasses progress throttle");
    }
    #[test]
    fn refresh_and_interleaved_requests_keep_one_continuation() {
        let fixture = Fixture::new();
        let mut worker = Engine::default();
        let (_, old) = fixture.start(&mut worker, 1);
        for epoch in 2..10 {
            let (reply, next) = fixture.start(&mut worker, epoch);
            assert!(matches!(reply, Some(Reply::Ready { .. })));
            assert!(next.is_none(), "refresh must not duplicate queued work");
        }
        let (_, next) = worker.dispatch(old.unwrap(), "");
        let (reply, extra) = worker.dispatch(
            Request::Validate {
                epoch: 9,
                generation: 4,
                raw: "~".into(),
            },
            "",
        );
        assert!(matches!(
            reply,
            Some(Reply::Validated { result: Ok(_), .. })
        ));
        assert!(extra.is_none());
        let (_, mut next) = worker.dispatch(next.unwrap(), "");
        let (reply, extra) = worker.dispatch(
            Request::Query {
                epoch: 9,
                generation: 5,
                text: "needle".into(),
            },
            "",
        );
        assert!(matches!(reply, Some(Reply::Results { paths, .. }) if !paths.is_empty()));
        assert!(extra.is_none());
        while let Some(request) = next {
            let (_, following) = worker.dispatch(request, "");
            next = following;
        }
        assert_eq!(worker.index.as_ref().unwrap().dirs.len(), 600);
        let (reply, next) = worker.dispatch(Request::Step { epoch: 1 }, "");
        assert!(reply.is_none() && next.is_none());
    }
    #[test]
    fn oversized_queries_are_rejected_without_poisoning_worker() {
        use zellij_launchpad_prototype::search::Directory;
        let home = "/home/example";
        let mut worker = Engine::default();
        worker.handle(
            Request::Start {
                epoch: 1,
                home: home.into(),
            },
            home,
        );
        for name in ["a".repeat(101), "修".repeat(101), "needle".into()] {
            worker.index.as_mut().unwrap().dirs.push(Directory {
                path: format!("{home}/{name}"),
                note: "directory",
                error: None,
            });
        }
        for text in ["a".repeat(101), "修".repeat(101), "a".repeat(4096)] {
            let Reply::Results { paths, .. } = worker.handle(
                Request::Query {
                    epoch: 1,
                    generation: 2,
                    text,
                },
                home,
            ) else {
                panic!("expected query reply")
            };
            assert!(
                paths.is_empty(),
                "oversized query must not be matched or truncated"
            );
        }
        for text in ["a".repeat(100), "修".repeat(100), "needle".into()] {
            let Reply::Results { paths, .. } = worker.handle(
                Request::Query {
                    epoch: 1,
                    generation: 3,
                    text,
                },
                home,
            ) else {
                panic!("expected query reply")
            };
            assert!(!paths.is_empty(), "bounded query must still match");
        }
    }
    #[test]
    fn real_frizbee_results_are_byte_and_row_bounded() {
        let mut worker = Engine::default();
        worker.handle(
            Request::Start {
                epoch: 1,
                home: "/home/example".into(),
            },
            "/home/example",
        );
        let index = worker.index.as_mut().unwrap();
        for i in 0..400 {
            index
                .dirs
                .push(zellij_launchpad_prototype::search::Directory {
                    path: format!("/home/example/{}/needle{i}", "x".repeat(1000)),
                    note: "directory",
                    error: None,
                });
        }
        let reply = worker.handle(
            Request::Query {
                epoch: 1,
                generation: 2,
                text: "ndl".into(),
            },
            "/home/example",
        );
        let Reply::Results {
            paths, revision, ..
        } = reply
        else {
            panic!()
        };
        assert_eq!(revision, 400);
        assert!(!paths.is_empty());
        assert!(paths.len() <= 100);
        assert!(paths.iter().map(String::len).sum::<usize>() <= 64 * 1024);
        assert!(paths.iter().all(|p| p.contains("needle")));
    }
}

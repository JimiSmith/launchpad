//! Bounded, serial worker protocol. State lives inside the worker instance.
use serde::{Deserialize, Serialize};
use zellij_launchpad_prototype::search::HomeIndex;
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
}
impl Engine {
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
                let paths = if text.len() > 4096 {
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

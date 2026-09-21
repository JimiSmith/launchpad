//! One bounded native worker owns traversal, matching and validation.
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread,
    time::{Duration, Instant},
};
use zellij_launchpad_core::{editor::MAX_INPUT_CHARS, remote::RemoteRequest, search::HomeIndex};

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
        let (requests, demand) = mpsc::sync_channel::<Request>(1);
        let (responses, replies) = mpsc::sync_channel(4);
        thread::Builder::new()
            .name("home-search".into())
            .spawn(move || {
                if let Err(error) = run(home, cwd, demand, &responses) {
                    let _ = responses.send(Reply::Failed(error));
                }
            })?;
        Ok(Self { requests, replies })
    }
}
fn run(
    home: PathBuf,
    cwd: String,
    demand: Receiver<Request>,
    responses: &SyncSender<Reply>,
) -> Result<(), String> {
    let mut index = HomeIndex::new(home.clone(), home.clone())?;
    let mut epoch = 0;
    let mut progress = Instant::now() - Duration::from_secs(1);
    loop {
        let request = if index.is_scanning() {
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
                index = HomeIndex::new(home.clone(), home.clone())?;
            }
            let reply = match request {
                RemoteRequest::Query { generation, text } => {
                    let mut bytes = 0;
                    let paths = if text.chars().count() > MAX_INPUT_CHARS {
                        Vec::new()
                    } else {
                        zellij_launchpad_core::search::matches_in(
                            &text,
                            &index.dirs,
                            home.to_str().unwrap(),
                        )
                        .into_iter()
                        .take(100)
                        .map(|i| &index.dirs[i].path)
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
                        revision: index.dirs.len() as u64,
                        paths,
                    }
                }
                RemoteRequest::Validate { generation, raw } => Reply::Validated {
                    epoch,
                    generation,
                    result: validate(&index, &cwd, &raw),
                },
            };
            if responses.send(reply).is_err() {
                break;
            }
        }
        if index.is_scanning() {
            index.step(128);
            if progress.elapsed() >= Duration::from_millis(100) || !index.is_scanning() {
                if responses
                    .send(Reply::Progress {
                        epoch,
                        revision: index.dirs.len() as u64,
                        status: index.status(),
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
pub fn validate(index: &HomeIndex, cwd: &str, raw: &str) -> Result<String, String> {
    if raw.len() > 4096 {
        return Err("Path exceeds 4096 bytes".into());
    }
    let label = Path::new(cwd).strip_prefix(&index.home).ok().map(|rest| {
        if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", rest.display())
        }
    });
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

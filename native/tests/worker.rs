use launchpad::worker::{Reply, Request, Worker, validate};
use launchpad_core::{remote::RemoteRequest, search::HomeIndex};
use std::{path::PathBuf, time::Duration};
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/native-worker-tests")
            .join(format!("{}-{name}", std::process::id()));
        std::fs::create_dir_all(root.join("home/needle")).unwrap();
        let root = root.canonicalize().unwrap();
        // Keep the same host spelling the index returns on Windows.
        Self(
            launchpad_core::host_path::normalize(root.to_str().unwrap(), root.to_str().unwrap())
                .unwrap()
                .into(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn validation_preserves_exact_outside_cwd_exception_and_rejects_arbitrary_symlinks() {
    let f = Fixture::new("validation");
    let outside = f.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let home = f.0.join("home");
    let index = HomeIndex::new(home.clone(), home.clone()).unwrap();
    #[cfg(unix)]
    {
        let link = home.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(validate(&index, outside.to_str().unwrap(), "~/link").is_err());
        assert_eq!(
            validate(&index, link.to_str().unwrap(), "~/link").unwrap(),
            link.to_str().unwrap()
        );
    }
    assert!(validate(&index, outside.to_str().unwrap(), outside.to_str().unwrap()).is_ok());
    std::fs::remove_dir(&outside).unwrap();
    assert!(
        validate(&index, outside.to_str().unwrap(), outside.to_str().unwrap())
            .unwrap_err()
            .contains("no fallback")
    );
}
#[test]
fn background_index_progress_search_and_refresh_use_epochs() {
    let f = Fixture::new("epochs");
    let home = f.0.join("home");
    let worker = Worker::start(home.clone(), home.to_str().unwrap().into()).unwrap();
    loop {
        if let Reply::Progress { status, .. } =
            worker.replies.recv_timeout(Duration::from_secs(5)).unwrap()
            && status.contains("indexed")
        {
            break;
        }
    }
    for epoch in [0, 1] {
        worker
            .requests
            .send(Request {
                epoch,
                request: RemoteRequest::Validate {
                    generation: 17,
                    raw: "~/needle".into(),
                },
            })
            .unwrap();
        loop {
            if let Reply::Validated {
                epoch: response,
                generation,
                result,
            } = worker.replies.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(response, epoch);
                assert_eq!(generation, 17);
                assert_eq!(result.unwrap(), home.join("needle").to_str().unwrap());
                break;
            }
        }
    }
}

#[test]
fn configured_ignores_filter_queries_after_refresh_but_allow_literal_validation() {
    let f = Fixture::new("configured-ignores");
    let home = f.0.join("home");
    std::fs::create_dir_all(home.join("needle/deep")).unwrap();
    std::fs::create_dir(home.join("needle-old")).unwrap();
    let config: launchpad::config::Config = toml::from_str(&format!(
        "ignore = [{}]",
        toml::Value::String(home.join("needle").to_str().unwrap().into())
    ))
    .unwrap();
    let ignore = config.apply(&mut launchpad_core::app::App::default());
    let worker =
        Worker::start_with_ignore(home.clone(), home.to_str().unwrap().into(), ignore).unwrap();
    for epoch in [0, 1] {
        worker
            .requests
            .send(Request {
                epoch,
                request: RemoteRequest::Validate {
                    generation: 17,
                    raw: "~/needle/deep".into(),
                },
            })
            .unwrap();
        let (mut complete, mut validated) = (false, false);
        while !complete || !validated {
            match worker.replies.recv_timeout(Duration::from_secs(5)).unwrap() {
                Reply::Progress {
                    epoch: response,
                    status,
                    ..
                } if response == epoch => {
                    complete = status.contains("HOME indexed");
                }
                Reply::Validated {
                    epoch: response,
                    result,
                    ..
                } => {
                    assert_eq!(response, epoch);
                    assert_eq!(
                        result.unwrap(),
                        home.join("needle").join("deep").to_str().unwrap()
                    );
                    validated = true;
                }
                Reply::Failed(error) => panic!("{error}"),
                _ => {}
            }
        }
        worker
            .requests
            .send(Request {
                epoch,
                request: RemoteRequest::Query {
                    generation: 18,
                    text: "needle".into(),
                    recent: Vec::new(),
                },
            })
            .unwrap();
        loop {
            match worker.replies.recv_timeout(Duration::from_secs(5)).unwrap() {
                Reply::Results {
                    epoch: response,
                    generation,
                    paths,
                    ..
                } => {
                    assert_eq!(response, epoch);
                    assert_eq!(generation, 18);
                    assert_eq!(paths, [home.join("needle-old").to_str().unwrap()]);
                    break;
                }
                Reply::Failed(error) => panic!("{error}"),
                _ => {}
            }
        }
    }
}

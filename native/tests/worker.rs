use std::{path::PathBuf, time::Duration};
use zellij_launchpad::worker::{Reply, Request, Worker, validate};
use zellij_launchpad_core::{remote::RemoteRequest, search::HomeIndex};
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/native-worker-tests")
            .join(format!("{}-{name}", std::process::id()));
        std::fs::create_dir_all(root.join("home/needle")).unwrap();
        Self(root.canonicalize().unwrap())
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
    let link = home.join("link");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert!(validate(&index, outside.to_str().unwrap(), "~/link").is_err());
    assert_eq!(
        validate(&index, link.to_str().unwrap(), "~/link").unwrap(),
        link.to_str().unwrap()
    );
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

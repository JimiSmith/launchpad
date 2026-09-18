use launchpad_plugin::workers::{Engine, Reply, Request};
#[test]
fn worker_refuses_wrong_mapping_and_steps_only_current_epoch() {
    let mut worker = Engine::default();
    assert!(matches!(
        worker.handle(
            Request::Start {
                epoch: 1,
                home: "/home/example".into()
            },
            "/wrong"
        ),
        Reply::Failed { .. }
    ));
    assert!(matches!(
        worker.handle(
            Request::Start {
                epoch: 2,
                home: "/home/example".into()
            },
            "/home/example"
        ),
        Reply::Ready { epoch: 2, .. }
    ));
    assert!(matches!(
        worker.handle(Request::Step { epoch: 1 }, "/home/example"),
        Reply::Ignored
    ));
}
#[test]
fn worker_search_is_capped_and_carries_epoch_and_generation() {
    let mut worker = Engine::default();
    worker.handle(
        Request::Start {
            epoch: 4,
            home: "/home/example".into(),
        },
        "/home/example",
    );
    assert!(matches!(
        worker.handle(
            Request::Query {
                epoch: 4,
                generation: 7,
                text: "nts".into()
            },
            "/home/example"
        ),
        Reply::Results {
            epoch: 4,
            generation: 7,
            ..
        }
    ));
    assert!(matches!(
        worker.handle(
            Request::Query {
                epoch: 3,
                generation: 7,
                text: "nts".into()
            },
            "/home/example"
        ),
        Reply::Ignored
    ));
    assert!(matches!(
        worker.handle(
            Request::Validate {
                epoch: 4,
                generation: 8,
                raw: "/etc".into()
            },
            "/home/example"
        ),
        Reply::Validated {
            epoch: 4,
            generation: 8,
            result: Err(_),
            ..
        }
    ));
}

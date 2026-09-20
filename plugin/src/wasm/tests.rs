//! Exercise the production Plugin::update adapter, faking only host IO.
use super::*;
use std::{cell::RefCell, path::PathBuf};

#[derive(Default)]
struct Host {
    remounts: Vec<PathBuf>,
    requests: Vec<Request>,
    directory_reads: usize,
    time: Option<Instant>,
    timers: Vec<f64>,
}
thread_local! {
    static HOST: RefCell<Host> = RefCell::new(Host::default());
}
pub(super) fn change_host_folder(path: PathBuf) {
    HOST.with_borrow_mut(|host| host.remounts.push(path));
}
pub(super) fn send(request: Request) {
    HOST.with_borrow_mut(|host| host.requests.push(request));
}
pub(super) fn set_timeout(seconds: f64) {
    HOST.with_borrow_mut(|host| host.timers.push(seconds));
}
pub(super) fn now() -> Instant {
    HOST.with_borrow_mut(|host| *host.time.get_or_insert_with(Instant::now))
}
fn advance(plugin: &mut Plugin, millis: u64) {
    let next = now() + Duration::from_millis(millis);
    HOST.with_borrow_mut(|host| host.time = Some(next));
    plugin.update(Event::Timer(millis as f64 / 1000.0));
}
pub(super) fn read_dir(path: &str) -> std::io::Result<()> {
    assert_eq!(path, "/host");
    HOST.with_borrow_mut(|host| host.directory_reads += 1);
    Ok(())
}
fn key(plugin: &mut Plugin, key: BareKey) {
    plugin.update(Event::Key(KeyWithModifier::new(key)));
}
fn reply(plugin: &mut Plugin, reply: Reply) {
    plugin.update(Event::CustomMessage(
        "index-reply".into(),
        serde_json::to_string(&reply).unwrap(),
    ));
}
fn ready(plugin: &mut Plugin) {
    reply(
        plugin,
        Reply::Ready {
            epoch: plugin.epoch,
            cwd: "/fixture/home".into(),
        },
    );
    let query = HOST.with_borrow(|h| match h.requests.last() {
        Some(Request::Query {
            epoch, generation, ..
        }) => Some((*epoch, *generation)),
        _ => None,
    });
    if let Some((epoch, generation)) = query {
        reply(
            plugin,
            Reply::Results {
                epoch,
                generation,
                revision: 0,
                paths: vec![],
            },
        );
    }
}
fn plugin() -> Plugin {
    HOST.with_borrow_mut(|host| *host = Host::default());
    let mut plugin = Plugin {
        home: Some("/fixture/home".into()),
        original_cwd: Some("/fixture/original".into()),
        // Keep persistence and OS spawning out of native tests. The real-host
        // verifier covers those; all event routing and request dispatch is real.
        simulate_launch: true,
        ..Plugin::default()
    };
    plugin.start();
    ready(&mut plugin);
    plugin
}

fn query_count() -> usize {
    HOST.with_borrow(|h| {
        h.requests
            .iter()
            .filter(|r| matches!(r, Request::Query { .. }))
            .count()
    })
}

#[test]
fn repeated_backspace_edits_immediately_and_searches_once_after_quiet() {
    let mut plugin = plugin();
    let before = query_count();
    // Also exercise repeats after the field has become empty.
    for _ in 0..25 {
        let previous = plugin.state.app.editor.text.len();
        assert!(plugin.update(Event::Key(KeyWithModifier::new(BareKey::Backspace))));
        assert_eq!(
            plugin.state.app.editor.text.len(),
            previous.saturating_sub(1)
        );
        advance(&mut plugin, 30);
        assert_eq!(query_count(), before);
    }
    advance(&mut plugin, 89);
    assert_eq!(query_count(), before);
    advance(&mut plugin, 1);
    assert_eq!(query_count(), before + 1);
    HOST.with_borrow(|h| {
        assert!(matches!(h.requests.last(), Some(Request::Query { text, .. }) if text.is_empty()))
    });
    // The outstanding worker request continues to provide backpressure.
    advance(&mut plugin, 1000);
    assert_eq!(query_count(), before + 1);
}

#[test]
fn progress_and_stale_results_do_not_bypass_edit_debounce() {
    let mut plugin = plugin();
    key(&mut plugin, BareKey::Char('a'));
    advance(&mut plugin, 120);
    let (epoch, generation) = HOST.with_borrow(|h| match h.requests.last().unwrap() {
        Request::Query {
            epoch, generation, ..
        } => (*epoch, *generation),
        other => panic!("expected query, got {other:?}"),
    });
    let before = query_count();
    key(&mut plugin, BareKey::Char('b'));
    advance(&mut plugin, 60);
    reply(
        &mut plugin,
        Reply::Progress {
            epoch,
            revision: 1,
            status: "Indexing HOME".into(),
            scanning: true,
        },
    );
    reply(
        &mut plugin,
        Reply::Results {
            epoch,
            generation,
            revision: 1,
            paths: vec!["/fixture/home/stale".into()],
        },
    );
    assert!(plugin.state.app.suggestions.is_empty());
    assert_eq!(query_count(), before);
    advance(&mut plugin, 59);
    assert_eq!(query_count(), before);
    advance(&mut plugin, 1);
    assert_eq!(query_count(), before + 1);
    HOST.with_borrow(|h| {
        assert!(
            matches!(h.requests.last(), Some(Request::Query { text, .. }) if text.ends_with("ab"))
        )
    });
}

#[test]
fn paste_then_submit_bypasses_search_delay() {
    let mut plugin = plugin();
    let before = query_count();
    plugin.update(Event::PastedText("/new-target".into()));
    key(&mut plugin, BareKey::Enter);
    assert_eq!(query_count(), before);
    HOST.with_borrow(|h| assert!(matches!(h.requests.last(), Some(Request::Validate { raw, .. }) if raw == "/fixture/original/new-target")));
    assert!(plugin.search_due.is_none());
}

#[test]
fn obsolete_timer_does_not_clear_a_newer_timer_or_hide_worker_timeout() {
    let mut plugin = plugin();
    key(&mut plugin, BareKey::Char('a'));
    advance(&mut plugin, 120); // search dispatched, watchdog remains necessary
    let timer = plugin.timer_due;
    let scheduled = HOST.with_borrow(|h| h.timers.len());
    advance(&mut plugin, 1); // a callback from an older timer
    assert_eq!(plugin.timer_due, timer);
    assert_eq!(HOST.with_borrow(|h| h.timers.len()), scheduled);
    advance(&mut plugin, 15000);
    assert!(plugin.failed);
    assert!(plugin.state.app.search_status.contains("timed out"));
}

#[test]
fn dismiss_and_reset_do_not_resurrect_a_deferred_query() {
    let mut plugin = plugin();
    let before = query_count();
    key(&mut plugin, BareKey::Char('a'));
    key(&mut plugin, BareKey::Esc);
    advance(&mut plugin, 120);
    assert_eq!(query_count(), before);
    key(&mut plugin, BareKey::Char('b'));
    key(&mut plugin, BareKey::F(5));
    assert!(plugin.search_due.is_none());
    ready(&mut plugin);
    let after_reset = query_count();
    advance(&mut plugin, 120);
    assert_eq!(query_count(), after_reset);
    assert_eq!(plugin.state.app.editor.text, "/fixture/original");
}
fn failed_ack(plugin: &mut Plugin) {
    plugin.update(Event::FailedToChangeHostFolder(Some("missing".into())));
}

fn success_ack(plugin: &mut Plugin) {
    plugin.update(Event::HostFolderChanged("/fixture/original".into()));
}

#[test]
fn reset_resubmission_waits_for_old_ack_before_remounting() {
    for old_success in [true, false] {
        let mut plugin = plugin();
        key(&mut plugin, BareKey::Enter);
        key(&mut plugin, BareKey::F(5));
        ready(&mut plugin);
        key(&mut plugin, BareKey::Enter);
        assert_eq!(
            HOST.with_borrow(|h| h.remounts.len()),
            1,
            "reset must not overlap anonymous host requests"
        );
        if old_success {
            success_ack(&mut plugin);
        } else {
            failed_ack(&mut plugin);
        }
        assert_eq!(
            HOST.with_borrow(|h| h.remounts.len()),
            2,
            "draining old ack dispatches queued submission exactly once"
        );
        assert!(plugin.state.app.history.is_empty(), "old ack cannot launch");
        assert_eq!(
            HOST.with_borrow(|h| h.directory_reads),
            0,
            "stale success is not validated"
        );
        assert!(!plugin.failed);
        success_ack(&mut plugin);
        assert_eq!(plugin.state.app.history.len(), 1);
        assert_eq!(plugin.state.app.history[0].path, "/fixture/original");
        assert_eq!(HOST.with_borrow(|h| h.directory_reads), 1);
    }
}

#[test]
fn stale_ack_cannot_validate_a_new_path_after_repeated_resets() {
    for old_success in [true, false] {
        let mut plugin = plugin();
        key(&mut plugin, BareKey::Enter);
        for _ in 0..2 {
            key(&mut plugin, BareKey::F(5));
            ready(&mut plugin);
        }
        plugin.update(Event::Key(KeyWithModifier {
            bare_key: BareKey::Char('u'),
            key_modifiers: [KeyModifier::Ctrl].into_iter().collect(),
        }));
        plugin.update(Event::PastedText("~/new-target".into()));
        key(&mut plugin, BareKey::Enter);
        let sent_before = HOST.with_borrow(|h| h.requests.len());
        assert_eq!(HOST.with_borrow(|h| h.remounts.len()), 1);
        if old_success {
            success_ack(&mut plugin);
        } else {
            failed_ack(&mut plugin);
        }
        assert!(!plugin.failed);
        assert!(
            plugin.state.app.history.is_empty(),
            "stale ack must never launch original or new path"
        );
        assert_eq!(plugin.state.app.editor.text, "~/new-target");
        assert_eq!(HOST.with_borrow(|h| h.directory_reads), 0);
        let (epoch, generation, raw) = HOST.with_borrow(|h| {
            assert_eq!(h.requests.len(), sent_before + 1);
            match h.requests.last().unwrap() {
                Request::Validate {
                    epoch,
                    generation,
                    raw,
                } => (*epoch, *generation, raw.clone()),
                other => panic!("expected latest worker validation, got {other:?}"),
            }
        });
        assert_eq!(raw, "~/new-target");
        reply(
            &mut plugin,
            Reply::Validated {
                epoch,
                generation,
                result: Ok("/fixture/home/new-target".into()),
            },
        );
        assert_eq!(plugin.state.app.history.len(), 1);
        assert_eq!(plugin.state.app.history[0].path, "/fixture/home/new-target");
    }
}

#[test]
fn reset_late_success_before_worker_ready_is_discarded() {
    let mut plugin = plugin();
    key(&mut plugin, BareKey::Enter);
    key(&mut plugin, BareKey::F(5));
    success_ack(&mut plugin);
    assert!(plugin.state.app.history.is_empty());
    assert_eq!(HOST.with_borrow(|h| h.directory_reads), 0);
    ready(&mut plugin);
    assert!(plugin.ready && !plugin.failed);
    assert_eq!(HOST.with_borrow(|h| h.remounts.len()), 1);
    assert!(plugin.state.app.history.is_empty());
}

#[test]
fn only_pending_home_bootstrap_failure_is_fatal() {
    let mut plugin = plugin();
    failed_ack(&mut plugin);
    assert!(!plugin.failed, "unsolicited failures are not HOME failures");
    // Represent an outstanding bootstrap command (permission/load IO is not
    // under test); acknowledgement routing is the actual production adapter.
    plugin.remounting = true;
    failed_ack(&mut plugin);
    assert!(plugin.failed);
    assert!(plugin
        .state
        .app
        .search_status
        .contains("HOME filesystem unavailable"));
    assert!(!plugin.remounting);
}

#[test]
fn reset_late_cwd_failure_is_not_a_home_failure() {
    let mut plugin = plugin();
    key(&mut plugin, BareKey::Enter);
    assert_eq!(
        HOST.with_borrow(|h| h.remounts.clone()),
        vec![PathBuf::from("/fixture/original")]
    );
    key(&mut plugin, BareKey::F(5));
    failed_ack(&mut plugin);
    ready(&mut plugin);
    assert!(
        !plugin.failed,
        "a cancelled cwd remount must not poison HOME"
    );
    assert!(plugin.ready);
    assert_eq!(plugin.state.app.editor.text, "/fixture/original");
    assert!(plugin.state.app.history.is_empty());
    // A fresh explicit submission must still report a recoverable cwd error.
    key(&mut plugin, BareKey::Enter);
    failed_ack(&mut plugin);
    assert!(!plugin.failed);
    assert!(plugin
        .state
        .app
        .message
        .as_ref()
        .unwrap()
        .contains("Invoking directory unavailable"));
    assert!(plugin.state.app.history.is_empty());
}

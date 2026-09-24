//! Debounce search at dispatch, independently of which action changed the input.
use launchpad_core::{app::App, remote::RemoteRequest};
use std::time::{Duration, Instant};

pub struct SearchScheduler {
    text: String,
    epoch: u64,
    due: Instant,
}

impl SearchScheduler {
    pub fn new(app: &App, now: Instant) -> Self {
        Self {
            text: app.editor.text.clone(),
            epoch: app.remote_refresh(),
            due: now,
        }
    }

    /// Call every UI iteration, even while the worker is busy, so every observed
    /// text change extends the quiet period. Validation bypasses that period.
    pub fn poll(&mut self, app: &mut App, now: Instant, ready: bool) -> Option<RemoteRequest> {
        if self.epoch != app.remote_refresh() {
            // Explicit refresh starts immediately, including its restored text.
            self.epoch = app.remote_refresh();
            self.text.clone_from(&app.editor.text);
            self.due = now;
        } else if self.text != app.editor.text {
            self.text.clone_from(&app.editor.text);
            self.due = now + Duration::from_millis(120);
        }
        if ready {
            app.take_remote_request_with_search(now >= self.due)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launchpad_core::app::{Action, Focus};

    fn query(request: Option<RemoteRequest>, expected: &str) {
        let Some(RemoteRequest::Query { text, .. }) = request else {
            panic!("expected a query for {expected:?}, got {request:?}");
        };
        assert_eq!(text, expected);
    }

    #[test]
    fn every_text_change_is_debounced_without_an_action_classifier() {
        let now = Instant::now();
        let mut app = App::from_remote("/home/example".into());
        let mut scheduler = SearchScheduler::new(&app, now);
        // Even a text change with no Action goes through the same gate. This is
        // the invariant that keeps future bindings from bypassing debounce.
        app.editor.set("foo/bar");
        assert!(scheduler.poll(&mut app, now, true).is_none());
        assert!(
            scheduler
                .poll(&mut app, now + Duration::from_millis(119), true)
                .is_none()
        );
        query(
            scheduler.poll(&mut app, now + Duration::from_millis(120), true),
            "foo/bar",
        );
        for action in [
            Action::Text("x".into()),
            Action::Backspace,
            Action::Delete,
            Action::DeleteSegmentLeft,
            Action::DeleteSegmentRight,
            Action::Clear,
        ] {
            let mut app = App::from_remote("/home/example".into());
            app.editor.set("foo/bar/baz");
            app.editor.cursor = 5;
            let mut scheduler = SearchScheduler::new(&app, now);
            query(scheduler.poll(&mut app, now, true), "foo/bar/baz");
            app.update(action.clone());
            let expected = app.editor.text.clone();
            assert_ne!(expected, "foo/bar/baz", "{action:?}");
            assert!(scheduler.poll(&mut app, now, true).is_none(), "{action:?}");
            assert!(
                scheduler
                    .poll(&mut app, now + Duration::from_millis(119), true)
                    .is_none()
            );
            query(
                scheduler.poll(&mut app, now + Duration::from_millis(120), true),
                &expected,
            );
        }
    }

    #[test]
    fn busy_worker_and_repeated_edits_wait_for_the_latest_text() {
        let now = Instant::now();
        let mut app = App::from_remote("/home/example".into());
        let mut scheduler = SearchScheduler::new(&app, now);
        query(scheduler.poll(&mut app, now, true), "~");
        app.update(Action::Text("/one".into()));
        assert!(scheduler.poll(&mut app, now, false).is_none());
        app.update(Action::Text("/two".into()));
        assert!(
            scheduler
                .poll(&mut app, now + Duration::from_millis(100), false)
                .is_none()
        );
        assert!(
            scheduler
                .poll(&mut app, now + Duration::from_millis(120), true)
                .is_none()
        );
        query(
            scheduler.poll(&mut app, now + Duration::from_millis(220), true),
            "~/one/two",
        );
        assert!(
            scheduler
                .poll(&mut app, now + Duration::from_millis(300), true)
                .is_none()
        );
    }

    #[test]
    fn motion_ignored_edits_and_progress_do_not_restart_the_delay() {
        let now = Instant::now();
        let mut app = App::from_remote("/home/example".into());
        let mut scheduler = SearchScheduler::new(&app, now);
        query(scheduler.poll(&mut app, now, true), "~");
        app.update(Action::Text("/one/two".into()));
        assert!(scheduler.poll(&mut app, now, true).is_none());
        app.update(Action::SegmentLeft);
        app.remote_progress(1, "Indexing HOME".into());
        app.update(Action::Focus(Focus::Tools));
        app.update(Action::DeleteSegmentRight);
        assert!(
            scheduler
                .poll(&mut app, now + Duration::from_millis(100), true)
                .is_none()
        );
        app.update(Action::Focus(Focus::Path));
        query(
            scheduler.poll(&mut app, now + Duration::from_millis(120), true),
            "~/one/two",
        );
        app.remote_progress(2, "HOME indexed".into());
        query(
            scheduler.poll(&mut app, now + Duration::from_millis(121), true),
            "~/one/two",
        );
    }

    #[test]
    fn validation_and_explicit_reset_bypass_debounce() {
        let now = Instant::now();
        let mut app = App::from_remote("/home/example".into());
        let mut scheduler = SearchScheduler::new(&app, now);
        app.update(Action::Text("/one".into()));
        assert!(scheduler.poll(&mut app, now, true).is_none());
        app.update(Action::Enter);
        assert!(scheduler.poll(&mut app, now, false).is_none());
        assert!(matches!(
            scheduler.poll(&mut app, now, true),
            Some(RemoteRequest::Validate { .. })
        ));
        app.update(Action::Reset);
        query(scheduler.poll(&mut app, now, true), "~");
    }
}

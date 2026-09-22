use zellij_launchpad_core::app::{Action, App};
use zellij_launchpad_core::remote::RemoteRequest;

#[test]
fn segment_edits_refresh_search_but_motion_does_not() {
    for action in [Action::DeleteSegmentLeft, Action::DeleteSegmentRight] {
        let mut app = App::from_remote("/home/example".into());
        app.update(Action::Clear);
        app.update(Action::Text("foo/bar/baz".into()));
        let old = app.take_remote_request().unwrap().generation();
        app.update(Action::Home);
        app.update(Action::SegmentRight);
        app.update(Action::SegmentRight);
        assert_eq!(app.editor.cursor, 7);
        assert!(app.take_remote_request().is_none());
        app.update(action.clone());
        let expected = if action == Action::DeleteSegmentLeft {
            "foo//baz"
        } else {
            "foo/bar"
        };
        assert_eq!(app.editor.text, expected);
        assert!(app.touched);
        assert!(!app.apply_remote_results(old, vec!["/home/example/stale".into()]));
        let Some(RemoteRequest::Query { text, generation }) = app.take_remote_request() else {
            panic!()
        };
        assert_eq!(text, expected);
        assert!(generation > old);
    }
}

#[test]
fn segment_actions_are_ignored_outside_active_path_editor() {
    use zellij_launchpad_core::app::Focus;
    for (focus, help, compact) in [
        (Focus::Tools, false, false),
        (Focus::History, false, false),
        (Focus::Path, true, false),
        (Focus::Path, false, true),
    ] {
        let mut app = App::from_remote("/home/example".into());
        app.editor.set("foo/bar");
        app.history.push(zellij_launchpad_core::app::Launch {
            id: 1,
            path: "/home/example/foo/bar".into(),
            tool: zellij_launchpad_core::app::Tool::Shell,
            age: "now".into(),
        });
        app.focus = focus;
        app.take_remote_request();
        app.update(Action::LaunchForm);
        let Some(RemoteRequest::Validate { generation, .. }) = app.take_remote_request() else {
            panic!()
        };
        app.help = help;
        app.compact = compact;
        for action in [
            Action::SegmentLeft,
            Action::SegmentRight,
            Action::DeleteSegmentLeft,
            Action::DeleteSegmentRight,
        ] {
            app.update(action);
            assert_eq!(app.editor.text, "foo/bar");
            assert_eq!(app.editor.cursor, 7);
            assert_eq!(app.focus, focus);
            assert!(app.take_remote_request().is_none());
        }
        assert!(app.finish_remote_validation(generation, Ok("/home/example/foo/bar".into())));
    }
}

#[test]
fn editor_path_style_follows_home_and_survives_reset() {
    for (home, expected) in [
        ("/home/example", 0),
        (r"C:\Users\example", 4),
        (r"\\server\share\example", 4),
    ] {
        let mut app = App::from_remote(home.into());
        for reset in [false, true] {
            if reset {
                app.update(Action::Reset);
            }
            app.editor.set(r"foo\bar");
            app.update(Action::SegmentLeft);
            assert_eq!(app.editor.cursor, expected, "{home}, reset={reset}");
        }
    }
}

#[test]
fn async_search_coalesces_edits_and_rejects_dismissed_or_stale_results() {
    let mut app = App::from_remote("/home/example".into());
    app.update(Action::Clear);
    app.update(Action::Text("nts".into()));
    let RemoteRequest::Query { generation, text } = app.take_remote_request().unwrap() else {
        panic!()
    };
    assert_eq!(text, "nts");
    app.update(Action::Text("x".into()));
    app.update(Action::Backspace);
    assert!(!app.apply_remote_results(generation, vec!["/home/example/notes".into()]));
    let RemoteRequest::Query { generation, text } = app.take_remote_request().unwrap() else {
        panic!()
    };
    assert_eq!(text, "nts");
    app.update(Action::Escape);
    assert!(!app.apply_remote_results(generation, vec!["/home/example/notes".into()]));
    app.remote_progress(1, "Indexing HOME".into());
    assert!(app.take_remote_request().is_none());
    assert!(app.dirs.is_empty());
}
#[test]
fn validation_runs_off_ui_and_late_selection_or_launch_cannot_win() {
    let mut app = App::from_remote("/home/example".into());
    assert!(app.apply_remote_results(0, vec!["/home/example/notes".into()]));
    app.update(Action::AcceptSuggestion(0));
    let RemoteRequest::Validate { generation, raw } = app.take_remote_request().unwrap() else {
        panic!()
    };
    assert_eq!(raw, "/home/example/notes");
    assert!(app.message.as_deref().unwrap().contains("Validating"));
    assert!(app.finish_remote_validation(generation, Ok(raw)));
    assert_eq!(app.editor.text, "~/notes");
    assert!(app.take_launch().is_none());
    app.update(Action::Enter);
    let RemoteRequest::Validate { generation, .. } = app.take_remote_request().unwrap() else {
        panic!()
    };
    app.update(Action::Text("x".into()));
    assert!(!app.finish_remote_validation(generation, Ok("/home/example/notes".into())));
    assert!(app.history.is_empty());
    app.update(Action::Enter);
    let RemoteRequest::Validate { generation, .. } = app.take_remote_request().unwrap() else {
        panic!()
    };
    assert!(app.finish_remote_validation(generation, Err("Directory unavailable".into())));
    assert!(app.take_launch().is_none());
    assert!(app.message.as_deref().unwrap().contains("unavailable"));
    app.update(Action::Enter);
    let RemoteRequest::Validate { generation, .. } = app.take_remote_request().unwrap() else {
        panic!()
    };
    assert!(app.finish_remote_validation(generation, Ok("/home/example/notes".into())));
    assert!(app.take_launch().is_some());
    assert!(!app.finish_remote_validation(generation, Ok("/home/example/notes".into())));
    assert!(app.history.is_empty(), "the shared store owns real history");
}
#[test]
fn navigating_results_never_resubmits_search_or_loses_selection() {
    let mut app = App::from_remote("/home/example".into());
    app.take_remote_request();
    app.apply_remote_results(0, vec!["/home/example/a".into(), "/home/example/b".into()]);
    app.update(Action::Down);
    assert!(app.take_remote_request().is_none());
    assert!(app.apply_remote_results(0, vec!["/home/example/b".into(), "/home/example/a".into()]));
    assert_eq!(app.highlighted, Some(1));
}
#[test]
fn timeout_after_selected_result_keeps_enter_safe() {
    let mut app = App::from_remote("/home/example".into());
    let generation = app.take_remote_request().unwrap().generation();
    assert!(app.apply_remote_results(generation, vec!["/home/example/notes".into()]));
    app.update(Action::Down);
    assert_eq!(app.highlighted, Some(0));

    app.remote_failed("Worker timed out".into());
    app.update(Action::Enter);
    assert_eq!(app.highlighted, None);
    assert!(app.suggestions.is_empty());
    assert_eq!(app.message.as_deref(), Some("Worker timed out"));
    assert!(app.take_launch().is_none());
    assert!(app.history.is_empty());
    assert!(app.take_remote_request().is_none());
    assert!(!app.apply_remote_results(generation, vec!["/home/example/late".into()]));
}

#[test]
fn enter_with_stale_highlight_does_not_index_missing_result() {
    let mut app = App::from_remote("/home/example".into());
    app.take_remote_request();
    app.highlighted = Some(0);
    app.update(Action::Enter);
    assert!(matches!(
        app.take_remote_request(),
        Some(RemoteRequest::Validate { .. })
    ));
    assert!(app.take_launch().is_none());
}

#[test]
fn delayed_selection_survives_path_cursor_navigation() {
    use zellij_launchpad_core::app::Focus;
    for action in [
        Action::Left,
        Action::Right,
        Action::SegmentLeft,
        Action::SegmentRight,
        Action::Home,
        Action::End,
        Action::PathCursor(0),
        Action::Focus(Focus::Path),
    ] {
        let mut app = App::from_remote("/home/example".into());
        app.take_remote_request();
        assert!(app.apply_remote_results(0, vec!["/home/example/notes".into()]));
        app.update(Action::AcceptSuggestion(0));
        let RemoteRequest::Validate { generation, raw } = app.take_remote_request().unwrap() else {
            panic!()
        };
        app.update(action.clone());
        assert!(app.take_remote_request().is_none(), "{action:?}");
        assert!(
            app.finish_remote_validation(generation, Ok(raw)),
            "{action:?}"
        );
        assert_eq!(app.editor.text, "~/notes", "{action:?}");
        assert_eq!(app.message, None, "{action:?}");
        assert!(app.take_launch().is_none());
        assert!(app.history.is_empty());
    }
}

#[test]
fn cancelling_delayed_validation_clears_status_and_restores_search() {
    use zellij_launchpad_core::app::{Focus, Tool};
    for start in [Action::AcceptSuggestion(0), Action::LaunchForm] {
        for action in [
            Action::Focus(Focus::Tools),
            Action::Focus(Focus::History),
            Action::Down,
            Action::SelectTool(Tool::new("codex").unwrap()),
            Action::Text("x".into()),
            Action::Backspace,
            Action::Delete,
            Action::DeleteSegmentLeft,
            Action::DeleteSegmentRight,
            Action::Clear,
            Action::ClearHistory,
        ] {
            let mut app = App::from_remote("/home/example".into());
            app.take_remote_request();
            app.apply_remote_results(0, vec!["/home/example/notes".into()]);
            app.update(start.clone());
            let generation = app.take_remote_request().unwrap().generation();
            app.update(action.clone());
            let text = app.editor.text.clone();
            assert!(!app.finish_remote_validation(generation, Ok("/home/example/notes".into())));
            assert_eq!(app.editor.text, text, "{start:?}, {action:?}");
            assert_ne!(
                app.message.as_deref(),
                Some("Validating directory…"),
                "{start:?}, {action:?}"
            );
            assert!(app.take_launch().is_none());
            assert!(app.history.is_empty());
            assert_eq!(app.highlighted, None);
            let Some(RemoteRequest::Query {
                generation: next,
                text: query,
            }) = app.take_remote_request()
            else {
                panic!("cancelled validation must resume search: {start:?}, {action:?}")
            };
            assert!(next > generation);
            assert_eq!(query, text);
            assert!(app.apply_remote_results(next, vec!["/home/example/fresh".into()]));
            app.update(Action::Focus(Focus::Path));
            app.update(Action::AcceptSuggestion(0));
            let Some(RemoteRequest::Validate { raw, .. }) = app.take_remote_request() else {
                panic!("fresh selection must be possible")
            };
            assert_eq!(
                raw, "/home/example/fresh",
                "cancelled selection must not survive"
            );
        }
    }
}

#[test]
fn timeout_during_selection_discards_delayed_reply() {
    let mut app = App::from_remote("/home/example".into());
    app.take_remote_request();
    app.apply_remote_results(0, vec!["/home/example/notes".into()]);
    app.update(Action::AcceptSuggestion(0));
    let generation = app.take_remote_request().unwrap().generation();
    app.remote_failed("Worker timed out".into());
    assert!(!app.finish_remote_validation(generation, Ok("/home/example/notes".into())));
    app.update(Action::Tab);
    assert_eq!(
        app.message.as_deref(),
        Some("Worker timed out"),
        "Tab keeps the failure visible without retrying"
    );
    assert_eq!(app.editor.text, "~");
    assert!(app.take_remote_request().is_none());
    assert!(app.take_launch().is_none());
}

#[test]
fn queued_validation_survives_cursor_motion_but_not_focus_or_tool_changes() {
    use zellij_launchpad_core::app::{Focus, Tool};
    for start in [Action::AcceptSuggestion(0), Action::LaunchForm] {
        let mut app = App::from_remote("/home/example".into());
        app.take_remote_request();
        app.apply_remote_results(0, vec!["/home/example/notes".into()]);
        app.update(start.clone());
        app.update(Action::Left);
        app.update(Action::SegmentLeft);
        app.update(Action::SegmentRight);
        let Some(RemoteRequest::Validate { generation, .. }) = app.take_remote_request() else {
            panic!("cursor motion must preserve even an unsent validation")
        };
        assert!(app.finish_remote_validation(generation, Ok("/home/example/notes".into())));
        if start == Action::AcceptSuggestion(0) {
            assert_eq!(app.message, None);
            assert_eq!(app.editor.text, "~/notes");
            assert!(app.take_launch().is_none());
        } else {
            assert!(app.take_launch().is_some());
            assert!(app.history.is_empty(), "the shared store owns real history");
        }
    }
    for action in [
        Action::Left,
        Action::Right,
        Action::Up,
        Action::Down,
        Action::PathCursor(0),
    ] {
        let mut app = App::from_remote("/home/example".into());
        app.take_remote_request();
        app.update(Action::Focus(Focus::Tools));
        app.update(Action::LaunchForm);
        let generation = app.take_remote_request().unwrap().generation();
        app.update(action);
        assert!(!app.finish_remote_validation(generation, Ok("/home/example".into())));
        assert_eq!(app.message, None);
        assert!(app.history.is_empty());
        assert!(matches!(
            app.take_remote_request(),
            Some(RemoteRequest::Query { .. })
        ));
    }
    let mut app = App::from_remote("/home/example".into());
    app.take_remote_request();
    app.update(Action::LaunchForm);
    app.update(Action::SelectTool(Tool::new("codex").unwrap()));
    assert!(matches!(
        app.take_remote_request(),
        Some(RemoteRequest::Query { .. })
    ));
    assert_eq!(
        app.message, None,
        "unsent validation must also be cancelled cleanly"
    );
}

#[test]
fn dismissing_delayed_validation_cannot_accept_or_launch_late() {
    for start in [Action::AcceptSuggestion(0), Action::LaunchForm] {
        for action in [Action::Escape, Action::Help, Action::Quit, Action::Reset] {
            let mut app = App::from_remote("/home/example".into());
            app.take_remote_request();
            app.apply_remote_results(0, vec!["/home/example/notes".into()]);
            app.update(start.clone());
            let generation = app.take_remote_request().unwrap().generation();
            app.update(action.clone());
            assert!(!app.finish_remote_validation(generation, Ok("/home/example/notes".into())));
            assert_eq!(app.editor.text, "~");
            assert_eq!(app.message, None);
            assert!(app.take_launch().is_none());
            assert!(app.history.is_empty());
            if action == Action::Reset {
                assert!(matches!(
                    app.take_remote_request(),
                    Some(RemoteRequest::Query { .. })
                ));
            } else {
                assert!(app.take_remote_request().is_none());
            }
            if action == Action::Help {
                app.update(Action::Help);
                assert!(matches!(
                    app.take_remote_request(),
                    Some(RemoteRequest::Query { .. })
                ));
            }
        }
    }
}

#[test]
fn tab_cycles_sections_without_selecting_a_suggestion() {
    let mut app = App::from_remote("/home/example".into());
    app.take_remote_request();
    app.apply_remote_results(0, vec!["/home/example/a".into(), "/home/example/b".into()]);
    app.update(Action::Tab);
    assert_eq!(app.focus, zellij_launchpad_core::app::Focus::Tools);
    assert!(app.take_remote_request().is_none());
    app.update(Action::Tab);
    assert_eq!(app.focus, zellij_launchpad_core::app::Focus::History);
    app.update(Action::BackTab);
    assert_eq!(app.focus, zellij_launchpad_core::app::Focus::Tools);
}

#[test]
fn old_index_revision_cannot_replace_newer_results() {
    let mut app = App::from_remote("/home/example".into());
    app.remote_progress(4, "Indexing HOME".into());
    assert!(!app.accept_remote_revision(3));
    assert!(app.accept_remote_revision(4));
    app.update(Action::Reset);
    assert!(app.accept_remote_revision(0));
    assert_eq!(
        app.remote_refresh(),
        1,
        "mouse and keyboard resets both request a worker epoch"
    );
}
#[test]
fn reset_close_quit_failure_and_section_navigation_keep_pending_work_safe() {
    use zellij_launchpad_core::app::{Focus, Launch, Tool};
    for action in [Action::Reset, Action::Quit, Action::Escape] {
        let mut app = App::from_remote("/home/example".into());
        app.take_remote_request();
        app.update(action);
        assert!(!app.apply_remote_results(0, vec!["/home/example/late".into()]));
    }
    let mut app = App::from_remote("/home/example".into());
    app.take_remote_request();
    app.remote_failed("Worker timed out".into());
    app.update(Action::Text("still editable".into()));
    assert!(app.editor.text.contains("still editable"));
    assert!(app.take_remote_request().is_none());
    app.update(Action::Enter);
    assert!(app.message.as_deref().unwrap().contains("timed out"));
    assert!(app.history.is_empty());
    let mut app = App::from_remote("/home/example".into());
    app.history.push(Launch {
        id: 1,
        path: "/home/example/saved".into(),
        tool: Tool::Shell,
        age: "now".into(),
    });
    app.take_remote_request();
    app.update(Action::Focus(Focus::History));
    app.update(Action::Tab);
    assert_eq!(app.focus, Focus::Path);
    assert_eq!(app.editor.text, "~/saved");
    assert!(!app.apply_remote_results(0, vec!["/home/example/late".into()]));
    app.update(Action::Text("a".into()));
    let query = app.take_remote_request().unwrap().generation();
    assert!(app.apply_remote_results(query, vec!["/home/example/late".into()]));
    app.remote_progress(100, "HOME indexed".into());
    app.update(Action::AcceptSuggestion(0));
    let generation = app.take_remote_request().unwrap().generation();
    app.update(Action::Escape);
    assert!(!app.finish_remote_validation(generation, Ok("/home/example/saved".into())));
    assert!(app.history.len() == 1);
}

#[test]
fn launch_from_empty_history_launches_the_form() {
    use zellij_launchpad_core::app::Focus;
    for action in [Action::LaunchForm, Action::Enter] {
        let mut app = App::from_remote("/home/example".into());
        app.editor.set("foo/bar");
        app.focus = Focus::History;
        app.take_remote_request();
        app.update(action.clone());
        assert!(
            matches!(
                app.take_remote_request(),
                Some(RemoteRequest::Validate { .. })
            ),
            "{action:?}"
        );
    }
}

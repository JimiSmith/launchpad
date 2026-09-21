use zellij_launchpad_core::{
    app::{Action, App, Tool},
    remote::RemoteRequest,
};

fn app() -> App {
    let mut app = App::from_remote("/home/fixture".into());
    app.configure(&std::collections::BTreeMap::from([
        ("commands".into(), "claude,codex,copilot,hermes".into()),
        ("command_claude".into(), "claude".into()),
        ("command_codex".into(), "codex".into()),
        ("command_copilot".into(), "copilot".into()),
        ("command_hermes".into(), "hermes".into()),
    ]));
    app
}
fn validate(app: &mut App, result: Result<String, String>) {
    let Some(RemoteRequest::Validate { generation, .. }) = app.take_remote_request() else {
        panic!("expected directory validation");
    };
    assert!(app.finish_remote_validation(generation, result));
}
#[test]
fn confirmed_host_history_clear_waits_for_durable_acknowledgement() {
    use zellij_launchpad_core::app::{Focus, Launch};
    let mut app = app();
    app.history.push(Launch {
        id: 1,
        path: "/home/fixture/a".into(),
        tool: Tool::new("codex").unwrap(),
        age: "Yesterday".into(),
    });
    app.update(Action::Focus(Focus::History));
    app.update(Action::ClearHistory);
    assert_eq!(app.history.len(), 1);
    app.update(Action::Escape);
    assert_eq!(app.history.len(), 1);
    app.update(Action::ClearHistory);
    app.update(Action::ClearHistory);
    assert_eq!(
        app.history.len(),
        1,
        "do not pretend clearing succeeded before persistence"
    );
    assert_eq!(
        app.take_history_mutation(),
        Some(zellij_launchpad_core::app::HistoryMutation::Clear)
    );
    assert!(app.take_history_mutation().is_none());
    app.update(Action::Delete);
    assert_eq!(app.history.len(), 1, "delete also waits for persistence");
    assert_eq!(
        app.take_history_mutation(),
        Some(zellij_launchpad_core::app::HistoryMutation::Remove(1))
    );
}
#[test]
fn real_launch_is_one_shot_and_never_records_its_own_history() {
    for id in ["shell", "claude", "codex", "copilot", "hermes"] {
        let tool = Tool::new(id).unwrap();
        let mut app = app();
        assert_eq!(app.visible_tools().len(), 5, "Shell plus four configured");
        app.update(Action::SelectTool(tool));
        app.update(Action::LaunchForm);
        assert!(
            app.take_launch().is_none(),
            "nothing reaches the host before validation"
        );
        validate(&mut app, Ok("/home/fixture/space 修理 it's; $HOME".into()));
        assert!(app.history.is_empty(), "the shared store owns real history");
        let launch = app.take_launch().expect("exactly one launch request");
        assert_eq!(launch.tool, tool);
        assert_eq!(launch.path, "/home/fixture/space 修理 it's; $HOME");
        for action in [
            Action::Enter,
            Action::LaunchForm,
            Action::Reset,
            Action::Escape,
        ] {
            app.update(action.clone());
            app.remote_progress(3, "HOME indexed".into());
            assert!(
                app.take_launch().is_none(),
                "{action:?} cannot launch again"
            );
        }
        assert!(!app.finish_remote_validation(0, Ok("/home/fixture".into())));
        assert!(app.take_launch().is_none());
    }
}
#[test]
fn rejected_host_request_retains_form_and_requires_explicit_resubmission() {
    let mut app = app();
    app.update(Action::SelectTool(Tool::new("copilot").unwrap()));
    app.update(Action::LaunchForm);
    validate(&mut app, Err("directory unavailable".into()));
    assert!(app.take_launch().is_none());
    assert_eq!(app.message.as_deref(), Some("directory unavailable"));
    app.update(Action::LaunchForm);
    validate(&mut app, Ok("/home/fixture".into()));
    assert!(app.take_launch().is_some());
    app.launch_rejected();
    assert!(app.take_launch().is_none());
    assert_eq!(app.tool, Tool::new("copilot").unwrap());
    assert_eq!(app.editor.text, "~");
    assert!(app.message.as_ref().unwrap().contains("did not accept"));
    app.remote_progress(4, "HOME indexed".into());
    assert!(app.take_launch().is_none());
    app.update(Action::LaunchForm);
    validate(&mut app, Ok("/home/fixture".into()));
    assert!(app.take_launch().is_some());
}
#[test]
fn rapid_submit_during_selection_does_not_launch_the_old_editor_path() {
    for submit in [Action::Enter, Action::LaunchForm] {
        let mut app = app();
        app.apply_remote_results(0, vec!["/home/fixture/selected".into()]);
        app.update(Action::AcceptSuggestion(0));
        let Some(RemoteRequest::Validate { generation, raw }) = app.take_remote_request() else {
            panic!("expected selection validation");
        };
        assert_eq!(raw, "/home/fixture/selected");
        app.update(submit);
        assert!(app.finish_remote_validation(generation, Ok(raw)));
        assert_eq!(app.editor.text, "~/selected");
        assert!(app.take_launch().is_none());
        assert!(!matches!(
            app.take_remote_request(),
            Some(RemoteRequest::Validate { .. })
        ));
        app.update(Action::Enter);
        validate(&mut app, Ok("/home/fixture/selected".into()));
        assert_eq!(app.take_launch().unwrap().path, "/home/fixture/selected");
    }
}
#[test]
fn real_launching_is_the_default_and_the_harness_flag_survives_reset() {
    let mut app = app();
    assert!(
        !app.simulate_launch,
        "the plugin launches for real by default"
    );
    assert!(!App::default().simulate_launch);
    assert!(!App::from_remote("/home/fixture".into()).simulate_launch);
    app.simulate_launch = true;
    app.update(Action::Reset);
    assert!(app.simulate_launch, "F5 keeps the configured harness mode");
}

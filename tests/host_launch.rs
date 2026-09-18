use zellij_launchpad_prototype::{
    app::{Action, App, Screen, Tool},
    remote::RemoteRequest,
};

fn app() -> App {
    let mut app = App::from_remote("/home/fixture".into());
    app.host_launch = true;
    app
}
fn validate(app: &mut App, result: Result<String, String>) {
    let Some(RemoteRequest::Validate { generation, .. }) = app.take_remote_request() else {
        panic!("expected directory validation");
    };
    assert!(app.finish_remote_validation(generation, result));
}
#[test]
fn real_launch_is_one_shot_not_a_simulated_terminal_or_history_event() {
    for tool in Tool::ALL {
        let mut app = app();
        app.copilot_available = false;
        assert_eq!(app.visible_tools(), Tool::ALL);
        app.update(Action::ToggleCopilot);
        app.update(Action::SelectTool(tool));
        app.update(Action::LaunchForm);
        assert!(app.take_host_launch().is_none());
        validate(&mut app, Ok("/home/fixture/space 修理 it's; $HOME".into()));
        assert_eq!(app.screen, Screen::Dashboard);
        assert!(app.history.is_empty());
        let launch = app.take_host_launch().unwrap();
        assert_eq!(launch.tool, tool);
        assert_eq!(launch.path, "/home/fixture/space 修理 it's; $HOME");
        for action in [
            Action::Enter,
            Action::LaunchForm,
            Action::Reset,
            Action::Escape,
        ] {
            app.update(action);
            app.remote_progress(3, "HOME indexed".into());
            assert!(app.take_host_launch().is_none());
        }
        assert!(!app.finish_remote_validation(0, Ok("/home/fixture".into())));
        assert!(app.take_host_launch().is_none());
    }
}
#[test]
fn rejected_host_request_retains_form_and_requires_explicit_resubmission() {
    let mut app = app();
    app.update(Action::SelectTool(Tool::Copilot));
    app.update(Action::LaunchForm);
    validate(&mut app, Err("directory unavailable".into()));
    assert!(app.take_host_launch().is_none());
    assert_eq!(app.message.as_deref(), Some("directory unavailable"));
    app.update(Action::LaunchForm);
    validate(&mut app, Ok("/home/fixture".into()));
    assert!(app.take_host_launch().is_some());
    app.host_launch_rejected();
    assert_eq!(app.screen, Screen::Dashboard);
    assert_eq!(app.tool, Tool::Copilot);
    assert_eq!(app.editor.text, "~");
    assert!(app.message.as_ref().unwrap().contains("did not accept"));
    app.remote_progress(4, "HOME indexed".into());
    assert!(app.take_host_launch().is_none());
    app.update(Action::LaunchForm);
    validate(&mut app, Ok("/home/fixture".into()));
    assert!(app.take_host_launch().is_some());
}
#[test]
fn rapid_submit_during_completion_does_not_launch_the_old_editor_path() {
    for submit in [Action::Enter, Action::LaunchForm] {
        let mut app = app();
        app.apply_remote_results(0, vec!["/home/fixture/selected".into()]);
        app.update(Action::Tab);
        let Some(RemoteRequest::Validate { generation, raw }) = app.take_remote_request() else {
            panic!("expected completion validation");
        };
        assert_eq!(raw, "/home/fixture/selected");
        app.update(submit);
        assert!(app.finish_remote_validation(generation, Ok(raw)));
        assert_eq!(app.editor.text, "~/selected");
        assert!(app.take_host_launch().is_none());
        assert!(!matches!(
            app.take_remote_request(),
            Some(RemoteRequest::Validate { .. })
        ));
        app.update(Action::Enter);
        validate(&mut app, Ok("/home/fixture/selected".into()));
        assert_eq!(
            app.take_host_launch().unwrap().path,
            "/home/fixture/selected"
        );
    }
}
#[test]
fn host_mode_survives_reset_while_native_remote_and_demo_default_to_simulation() {
    let mut app = app();
    app.update(Action::Reset);
    assert!(app.host_launch);
    assert!(!App::default().host_launch);
    assert!(!App::demo().host_launch);
    assert!(!App::from_remote("/home/fixture".into()).host_launch);
}

use zellij_launchpad_core::{
    app::{Action, App, Focus, Tool},
    remote::RemoteRequest,
};

#[test]
fn initial_cwd_is_a_full_identity_and_one_enter_validates_once_during_indexing() {
    let cwd = format!("/home/owned/{} 修理 e\u{301}; $HOME", "long".repeat(40));
    let mut app = App::from_remote("/home/owned".into());
    app.set_initial_cwd(cwd.clone());
    assert_eq!(
        app.editor.text,
        format!("~/{}", cwd.strip_prefix("/home/owned/").unwrap())
    );
    assert_eq!(app.tool, Tool::Shell);
    assert_eq!(app.focus, Focus::Path);
    app.remote_progress(1, "Indexing HOME".into());
    assert!(!app.apply_remote_results(0, vec!["/home/owned/other".into()]));
    assert!(
        app.suggestions.is_empty(),
        "initial directory stays quiet until edited"
    );
    assert_eq!(app.highlighted, None);
    assert!(app.take_remote_request().is_none());
    app.update(Action::Enter);
    let Some(RemoteRequest::Validate { generation, raw }) = app.take_remote_request() else {
        panic!("one Enter must validate, not accept a suggestion");
    };
    assert_eq!(raw, app.path_label(&cwd));
    app.update(Action::Enter);
    assert!(!matches!(
        app.take_remote_request(),
        Some(RemoteRequest::Validate { .. })
    ));
    assert!(app.finish_remote_validation(generation, Ok(cwd.clone())));
    assert_eq!(app.take_launch().unwrap().path, cwd);
    assert!(app.take_launch().is_none());
    app.update(Action::Enter);
    assert!(app.take_launch().is_none());
}

#[test]
fn reset_restores_original_identity_and_shell_but_editing_stays_normal_search() {
    let mut app = App::from_remote("/home/owned".into());
    app.set_initial_cwd("/outside/notes".into());
    app.update(Action::Clear);
    app.update(Action::Text("needle".into()));
    let Some(RemoteRequest::Query { text, .. }) = app.take_remote_request() else {
        panic!("edited input should query HOME");
    };
    assert_eq!(text, "needle");
    app.update(Action::Reset);
    assert_eq!(app.editor.text, "/outside/notes");
    assert_eq!(app.tool, Tool::Shell);
    assert_eq!(app.focus, Focus::Path);
    assert_eq!(app.highlighted, None);
    let Some(RemoteRequest::Query { generation, .. }) = app.take_remote_request() else {
        panic!("F5 must notify the worker even when suggestions are hidden");
    };
    assert!(!app.apply_remote_results(generation, vec!["/home/owned/notes".into()]));
    app.update(Action::Enter);
    let Some(RemoteRequest::Validate { generation, .. }) = app.take_remote_request() else {
        panic!();
    };
    assert!(app.finish_remote_validation(
        generation,
        Err("Invoking directory unavailable; no fallback.".into())
    ));
    assert_eq!(app.editor.text, "/outside/notes");
    assert!(app.message.as_ref().unwrap().contains("no fallback"));
    assert!(app.take_launch().is_none());
}

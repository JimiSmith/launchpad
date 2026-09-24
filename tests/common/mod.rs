//! Shared integration fixture: a worker-backed `App` exactly as the plugin
//! builds one, seeded from settled worker replies. There is no fixture data
//! inside the shipped crate, so every test states its own directories.
#![allow(dead_code)]
use launchpad_core::app::{Action, App, Launch, Tool};
use launchpad_core::remote::RemoteRequest;

pub const HOME: &str = "/home/example";

pub fn tool(id: &str) -> Tool {
    Tool::new(id).expect("valid command ID")
}

/// Four configured commands after the built-in Shell, in configured order.
pub fn configure(app: &mut App) {
    let mut configuration = std::collections::BTreeMap::from([(
        "commands".to_string(),
        "claude,codex,copilot,hermes".to_string(),
    )]);
    for (id, label) in [
        ("claude", "Claude"),
        ("codex", "Codex"),
        ("copilot", "Copilot"),
        ("hermes", "Hermes"),
    ] {
        configuration.insert(format!("command_{id}"), id.into());
        configuration.insert(format!("label_{id}"), label.into());
    }
    app.configure(&configuration);
}

/// Indexed directories, including spaces, Unicode and shell-looking literals.
pub const DIRECTORIES: [&str; 6] = [
    "/home/example/Projects/launchpad",
    "/home/example/Projects/notes",
    "/home/example/Projects/service",
    "/home/example/Projects/team notes",
    "/home/example/Projects/修理",
    "/home/example/Projects/it's literal; $HOME",
];

/// Ten distinct recent directories, newest first. The oldest row deliberately
/// carries a shell-looking name so narrow layouts can be checked against it.
pub fn history() -> Vec<Launch> {
    [
        ("claude", "launchpad", "12m ago"),
        ("shell", "notes", "38m ago"),
        ("codex", "service", "1h ago"),
        ("hermes", "team notes", "2h ago"),
        ("claude", "修理", "3h ago"),
        ("copilot", "api", "4h ago"),
        ("shell", "current", "Yesterday"),
        ("codex", "docs", "Yesterday"),
        ("claude", "scratch", "Yesterday"),
        ("hermes", "it's literal; $HOME", "2d ago"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (id, name, age))| Launch {
        id: i as u64,
        path: format!("{HOME}/Projects/{name}"),
        tool: tool(id),
        age: age.into(),
    })
    .collect()
}

/// Settle the outstanding worker query with `paths`, returning its generation.
pub fn results(app: &mut App, paths: &[&str]) -> u64 {
    let Some(RemoteRequest::Query { generation, .. }) = app.take_remote_request() else {
        panic!("expected a pending worker query");
    };
    assert!(app.apply_remote_results(generation, paths.iter().map(|p| (*p).into()).collect()));
    generation
}

/// Answer the outstanding validation.
pub fn validate(app: &mut App, result: Result<String, String>) {
    let Some(RemoteRequest::Validate { generation, .. }) = app.take_remote_request() else {
        panic!("expected a pending validation");
    };
    assert!(app.finish_remote_validation(generation, result));
}

/// A settled dashboard: six suggestions, ten recent rows, Shell selected.
pub fn app() -> App {
    let mut app = App::from_remote(HOME.into());
    configure(&mut app);
    app.update(Action::Clear);
    app.update(Action::Text("notes".into()));
    results(&mut app, &DIRECTORIES);
    app.history = history();
    app
}

use std::collections::BTreeMap;
use zellij_launchpad::State;
use zellij_launchpad_core::app::{Action, Tool};
use zellij_tile::prelude::Event;

fn config() -> BTreeMap<String, String> {
    [
        ("commands", "worktree,bad"),
        ("command_worktree", "claude"),
        ("arguments_worktree", "-w"),
        ("label_worktree", "Claude in Worktree"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect()
}
#[test]
fn configured_labels_and_errors_are_visible_in_dashboard_and_help() {
    let mut s = State::default();
    s.app.configure(&config());
    let frame = s.render_frame(24, 80);
    assert!(frame.contains("Claude in Worktree"), "{frame}");
    assert!(frame.contains("Config error"), "{frame}");
    s.app.update(Action::Help);
    let frame = s.render_frame(60, 160);
    assert!(frame.contains("Claude in Worktree"));
    assert!(frame.contains("command_bad"));
    assert!(!frame.contains("all five"));
    assert!(!frame.contains("no flags"));
    assert!(!frame.contains("No persistent"));
}

#[test]
fn configured_commands_and_diagnostics_survive_home_remount_and_form_reset() {
    let mut s = State::default();
    s.app.configure(&config());
    s.prepare_home(Some("/home/fixture".into()));
    s.handle(Event::HostFolderChanged("/home/fixture".into()));
    for _ in 0..2 {
        assert_eq!(s.app.visible_tools().len(), 2);
        assert_eq!(s.app.commands.entries[1].arguments, vec!["-w"]);
        assert_eq!(s.app.commands.errors.len(), 1);
        assert_eq!(s.app.tool, Tool::Shell);
        s.app.update(Action::Right);
        s.app.update(Action::Reset);
    }
}

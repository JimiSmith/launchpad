use launchpad_plugin::{launch_tab, tab_name};
use zellij_tile::prelude::{PaneInfo, SessionInfo, TabInfo};

#[test]
fn maps_originating_plugin_to_stable_id_not_active_tab_position_or_terminal_id() {
    let mut session = SessionInfo {
        tabs: vec![
            TabInfo {
                tab_id: 88,
                position: 0,
                active: true,
                name: "manual-other".into(),
                ..Default::default()
            },
            TabInfo {
                tab_id: 41,
                position: 1,
                active: false,
                name: "origin".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    session.panes.panes.insert(
        0,
        vec![PaneInfo {
            id: 7,
            is_plugin: false,
            ..Default::default()
        }],
    );
    session.panes.panes.insert(
        1,
        vec![PaneInfo {
            id: 7,
            is_plugin: true,
            ..Default::default()
        }],
    );
    assert_eq!(launch_tab(&session, 7).unwrap().tab_id, 41);
    session.tabs[0].position = 1;
    session.tabs[1].position = 0;
    let other = session.panes.panes.remove(&0).unwrap();
    let origin = session.panes.panes.remove(&1).unwrap();
    session.panes.panes.insert(0, origin);
    session.panes.panes.insert(1, other);
    assert_eq!(launch_tab(&session, 7).unwrap().tab_id, 41);
    assert!(launch_tab(&session, 8).is_none());
}

#[test]
fn names_preserve_basename_and_configured_label_bytes_with_root_and_home_fallbacks() {
    for (path, label, expected) in [
        ("/home/owned/notes", "Shell", "notes · Shell"),
        (
            "/home/owned/my-app/",
            "Claude in Worktree",
            "my-app · Claude in Worktree",
        ),
        ("/home/owned", "Shell", "owned · Shell"),
        ("/", "Shell", "/ · Shell"),
        (
            "/home/owned/修理 e\u{301}; $HOME",
            "👩🏽‍💻",
            "修理 e\u{301}; $HOME · 👩🏽‍💻",
        ),
    ] {
        assert_eq!(tab_name(path, label), expected);
    }
}

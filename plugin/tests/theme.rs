use std::collections::BTreeMap;

use zellij_launchpad::State;
use zellij_launchpad_core::app::{Action, App};
use zellij_tile::prelude::Event;

#[test]
fn configured_theme_and_errors_survive_remounts_and_resets_in_ansi_output() {
    let mut state = State::default();
    state.app.configure(&BTreeMap::from([
        ("theme_background".into(), "#010203".into()),
        ("theme_accent".into(), "#040506".into()),
        ("theme_text".into(), "invalid".into()),
    ]));
    let theme = state.app.theme;
    let errors = state.app.theme_errors.clone();
    assert!(state.prepare_home(Some("/home/fixture".into())));
    state.handle(Event::HostFolderChanged("/home/fixture".into()));
    for _ in 0..2 {
        assert_eq!(state.app.theme, theme);
        assert_eq!(state.app.theme_errors, errors);
        let frame = state.render_frame(24, 80);
        assert!(frame.contains("48;2;1;2;3m"));
        assert!(frame.contains("38;2;4;5;6m"));
        assert!(frame.contains("Config error: theme_text"));
        state.app.update(Action::Reset);
    }
    state.replace_app(App::from_remote("/tmp/project".into()));
    assert_eq!(state.app.theme, theme);
    assert_eq!(state.app.theme_errors, errors);
    state.app.update(Action::Help);
    assert!(state
        .render_frame(60, 160)
        .contains("theme_text: expected #RRGGBB"));
}

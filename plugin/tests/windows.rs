use std::collections::BTreeMap;
use zellij_launchpad::{session_home, tab_name, State};

#[test]
fn home_comes_from_the_host_session_including_windows_fallbacks() {
    let mut env = BTreeMap::from([
        ("USERPROFILE".into(), r"C:\Users\Ada".into()),
        ("HOMEDRIVE".into(), "D:".into()),
        ("HOMEPATH".into(), r"\Ada".into()),
    ]);
    assert_eq!(session_home(&env).as_deref(), Some(r"C:\Users\Ada"));
    env.insert("HOME".into(), "C:/custom/home".into());
    assert_eq!(session_home(&env).as_deref(), Some("C:/custom/home"));
    env.insert("HOME".into(), String::new());
    env.remove("USERPROFILE");
    assert_eq!(session_home(&env).as_deref(), Some(r"D:\Ada"));
    env.remove("HOMEDRIVE");
    assert!(session_home(&env).is_none());
    env.insert("UserProfile".into(), r"C:\Users\Ada".into());
    assert_eq!(session_home(&env).as_deref(), Some(r"C:\Users\Ada"));
}

#[test]
fn windows_home_bootstraps_the_adapter_and_tab_names_use_the_basename() {
    for home in [r"C:\Users\Ada", "C:/Users/Ada", r"\\server\share\Ada"] {
        let mut state = State::default();
        assert!(state.prepare_home(Some(home.into())), "{home}");
        state.handle(zellij_tile::prelude::Event::HostFolderChanged(home.into()));
        assert_eq!(state.app.path_label(home), "~");
    }
    assert_eq!(tab_name(r"C:\Users\Ada\notes", "Shell"), "notes · Shell");
    assert_eq!(tab_name(r"\\server\share\notes", "Tool"), "notes · Tool");
}

#[test]
fn worker_and_remount_accept_separator_variations_but_reject_other_directories() {
    use zellij_launchpad::workers::{Engine, Reply, Request};
    let mut worker = Engine::default();
    let start = || Request::Start {
        epoch: 1,
        home: "C:/Users/Ada".into(),
    };
    assert!(matches!(
        worker.handle(start(), r"C:\Users\Ada\"),
        Reply::Ready { .. }
    ));
    assert!(matches!(
        worker.handle(start(), r"C:\Users\Adam"),
        Reply::Failed { .. }
    ));
    let mut state = State::default();
    assert!(state.prepare_home(Some("C:/Users/Ada".into())));
    state.handle(zellij_tile::prelude::Event::HostFolderChanged(
        r"C:\Users\Ada".into(),
    ));
    assert_eq!(state.app.path_label(r"C:\Users\Ada\notes"), "~/notes");
}

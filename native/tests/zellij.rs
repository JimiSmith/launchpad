#[cfg(unix)]
use launchpad::zellij::Failure;
use launchpad::zellij::{Zellij, supported_version};
use launchpad_core::commands::{Command, Tool};
use std::{path::PathBuf, time::Duration};
#[cfg(unix)]
static FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(unix)]
struct Fixture {
    root: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}
#[cfg(unix)]
impl Fixture {
    fn new(name: &str, body: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        // A concurrent spawn can inherit another fixture's open write handle
        // before exec closes it, making that script fail with ETXTBSY.
        let guard = FIXTURE_LOCK.lock().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/native-cli-tests")
            .join(format!("{}-{name}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("zellij");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            _guard: guard,
        }
    }
    fn host(&self) -> Zellij {
        Zellij {
            executable: self.root.join("zellij"),
            session: "test session".into(),
            pane_id: 7,
            // Allow for process startup and scheduling delays on CI runners.
            // Tests of deadline behavior opt into a short timeout explicitly.
            timeout: Duration::from_secs(2),
        }
    }
}
#[cfg(unix)]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn tool() -> Command {
    Command {
        id: Tool::new("fixture").unwrap(),
        label: "Fixture".into(),
        executable: Some("fixture".into()),
        arguments: vec!["".into(), "a b".into(), "$HOME".into(), ";".into()],
        shortcut: None,
    }
}
#[test]
fn argument_boundaries_and_explicit_pane_target_are_preserved() {
    let host = Zellij {
        executable: PathBuf::from("zellij"),
        session: "test session".into(),
        pane_id: 7,
        timeout: Duration::from_millis(200),
    };
    assert_eq!(
        host.launch_args("/tmp/a b;$HOME", &tool()),
        [
            "action",
            "new-pane",
            "--in-place",
            "--close-replaced-pane",
            "--pane-id",
            "terminal_7",
            "--cwd",
            "/tmp/a b;$HOME",
            "--close-on-exit",
            "--",
            "fixture",
            "",
            "a b",
            "$HOME",
            ";"
        ]
    );
}
#[test]
#[cfg(unix)]
fn missing_command_success_status_is_rejected_only_after_origin_readback() {
    let f = Fixture::new(
        "missing",
        r#"if [ "$4" = "list-panes" ]; then
printf '%s\n' '[{"id":9,"is_plugin":false,"tab_id":1,"tab_name":"Neighbor"},{"id":7,"is_plugin":false,"tab_id":2,"tab_name":"Own"}]'
fi"#,
    );
    assert_eq!(f.host().origin().unwrap().tab_name, "Own");
    assert!(matches!(
        f.host().launch("/tmp", &tool()),
        Err(Failure::Rejected(_))
    ));
}
#[test]
#[cfg(unix)]
fn malformed_or_missing_pane_information_never_targets_neighbor() {
    for (name, body) in [("bad", "echo malformed"), ("empty", "echo '[]'")] {
        let f = Fixture::new(name, body);
        assert!(matches!(f.host().origin(), Err(Failure::Unknown(_))));
    }
}
#[test]
#[cfg(unix)]
fn no_response_is_unknown_and_not_safe_to_retry() {
    let f = Fixture::new("timeout", "while :; do :; done");
    let mut host = f.host();
    host.timeout = Duration::from_millis(200);
    let result = host.launch("/tmp", &tool());
    assert!(matches!(result, Err(Failure::Unknown(_))), "{result:?}");
}
#[test]
#[cfg(unix)]
fn empty_pane_response_is_retried_without_repeating_the_launch() {
    let f = Fixture::new(
        "starting",
        r#"state="${0%/*}/ready"
if [ "$4" = "list-panes" ]; then
    if [ ! -f "$state" ]; then
        : > "$state"
        exit 0
    fi
    printf '%s\n' '[{"id":7,"is_plugin":false,"tab_id":2,"tab_name":"Own"}]'
else
    printf 'launch\n' >> "${0%/*}/launches"
fi"#,
    );
    let result = f.host().launch("/tmp", &tool());
    assert!(matches!(result, Err(Failure::Rejected(_))), "{result:?}");
    assert!(f.root.join("ready").exists());
    assert_eq!(
        std::fs::read_to_string(f.root.join("launches")).unwrap(),
        "launch\n"
    );
    assert_eq!(f.host().origin().unwrap().tab_name, "Own");
}
#[test]
#[cfg(unix)]
fn persistently_empty_pane_response_obeys_the_deadline() {
    let f = Fixture::new("empty-response", "exit 0");
    let mut host = f.host();
    host.timeout = Duration::from_millis(200);
    let started = std::time::Instant::now();
    let result = host.origin();
    assert!(matches!(result, Err(Failure::Unknown(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_millis(800));
}
#[test]
#[cfg(unix)]
fn interrupted_cli_is_unknown_and_must_not_roll_back_a_launch() {
    for signal in ["HUP", "TERM", "INT"] {
        let f = Fixture::new(&format!("signal-{signal}"), &format!("kill -{signal} $$"));
        let result = f.host().launch("/tmp", &tool());
        assert!(matches!(result, Err(Failure::Unknown(_))), "{result:?}");
    }
}
#[test]
#[cfg(unix)]
fn explicit_cli_error_is_still_a_rejection() {
    let f = Fixture::new("rejected", "echo 'action rejected' >&2\nexit 1");
    let result = f.host().launch("/tmp", &tool());
    assert!(matches!(result, Err(Failure::Rejected(_))), "{result:?}");
}
#[test]
fn supported_versions() {
    assert!(supported_version("zellij 0.45.0\n"));
    assert!(supported_version("zellij 0.45.1"));
    assert!(!supported_version("zellij 0.44.0"));
    assert!(!supported_version("garbage"));
}

#[test]
#[cfg(unix)]
fn command_output_deadline_includes_inherited_pipes() {
    let f = Fixture::new("pipe-timeout", "/bin/sleep 1 &\nexit 0");
    let mut host = f.host();
    host.timeout = Duration::from_millis(200);
    let started = std::time::Instant::now();
    let result = host.launch("/tmp", &tool());
    assert!(matches!(result, Err(Failure::Unknown(_))), "{result:?}");
    assert!(started.elapsed() < Duration::from_millis(800));
}

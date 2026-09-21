use std::{path::PathBuf, time::Duration};
#[cfg(unix)]
use zellij_launchpad::zellij::Failure;
use zellij_launchpad::zellij::{Zellij, supported_version};
use zellij_launchpad_core::commands::{Command, Tool};
#[cfg(unix)]
struct Fixture(PathBuf);
#[cfg(unix)]
impl Fixture {
    fn new(name: &str, body: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/native-cli-tests")
            .join(format!("{}-{name}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("zellij");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
    fn host(&self) -> Zellij {
        Zellij {
            executable: self.0.join("zellij"),
            session: "test session".into(),
            pane_id: 7,
            timeout: Duration::from_millis(200),
        }
    }
}
#[cfg(unix)]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tool() -> Command {
    Command {
        id: Tool::new("fixture").unwrap(),
        label: "Fixture".into(),
        executable: Some("fixture".into()),
        arguments: vec!["".into(), "a b".into(), "$HOME".into(), ";".into()],
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
    assert!(matches!(
        f.host().launch("/tmp", &tool()),
        Err(Failure::Unknown(_))
    ));
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
    let started = std::time::Instant::now();
    assert!(matches!(
        f.host().launch("/tmp", &tool()),
        Err(Failure::Unknown(_))
    ));
    assert!(started.elapsed() < Duration::from_millis(800));
}

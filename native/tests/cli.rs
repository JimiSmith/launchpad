use std::process::Command;
#[test]
fn help_and_version_work_without_a_host_and_normal_invocation_requires_one() {
    for argument in ["--help", "--version"] {
        let result = Command::new(env!("CARGO_BIN_EXE_zellij-launchpad"))
            .env_clear()
            .arg(argument)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(String::from_utf8_lossy(&result.stdout).contains("zellij-launchpad"));
        assert!(!String::from_utf8_lossy(&result.stdout).contains("shell-handoff"));
    }
    let result = Command::new(env!("CARGO_BIN_EXE_zellij-launchpad"))
        .env_clear()
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("requires an existing Zellij session or herdr pane")
    );
    assert!(
        !result.stdout.contains(&27),
        "no terminal escape output outside a host"
    );
}
#[test]
fn both_hosts_require_an_explicit_choice() {
    let result = Command::new(env!("CARGO_BIN_EXE_zellij-launchpad"))
        .env_clear()
        .env("ZELLIJ_SESSION_NAME", "outer")
        .env("HERDR_ENV", "1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("LAUNCHPAD_HOST"));
}
fn stderr(env: &[(&str, &str)], args: &[&str]) -> String {
    let result = Command::new(env!("CARGO_BIN_EXE_zellij-launchpad"))
        .env_clear()
        .envs(env.iter().copied())
        .args(args)
        .output()
        .unwrap();
    assert!(!result.status.success());
    String::from_utf8_lossy(&result.stderr).into_owned()
}
#[test]
fn launchpad_host_chooses_between_nested_hosts() {
    let both = [("ZELLIJ_SESSION_NAME", "outer"), ("HERDR_ENV", "1")];
    let zellij = stderr(&[both[0], both[1], ("LAUNCHPAD_HOST", "zellij")], &[]);
    assert!(zellij.contains("ZELLIJ_PANE_ID is missing"), "{zellij}");
    let herdr = stderr(&[both[0], both[1], ("LAUNCHPAD_HOST", "herdr")], &[]);
    assert!(herdr.contains("HERDR_PANE_ID is missing"), "{herdr}");
    let flag = stderr(
        &[both[0], both[1], ("LAUNCHPAD_HOST", "zellij")],
        &["--host", "herdr"],
    );
    assert!(
        flag.contains("HERDR_PANE_ID is missing"),
        "--host wins: {flag}"
    );
}
#[test]
fn only_herdr_env_1_counts_as_herdr() {
    let zellij = stderr(&[("ZELLIJ_SESSION_NAME", "s"), ("HERDR_ENV", "0")], &[]);
    assert!(zellij.contains("ZELLIJ_PANE_ID is missing"), "{zellij}");
    let neither = stderr(&[("HERDR_ENV", "0")], &[]);
    assert!(
        neither.contains("requires an existing Zellij session or herdr pane"),
        "{neither}"
    );
}
#[test]
#[cfg(unix)]
fn herdr_uses_its_bin_path_and_refuses_the_zellij_shell_handoff() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/native-cli-tests")
        .join(format!("{}-herdr-cli", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let script = root.join("fake-herdr");
    std::fs::write(
        &script,
        r#"#!/bin/sh
echo "$@" >> "${0%/*}/calls"
case "$1" in
pane) echo '{"result":{"pane":{"pane_id":"w1:p1","tab_id":"w1:t1"}}}';;
tab) echo '{"result":{"tab":{"label":"Own"}}}';;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = [
        ("HERDR_ENV", "1"),
        ("HERDR_PANE_ID", "w1:p1"),
        ("HERDR_BIN_PATH", script.to_str().unwrap()),
    ];
    // Discovery succeeds through the fake CLI; the test has no terminal.
    let normal = stderr(&env, &[]);
    assert!(
        normal.contains("requires an interactive terminal"),
        "{normal}"
    );
    assert!(
        std::fs::read_to_string(root.join("calls"))
            .unwrap()
            .contains("pane current --current")
    );
    let handoff = r#"{"config":"/c","explicit_config":false,"attempt":"a","tab_id":"w1:t1","previous_name":"p","attempted_name":"n"}"#;
    let handoff = stderr(&env, &["--shell-handoff", handoff]);
    assert!(
        handoff.contains("Shell handoff requires Zellij"),
        "{handoff}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

use std::process::Command;
#[test]
fn help_and_version_work_without_zellij_and_normal_invocation_requires_it() {
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
        String::from_utf8_lossy(&result.stderr).contains("requires an existing Zellij session")
    );
    assert!(
        !result.stdout.contains(&27),
        "no terminal escape output outside Zellij"
    );
}

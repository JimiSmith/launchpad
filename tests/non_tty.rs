use std::process::{Command, Stdio};
#[test]
fn rejects_non_tty_without_escape_sequences() {
    let out = Command::new(env!("CARGO_BIN_EXE_zellij-launchpad-prototype"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("interactive terminal"));
    assert!(!out.stdout.contains(&0x1b));
}

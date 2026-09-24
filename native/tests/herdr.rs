#![cfg(unix)]
use std::{path::PathBuf, sync::atomic::AtomicUsize, time::Duration};
use zellij_launchpad::{
    herdr::Herdr,
    host::{Failure, Host, Launched},
};
use zellij_launchpad_core::commands::{Command, Tool};

static FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// A fake herdr CLI that logs its argv and answers for pane w1:p2 in tab w1:t3.
struct Fixture {
    root: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}
const HERDR: &str = r#"dir="${0%/*}"
printf '%s|' "$@" >> "$dir/calls"; echo >> "$dir/calls"
label=$(cat "$dir/label" 2>/dev/null || printf Own)
case "$1 $2" in
"pane current") printf '%s\n' '{"id":"x","result":{"pane":{"pane_id":"w1:p2","tab_id":"w1:t3"},"type":"pane_current"}}';;
"tab get") printf '{"id":"x","result":{"tab":{"label":"%s","tab_id":"w1:t3"},"type":"tab_info"}}\n' "$label";;
"tab rename") [ -f "$dir/stuck" ] || printf '%s' "$4" > "$dir/label"
    printf '%s\n' '{"id":"x","result":{"type":"ok"}}';;
"pane close") [ -f "$dir/gone" ] && { printf '%s\n' '{"error":{"code":"pane_not_found","message":"pane w1:p2 not found"}}' >&2; exit 1; }
    printf '%s\n' '{"id":"x","result":{"type":"ok"}}';;
*) printf '%s\n' '{"error":{"code":"nope","message":"unsupported"},"id":"x"}' >&2; exit 1;;
esac"#;
impl Fixture {
    fn new(name: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let guard = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/native-cli-tests")
            .join(format!("{}-herdr-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("herdr");
        std::fs::write(&script, format!("#!/bin/sh\n{HERDR}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            _guard: guard,
        }
    }
    fn herdr(&self) -> Herdr {
        Herdr {
            executable: self.root.join("herdr"),
            timeout: Duration::from_secs(2),
        }
    }
    fn host(&self) -> Host {
        Host::Herdr(self.herdr())
    }
    fn replace(&self, body: &str) {
        std::fs::write(self.root.join("herdr"), format!("#!/bin/sh\n{body}\n")).unwrap();
    }
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Successful launches move this process into the tool's directory.
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn tool(executable: &str, arguments: &[&str]) -> Command {
    Command {
        id: Tool::new("fixture").unwrap(),
        label: "Fixture".into(),
        executable: Some(executable.into()),
        arguments: arguments.iter().map(|a| a.to_string()).collect(),
        shortcut: None,
    }
}
#[test]
fn origin_uses_the_callers_pane_context_and_its_tab_label() {
    let f = Fixture::new("origin");
    let origin = f.host().origin().unwrap();
    assert_eq!(
        (origin.tab_id.as_str(), origin.tab_name.as_str()),
        ("w1:t3", "Own")
    );
    assert_eq!(f.calls(), ["pane|current|--current|", "tab|get|w1:t3|"]);
}
#[test]
fn rename_passes_hyphenated_labels_verbatim_and_reads_them_back() {
    let f = Fixture::new("rename");
    let host = f.host();
    host.rename("w1:t3", "-x · Shell").unwrap();
    assert_eq!(host.origin().unwrap().tab_name, "-x · Shell");
    assert!(
        f.calls()
            .contains(&"tab|rename|w1:t3|-x · Shell|".to_string())
    );
}
#[test]
fn rename_that_does_not_stick_is_rejected() {
    let f = Fixture::new("stuck");
    std::fs::write(f.root.join("stuck"), "").unwrap();
    let result = f.host().rename("w1:t3", "a · Shell");
    assert!(matches!(result, Err(Failure::Rejected(_))), "{result:?}");
}
#[test]
fn cli_errors_report_herdrs_message() {
    let f = Fixture::new("error-message");
    f.replace(r#"echo '{"error":{"code":"pane_not_found","message":"pane gone"}}' >&2; exit 1"#);
    let error = f.host().origin().unwrap_err();
    assert!(matches!(error, Failure::Rejected(_)), "{error:?}");
    assert!(error.to_string().contains("pane gone"), "{error}");
}
#[test]
fn missing_executable_is_rejected_before_anything_runs() {
    let f = Fixture::new("missing");
    let result = f
        .host()
        .launch("/tmp", &tool("launchpad-no-such-tool", &[]));
    assert!(matches!(result, Err(Failure::Rejected(_))));
    let result = f
        .host()
        .launch(f.root.join("absent").to_str().unwrap(), &tool("true", &[]));
    assert!(matches!(result, Err(Failure::Rejected(_))));
    assert!(f.calls().is_empty(), "no pane closed on rejection");
}
#[test]
fn tool_runs_in_the_directory_with_literal_arguments_then_closes_the_pane() {
    let f = Fixture::new("run");
    let dir = f.root.join("a b;$HOME");
    std::fs::create_dir(&dir).unwrap();
    let host = f.host();
    let Launched::Running(child) = host
        .launch(
            dir.to_str().unwrap(),
            &tool(
                "sh",
                &[
                    "-c",
                    r#"pwd > out; printf '%s|' "$@" >> out"#,
                    "sh",
                    "",
                    "a b",
                    "$HOME",
                    ";",
                ],
            ),
        )
        .unwrap()
    else {
        panic!("herdr launches run as a child");
    };
    host.finish(child, &AtomicUsize::new(0)).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("out")).unwrap(),
        format!("{}\n|a b|$HOME|;|", dir.display())
    );
    assert_eq!(f.calls(), ["pane|current|--current|", "pane|close|w1:p2|"]);
}
#[test]
fn failing_tool_still_closes_the_pane() {
    let f = Fixture::new("fail");
    let host = f.host();
    let Launched::Running(child) = host.launch("/tmp", &tool("false", &[])).unwrap() else {
        panic!("herdr launches run as a child");
    };
    host.finish(child, &AtomicUsize::new(0)).unwrap();
    assert_eq!(f.calls().last().unwrap(), "pane|close|w1:p2|");
}
#[test]
fn non_json_errors_are_rejections_with_the_raw_text() {
    let f = Fixture::new("raw-error");
    f.replace("echo 'socket unavailable' >&2; exit 2");
    let error = f.host().origin().unwrap_err();
    assert!(matches!(error, Failure::Rejected(_)), "{error:?}");
    assert!(error.to_string().contains("socket unavailable"), "{error}");
}
#[test]
fn interrupted_timed_out_or_malformed_calls_are_unknown() {
    for (name, body) in [
        ("signal", "kill -TERM $$"),
        ("timeout", "while :; do :; done"),
        ("garbage", "echo not-json"),
    ] {
        let f = Fixture::new(name);
        f.replace(body);
        let mut herdr = f.herdr();
        herdr.timeout = Duration::from_millis(300);
        let result = herdr.origin();
        assert!(
            matches!(result, Err(Failure::Unknown(_))),
            "{name}: {result:?}"
        );
    }
}
#[test]
fn a_pane_that_is_already_gone_is_reported_after_the_tool_is_reaped() {
    let f = Fixture::new("gone");
    std::fs::write(f.root.join("gone"), "").unwrap();
    let host = f.host();
    let Launched::Running(child) = host.launch("/tmp", &tool("true", &[])).unwrap() else {
        panic!("herdr launches run as a child");
    };
    let pid = child.id();
    let error = host.finish(child, &AtomicUsize::new(0)).unwrap_err();
    assert!(error.contains("pane w1:p2 not found"), "{error}");
    // SAFETY: probe only. A reaped child no longer exists under its PID.
    let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    assert!(!alive, "the tool was waited for before closing");
}
#[test]
fn termination_signals_are_forwarded_to_the_tool() {
    let f = Fixture::new("forward");
    let host = f.host();
    let Launched::Running(child) = host.launch("/tmp", &tool("sleep", &["30"])).unwrap() else {
        panic!("herdr launches run as a child");
    };
    let started = std::time::Instant::now();
    host.finish(child, &AtomicUsize::new(libc::SIGTERM as usize))
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(f.calls().last().unwrap(), "pane|close|w1:p2|");
}
#[test]
fn launchpad_follows_the_tool_into_its_directory_but_not_on_failure() {
    let f = Fixture::new("cwd");
    let dir = f.root.join("target dir");
    std::fs::create_dir(&dir).unwrap();
    let before = std::env::current_dir().unwrap();
    let result = f
        .host()
        .launch(dir.to_str().unwrap(), &tool("launchpad-no-such-tool", &[]));
    assert!(matches!(result, Err(Failure::Rejected(_))));
    assert_eq!(
        std::env::current_dir().unwrap(),
        before,
        "restored after a failed start"
    );
    let host = f.host();
    let Launched::Running(child) = host
        .launch(dir.to_str().unwrap(), &tool("true", &[]))
        .unwrap()
    else {
        panic!("herdr launches run as a child");
    };
    assert_eq!(
        std::env::current_dir().unwrap(),
        dir.canonicalize().unwrap()
    );
    host.finish(child, &AtomicUsize::new(0)).unwrap();
}

#![cfg(unix)]
use launchpad::{
    herdr::Herdr,
    host::{Failure, Forward, Host, Launched, Origin},
};
use launchpad_core::commands::{Command, Tool};
use std::{path::PathBuf, time::Duration};

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
    [ -f "$dir/fail-once" ] && { rm "$dir/fail-once"; printf '%s\n' '{"error":{"code":"lost","message":"reply lost"}}' >&2; exit 1; }
    printf '%s\n' '{"id":"x","result":{"type":"ok"}}';;
"pane process-info") printf '{"id":"x","result":{"process_info":{"pane_id":"w1:p2","shell_pid":%s}}}\n' "$(cat "$dir/root" 2>/dev/null || echo 1)";;
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
        // Shells differ on whether `pwd` keeps the `..` in this path.
        let root = root.canonicalize().unwrap();
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
fn renames(f: &Fixture) -> Vec<String> {
    f.calls()
        .into_iter()
        .filter(|c| c.starts_with("tab|rename|"))
        .collect()
}
#[test]
fn name_tab_renames_only_when_enabled() {
    let f = Fixture::new("name-tab");
    let host = f.host();
    let original = host.origin().unwrap();
    let kept = host
        .name_tab(&original, "/home/ada/notes", "Fixture", false)
        .unwrap();
    assert_eq!(kept, "Own");
    assert!(renames(&f).is_empty());
    // Restoring an unchanged name makes no host calls at all.
    let before = f.calls().len();
    host.restore_name(&original, &kept).unwrap();
    assert_eq!(f.calls().len(), before);
    let named = host
        .name_tab(&original, "/home/ada/notes", "Fixture", true)
        .unwrap();
    assert_eq!(named, "notes · Fixture");
    assert_eq!(renames(&f), ["tab|rename|w1:t3|notes · Fixture|"]);
}
#[test]
fn a_rename_that_does_not_stick_rejects_the_launch_name() {
    let f = Fixture::new("name-tab-stuck");
    std::fs::write(f.root.join("stuck"), "").unwrap();
    let host = f.host();
    let original = host.origin().unwrap();
    let result = host.name_tab(&original, "/home/ada/notes", "Fixture", true);
    assert!(matches!(result, Err(Failure::Rejected(_))), "{result:?}");
    // The tab still shows its own name, so nothing is renamed back.
    assert_eq!(renames(&f), ["tab|rename|w1:t3|notes · Fixture|"]);
}
#[test]
fn a_rename_that_applies_but_fails_is_rolled_back() {
    let f = Fixture::new("name-tab-rollback");
    std::fs::write(f.root.join("fail-once"), "").unwrap();
    let host = f.host();
    let original = host.origin().unwrap();
    let result = host.name_tab(&original, "/home/ada/notes", "Fixture", true);
    assert!(
        matches!(&result, Err(Failure::Rejected(m)) if m.contains("reply lost")),
        "{result:?}"
    );
    assert_eq!(
        renames(&f),
        ["tab|rename|w1:t3|notes · Fixture|", "tab|rename|w1:t3|Own|"]
    );
    assert_eq!(host.origin().unwrap().tab_name, "Own");
}
#[test]
fn restore_name_undoes_only_the_attempted_name() {
    let f = Fixture::new("restore");
    let host = f.host();
    let original = Origin {
        tab_id: "w1:t3".into(),
        tab_name: "Own".into(),
    };
    // A different tab now shows the attempted name: not ours to restore.
    std::fs::write(f.root.join("label"), "notes · Fixture").unwrap();
    let moved = Origin {
        tab_id: "w1:t9".into(),
        ..original.clone()
    };
    host.restore_name(&moved, "notes · Fixture").unwrap();
    assert!(renames(&f).is_empty());
    // Someone renamed the tab since: leave their name alone.
    std::fs::write(f.root.join("label"), "Theirs").unwrap();
    host.restore_name(&original, "notes · Fixture").unwrap();
    assert!(renames(&f).is_empty());
    std::fs::write(f.root.join("label"), "notes · Fixture").unwrap();
    host.restore_name(&original, "notes · Fixture").unwrap();
    assert_eq!(renames(&f), ["tab|rename|w1:t3|Own|"]);
    assert_eq!(host.origin().unwrap().tab_name, "Own");
}
#[test]
fn plugin_action_opens_a_focused_tab_in_the_given_directory() {
    let f = Fixture::new("plugin-open");
    f.replace(
        r#"printf '%s|' "$@" >> "${0%/*}/calls"; echo >> "${0%/*}/calls"
case "$1" in
plugin) printf '%s\n' '{"id":"x","result":{"plugin_pane":{"pane":{"pane_id":"w1:p9","tab_id":"w1:t4"}},"type":"plugin_pane_opened"}}';;
*) printf '%s\n' '{"id":"x","result":{"type":"tab_info"}}';;
esac"#,
    );
    let herdr = f.herdr();
    herdr
        .open_plugin_tab("launchpad", Some("/srv/a b".as_ref()))
        .unwrap();
    herdr.open_plugin_tab("launchpad", None).unwrap();
    let open =
        "plugin|pane|open|--plugin|launchpad|--entrypoint|launchpad|--placement|tab|--focus|";
    // Attached herdr 0.9 clients only follow an explicit tab focus.
    let focus = "tab|focus|w1:t4|";
    assert_eq!(
        f.calls(),
        [
            format!("{open}--cwd|/srv/a b|"),
            focus.into(),
            open.into(),
            focus.into()
        ]
    );
}
#[test]
fn workspace_action_moves_the_plugin_pane_and_focuses_its_new_tab() {
    let f = Fixture::new("plugin-workspace");
    f.replace(
        r#"printf '%s|' "$@" >> "${0%/*}/calls"; echo >> "${0%/*}/calls"
case "$1 $2" in
"plugin pane") printf '%s\n' '{"result":{"plugin_pane":{"pane":{"pane_id":"w1:p9","tab_id":"w1:t4"}}}}';;
"pane move") printf '%s\n' '{"result":{"move_result":{"pane":{"pane_id":"w8:p1","tab_id":"w8:t1"}}}}';;
"tab focus") printf '%s\n' '{"result":{}}';;
*) exit 1;;
esac"#,
    );
    // Exercise the real action entrypoint and directory fallback, including
    // characters that must remain literal CLI arguments.
    let cwd = "/srv/a b;$HOME";
    let context = serde_json::json!({"focused_pane_cwd": cwd, "workspace_cwd": "/srv"});
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_launchpad"))
        .env_clear()
        .env("HERDR_PLUGIN_ID", "launchpad")
        .env("HERDR_BIN_PATH", f.root.join("herdr"))
        .env("HERDR_PLUGIN_CONTEXT_JSON", context.to_string())
        .arg("--herdr-plugin-open-workspace")
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let open =
        "plugin|pane|open|--plugin|launchpad|--entrypoint|launchpad|--placement|tab|--no-focus|";
    // Like the new-tab action, leave the initial tab name to Herdr.
    let moved = "pane|move|w1:p9|--new-workspace|--focus|";
    let focus = "tab|focus|w8:t1|";
    assert_eq!(
        f.calls(),
        [format!("{open}--cwd|{cwd}|"), moved.into(), focus.into()]
    );
    std::fs::remove_file(f.root.join("calls")).unwrap();
    f.herdr().open_plugin_workspace("launchpad", None).unwrap();
    assert_eq!(f.calls(), [open, moved, focus]);
}
#[test]
fn workspace_action_stops_on_failure_without_retrying_or_closing_panes() {
    for stage in ["plugin", "pane", "tab"] {
        let f = Fixture::new(&format!("plugin-workspace-fail-{stage}"));
        f.replace(&format!(
            r#"printf '%s|' "$@" >> "${{0%/*}}/calls"; echo >> "${{0%/*}}/calls"
if [ "$1" = "{stage}" ]; then
    printf '%s\n' '{{"error":{{"message":"fixture rejection"}}}}' >&2; exit 1
fi
case "$1" in
plugin) printf '%s\n' '{{"result":{{"plugin_pane":{{"pane":{{"pane_id":"w1:p9","tab_id":"w1:t4"}}}}}}}}';;
pane) printf '%s\n' '{{"result":{{"move_result":{{"pane":{{"pane_id":"w8:p1","tab_id":"w8:t1"}}}}}}}}';;
esac"#
        ));
        let error = f
            .herdr()
            .open_plugin_workspace("launchpad", None)
            .unwrap_err();
        assert!(matches!(error, Failure::Rejected(_)), "{error:?}");
        assert!(error.to_string().contains("fixture rejection"));
        let expected = match stage {
            "plugin" => 1,
            "pane" => 2,
            _ => 3,
        };
        assert_eq!(f.calls().len(), expected);
        assert!(f.calls().iter().all(|call| !call.contains("|close|")));
    }
}
#[test]
fn startup_does_not_read_a_temporary_tab_that_may_be_removed_by_a_move() {
    let f = Fixture::new("plugin-workspace-startup");
    f.replace(
        r#"printf '%s|' "$@" >> "${0%/*}/calls"; echo >> "${0%/*}/calls"
if [ "$1 $2" = "pane current" ]; then
    printf '%s\n' '{"result":{"pane":{"pane_id":"w1:p9","tab_id":"w1:t4"}}}'
else
    printf '%s\n' '{"error":{"message":"temporary tab removed"}}' >&2; exit 1
fi"#,
    );
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_launchpad"))
        .env_clear()
        .env("HERDR_ENV", "1")
        .env("HERDR_PANE_ID", "w1:p9")
        .env("HERDR_BIN_PATH", f.root.join("herdr"))
        .output()
        .unwrap();
    // Host discovery succeeds; this noninteractive subprocess stops at the
    // terminal check instead of looking up the now-removed temporary tab.
    assert!(String::from_utf8_lossy(&result.stderr).contains("interactive terminal"));
    assert_eq!(f.calls(), ["pane|current|--current|"]);
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
    assert!(
        !f.calls().iter().any(|c| c.starts_with("pane|close")),
        "no pane closed on rejection"
    );
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
    host.finish(child, Forward::new().unwrap()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("out")).unwrap(),
        format!("{}\n|a b|$HOME|;|", dir.display())
    );
    assert_eq!(
        f.calls(),
        [
            "pane|current|--current|",
            "pane|process-info|--pane|w1:p2|",
            "pane|current|--current|",
            "pane|close|w1:p2|"
        ]
    );
}
#[test]
fn failing_tool_still_closes_the_pane() {
    let f = Fixture::new("fail");
    let host = f.host();
    let Launched::Running(child) = host.launch("/tmp", &tool("false", &[])).unwrap() else {
        panic!("herdr launches run as a child");
    };
    host.finish(child, Forward::new().unwrap()).unwrap();
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
    let error = host.finish(child, Forward::new().unwrap()).unwrap_err();
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
    // Registered before the signal, as main does at startup; this process
    // then survives SIGTERM and passes it on.
    let forward = Forward::new().unwrap();
    let sender = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(300));
        // SAFETY: plain syscall to this test process.
        unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
    });
    let started = std::time::Instant::now();
    host.finish(child, forward).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    sender.join().unwrap();
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
    host.finish(child, Forward::new().unwrap()).unwrap();
}
#[test]
fn only_the_panes_own_process_counts_as_its_root() {
    let f = Fixture::new("root");
    assert!(
        !f.herdr().is_pane_root(),
        "another process is the pane's shell"
    );
    std::fs::write(f.root.join("root"), std::process::id().to_string()).unwrap();
    assert!(f.herdr().is_pane_root());
    f.replace("exit 1");
    assert!(!f.herdr().is_pane_root(), "errors mean no");
}

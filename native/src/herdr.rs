use crate::host::{self, Failure, Forward, Launched, Origin, clean};
use launchpad_core::commands::Command as ToolCommand;
use serde::de::DeserializeOwned;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

/// herdr has no in-place pane replacement, so Launchpad runs tools as its own
/// child in its pane and closes that pane when the tool exits.
pub struct Herdr {
    pub executable: PathBuf,
    pub timeout: Duration,
}
#[derive(serde::Deserialize)]
struct Reply<T> {
    result: T,
}
#[derive(serde::Deserialize)]
struct PaneReply {
    pane: Pane,
}
#[derive(serde::Deserialize)]
struct Pane {
    pane_id: String,
    tab_id: String,
}
#[derive(serde::Deserialize)]
struct TabReply {
    tab: Tab,
}
#[derive(serde::Deserialize)]
struct Tab {
    label: String,
}
#[derive(serde::Deserialize)]
struct ErrorReply {
    error: ErrorBody,
}
#[derive(serde::Deserialize)]
struct ErrorBody {
    message: String,
}
impl Herdr {
    pub fn discover() -> Result<Self, String> {
        if std::env::var_os("HERDR_ENV").is_none_or(|v| v != "1") {
            return Err("Launchpad requires a herdr pane (HERDR_ENV=1). Run it in herdr.".into());
        }
        // `--current` resolves this variable; without it herdr would fall back
        // to whichever pane a client has focused.
        if std::env::var_os("HERDR_PANE_ID").is_none_or(|v| v.is_empty()) {
            return Err("HERDR_PANE_ID is missing; run Launchpad in a herdr pane.".into());
        }
        let host = Self {
            executable: std::env::var_os("HERDR_BIN_PATH")
                .filter(|p| !p.is_empty())
                .map_or_else(|| "herdr".into(), PathBuf::from),
            timeout: Duration::from_secs(5),
        };
        host.origin().map_err(|e| e.to_string())?;
        Ok(host)
    }
    fn call<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T, Failure> {
        let mut command = Command::new(&self.executable);
        command.args(args);
        // Keep the CLI out of the terminal's foreground group, so closing
        // this pane cannot hang it up before herdr replies.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let output = host::run(command, "herdr", Instant::now() + self.timeout)?;
        if !output.status.success() {
            if output.status.code().is_none() {
                return Err(Failure::Unknown(format!(
                    "herdr CLI interrupted ({}). Quit/reopen; do not automatically retry.",
                    output.status
                )));
            }
            let message = serde_json::from_str::<ErrorReply>(&output.stderr)
                .map_or_else(|_| clean(&output.stderr), |e| clean(&e.error.message));
            return Err(Failure::Rejected(format!(
                "herdr rejected the action: {message}"
            )));
        }
        serde_json::from_str::<Reply<T>>(&output.stdout)
            .map(|reply| reply.result)
            .map_err(|e| Failure::Unknown(format!("Invalid herdr response: {e}")))
    }
    /// This process's pane ID, which changes if the pane moves.
    fn pane(&self) -> Result<Pane, Failure> {
        let PaneReply { pane } = self.call(&["pane", "current", "--current"])?;
        Ok(pane)
    }
    pub fn origin(&self) -> Result<Origin, Failure> {
        let pane = self.pane()?;
        let TabReply { tab } = self.call(&["tab", "get", &pane.tab_id])?;
        Ok(Origin {
            tab_id: pane.tab_id,
            tab_name: tab.label,
        })
    }
    pub fn rename(&self, tab_id: &str, name: &str) -> Result<(), Failure> {
        // The label is a trailing argument: herdr would keep a `--` separator
        // as part of it, and accepts labels that begin with a hyphen. The
        // readback below, not this reply, confirms the rename.
        let _: serde_json::Value = self.call(&["tab", "rename", tab_id, name])?;
        let origin = self.origin()?;
        if origin.tab_id != tab_id || origin.tab_name != name {
            return Err(Failure::Rejected(
                "Originating tab changed; no launch. Retry.".into(),
            ));
        }
        Ok(())
    }
    /// Whether Launchpad is the pane's own process, as when it is herdr's
    /// default shell. Any doubt means no.
    pub fn is_pane_root(&self) -> bool {
        #[derive(serde::Deserialize)]
        struct Reply {
            process_info: ProcessInfo,
        }
        #[derive(serde::Deserialize)]
        struct ProcessInfo {
            shell_pid: u32,
        }
        self.pane()
            .and_then(|pane| self.call::<Reply>(&["pane", "process-info", "--pane", &pane.pane_id]))
            .is_ok_and(|reply| reply.process_info.shell_pid == std::process::id())
    }
    /// Starts the tool in `path` on this terminal. A missing executable or
    /// directory fails here, before anything is committed.
    ///
    /// When Launchpad is the pane's own process on Unix, the tool replaces it
    /// in place, as `exec` does: herdr then tracks the tool's cwd exactly as
    /// for its own shells and closes the pane when it exits. Only a failure
    /// returns.
    pub fn launch(&self, path: &str, tool: &ToolCommand) -> Result<Launched, Failure> {
        #[cfg(unix)]
        if self.is_pane_root() {
            use std::os::unix::process::CommandExt;
            let (program, mut command) = tool_command(tool);
            let error = command.current_dir(path).env("PWD", path).exec();
            return Err(Failure::Rejected(format!(
                "Cannot start {program} in {path}: {error}"
            )));
        }
        self.spawn(path, tool).map(Launched::Running)
    }
    fn spawn(&self, path: &str, tool: &ToolCommand) -> Result<Child, Failure> {
        let (program, mut command) = tool_command(tool);
        let fail = |e| Failure::Rejected(format!("Cannot start {program} in {path}: {e}"));
        #[cfg(windows)]
        windows::ignore_console_interrupts(true);
        // herdr falls back to the foreground group leader's cwd (Unix) or the
        // pane process's (Windows) when nothing reports one, and that is often
        // Launchpad. Move there too.
        let previous = std::env::current_dir().ok();
        let started = std::env::set_current_dir(path)
            .and_then(|()| command.current_dir(path).env("PWD", path).spawn());
        match started {
            Ok(child) => {
                // A report wins over process cwds, and the invoking shell may
                // have reported its own directory before starting Launchpad.
                report_cwd(path);
                Ok(child)
            }
            Err(e) => {
                if let Some(previous) = &previous {
                    let _ = std::env::set_current_dir(previous);
                }
                #[cfg(windows)]
                windows::ignore_console_interrupts(false);
                Err(fail(e))
            }
        }
    }
    /// Opens the plugin's Launchpad pane in a new, focused tab in `dir`.
    /// Without a directory herdr starts it in the plugin's own.
    pub fn open_plugin_tab(&self, plugin: &str, dir: Option<&Path>) -> Result<(), Failure> {
        #[derive(serde::Deserialize)]
        struct Opened {
            plugin_pane: PaneReply,
        }
        let mut args = vec![
            "plugin",
            "pane",
            "open",
            "--plugin",
            plugin,
            "--entrypoint",
            "launchpad",
            "--placement",
            "tab",
            "--focus",
        ];
        if let Some(dir) = dir.and_then(Path::to_str) {
            args.extend(["--cwd", dir]);
        }
        let opened: Opened = self.call(&args)?;
        // herdr 0.9 focuses the tab on the server but leaves attached
        // clients showing the previous one until a tab is focused.
        let tab = opened.plugin_pane.pane.tab_id;
        self.call::<serde_json::Value>(&["tab", "focus", &tab])
            .map(|_| ())
    }
    /// Waits for the tool, passing on termination signals Launchpad receives,
    /// then closes this pane as Zellij's close-on-exit would.
    pub fn finish(&self, mut child: Child, forward: Forward) -> Result<(), String> {
        let waited = forward
            .wait(&mut child)
            .map_err(|e| format!("Wait for tool: {e}"));
        let closed = self
            .pane()
            .and_then(|pane| self.call::<serde_json::Value>(&["pane", "close", &pane.pane_id]))
            .map(|_| ())
            .map_err(|e| format!("Close pane: {e}"));
        waited.and(closed)
    }
}

/// The herdr plugin's `open` action, run by herdr detached in the plugin's
/// directory with the plugin's environment.
pub fn plugin_open_action() -> Result<(), String> {
    let plugin = std::env::var("HERDR_PLUGIN_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .ok_or("HERDR_PLUGIN_ID is missing; herdr runs this as a plugin action.")?;
    let host = Herdr {
        executable: std::env::var_os("HERDR_BIN_PATH")
            .filter(|p| !p.is_empty())
            .map_or_else(|| "herdr".into(), PathBuf::from),
        timeout: Duration::from_secs(5),
    };
    let dir = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|json| plugin_context_dir(&json));
    host.open_plugin_tab(&plugin, dir.as_deref())
        .map_err(|e| e.to_string())
}
/// The focused pane's directory from herdr's plugin invocation context,
/// else the workspace's, if absolute.
pub fn plugin_context_dir(json: &str) -> Option<PathBuf> {
    #[derive(serde::Deserialize)]
    struct Context {
        focused_pane_cwd: Option<PathBuf>,
        workspace_cwd: Option<PathBuf>,
    }
    let context = serde_json::from_str::<Context>(json).ok()?;
    [context.focused_pane_cwd, context.workspace_cwd]
        .into_iter()
        .flatten()
        .find(|path| path.is_absolute())
}
/// The tool's program name and command, without a directory.
fn tool_command(tool: &ToolCommand) -> (String, Command) {
    // `default_shell` from Launchpad's config arrives as Shell's executable.
    let (program, arguments) = match &tool.executable {
        Some(executable) => (executable.clone(), &tool.arguments[..]),
        None => {
            // herdr's login-shell mode sets $SHELL to its default shell,
            // which may be Launchpad itself.
            let own = std::env::current_exe().ok();
            let not_launchpad = |shell: &String| !names_launchpad(shell, own.as_deref());
            let env_shell = std::env::var("SHELL").ok().filter(not_launchpad);
            let login = login_shell().filter(not_launchpad);
            (
                fallback_shell([env_shell, login], cfg!(windows), on_path),
                &[][..],
            )
        }
    };
    let mut command = Command::new(&program);
    command.args(arguments);
    // herdr's login-shell mode starts Launchpad as a login shell, so it never
    // read the login profile; the shell it hands over to must.
    #[cfg(unix)]
    if tool.id == launchpad_core::commands::Tool::Shell
        && std::env::args_os()
            .next()
            .is_some_and(|name| is_login_name(&name))
    {
        use std::os::unix::process::CommandExt;
        command.arg0(login_name(&program));
    }
    (program, command)
}
/// Whether a program name marks a login shell, which by convention begins
/// with `-`.
pub fn is_login_name(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'-')
}
/// The login-shell name for a shell: its file name prefixed with `-`.
pub fn login_name(program: &str) -> String {
    format!("-{}", program.rsplit('/').next().unwrap_or(program))
}
/// Tells herdr the tool's directory, as a shell prompt would, when stdout is
/// the pane's terminal.
fn report_cwd(path: &str) {
    use std::io::{IsTerminal, Write};
    let mut stdout = std::io::stdout();
    if stdout.is_terminal() {
        let _ = stdout
            .write_all(cwd_report(path, cfg!(windows)).as_bytes())
            .and_then(|()| stdout.flush());
    }
}
/// OSC 7 with an empty host on Unix (herdr ignores other hosts), and OSC 9;9
/// with a bare path on Windows.
pub fn cwd_report(path: &str, windows: bool) -> String {
    if windows {
        return format!("\x1b]9;9;{path}\x1b\\");
    }
    let mut uri = String::from("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("\x1b]7;{uri}\x1b\\")
}
/// Whether a shell names this executable, by file name without `.exe`,
/// ignoring case.
pub fn names_launchpad(shell: &str, own: Option<&std::path::Path>) -> bool {
    let stem = |name: &str| {
        let name = name.trim().rsplit(['/', '\\']).next().unwrap_or("");
        let name = name.to_ascii_lowercase();
        name.strip_suffix(".exe").unwrap_or(&name).to_string()
    };
    let own = own
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(stem);
    own.is_some_and(|own| !own.is_empty() && own == stem(shell))
}
/// Shell's program when no `default_shell` is configured: `$SHELL`, else the
/// account's login shell, else PowerShell 7, Windows PowerShell or bash, then
/// `/bin/sh`, by what is present.
pub fn fallback_shell(
    detected: [Option<String>; 2],
    windows: bool,
    present: impl Fn(&str) -> bool,
) -> String {
    if let Some(shell) = detected
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
    {
        return shell;
    }
    let (candidates, last): (&[&str], _) = if windows {
        (&["pwsh.exe"], "powershell.exe")
    } else {
        (&["bash"], "/bin/sh")
    };
    candidates
        .iter()
        .find(|c| present(c))
        .map_or(last, |c| c)
        .to_string()
}
/// The account's login shell from the user database, as macOS terminals use
/// when `$SHELL` is unusable.
#[cfg(unix)]
fn login_shell() -> Option<String> {
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: every pointer is valid for the call; `pw_shell` points into `buf`.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        )
    };
    if status != 0 || found.is_null() || entry.pw_shell.is_null() {
        return None;
    }
    // SAFETY: getpwuid_r succeeded, so `pw_shell` is a C string inside `buf`.
    let shell = unsafe { std::ffi::CStr::from_ptr(entry.pw_shell) };
    shell.to_str().ok().map(str::to_string)
}
#[cfg(not(unix))]
fn login_shell() -> Option<String> {
    None
}
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}
#[cfg(windows)]
mod windows {
    use windows_sys::{
        Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler},
        core::BOOL,
    };
    /// The tool shares this console, so Ctrl+C reaches both. Launchpad must
    /// survive it to close the pane later. Handlers are not inherited, so the
    /// tool keeps the default behaviour. `false` removes the handler again.
    pub fn ignore_console_interrupts(ignore: bool) {
        unsafe extern "system" fn handler(event: u32) -> BOOL {
            (event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT).into()
        }
        // SAFETY: (un)registers a static function that touches no state.
        unsafe { SetConsoleCtrlHandler(Some(handler), ignore.into()) };
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cwd_report, fallback_shell, is_login_name, login_name, names_launchpad, plugin_context_dir,
    };
    use std::path::Path;
    #[test]
    fn plugin_tabs_open_in_the_focused_panes_directory() {
        let context = r#"{"workspace_id":"w1","focused_pane_id":"w1:p2","focused_pane_cwd":"/usr","invocation_source":"keybinding"}"#;
        assert_eq!(plugin_context_dir(context), Some("/usr".into()));
        assert_eq!(plugin_context_dir(r#"{"focused_pane_cwd":null}"#), None);
        assert_eq!(plugin_context_dir(r#"{"focused_pane_cwd":"usr"}"#), None);
        assert_eq!(
            plugin_context_dir(r#"{"focused_pane_cwd":null,"workspace_cwd":"/srv"}"#),
            Some("/srv".into())
        );
        assert_eq!(plugin_context_dir(r#"{"workspace_id":"w1"}"#), None);
        assert_eq!(plugin_context_dir("not json"), None);
    }
    #[test]
    fn cwd_reports_use_forms_herdr_accepts() {
        assert_eq!(
            cwd_report("/home/a b/%;\x1b", false),
            "\x1b]7;file:///home/a%20b/%25%3B%1B\x1b\\"
        );
        assert_eq!(
            cwd_report("/tmp/é", false),
            "\x1b]7;file:///tmp/%C3%A9\x1b\\"
        );
        assert_eq!(
            cwd_report(r"C:\Users\Ada", true),
            "\x1b]9;9;C:\\Users\\Ada\x1b\\"
        );
    }
    #[test]
    fn launchpad_is_recognised_as_shell_by_file_name() {
        let own = Some(Path::new("/home/u/.local/bin/launchpad"));
        assert!(names_launchpad("/opt/bin/launchpad", own));
        assert!(!names_launchpad("/bin/zsh", own));
        assert!(!names_launchpad("", own));
        assert!(!names_launchpad("launchpad", None));
        let own = Some(Path::new(r"C:\Tools\launchpad.exe"));
        assert!(names_launchpad(r"C:\Tools\Launchpad.EXE", own));
    }
    #[test]
    fn shell_prefers_shell_env_then_login_shell_then_what_is_installed() {
        let all = |_: &str| true;
        let none = |_: &str| false;
        let zsh = || Some("/bin/zsh".to_string());
        assert_eq!(fallback_shell([zsh(), None], false, none), "/bin/zsh");
        assert_eq!(
            fallback_shell([Some("/bin/fish".into()), zsh()], false, none),
            "/bin/fish"
        );
        assert_eq!(
            fallback_shell([Some(" ".into()), zsh()], false, all),
            "/bin/zsh"
        );
        assert_eq!(fallback_shell([Some(" ".into()), None], false, all), "bash");
        assert_eq!(fallback_shell([None, None], false, none), "/bin/sh");
        assert_eq!(fallback_shell([None, None], true, all), "pwsh.exe");
        assert_eq!(fallback_shell([None, None], true, none), "powershell.exe");
        assert_eq!(
            fallback_shell([Some("C:/Git/bin/bash.exe".into()), None], true, all),
            "C:/Git/bin/bash.exe"
        );
    }
    #[cfg(unix)]
    #[test]
    fn login_shell_comes_from_the_user_database() {
        assert!(super::login_shell().is_some_and(|shell| shell.starts_with('/')));
    }
    #[test]
    fn login_shells_are_named_with_a_leading_hyphen() {
        assert!(is_login_name("-launchpad".as_ref()));
        assert!(!is_login_name("launchpad".as_ref()));
        assert!(!is_login_name("/usr/local/bin/launchpad".as_ref()));
        assert_eq!(login_name("/bin/zsh"), "-zsh");
        assert_eq!(login_name("fish"), "-fish");
    }
}

use crate::host::{self, Failure, Origin, clean};
use serde::de::DeserializeOwned;
use std::{
    path::PathBuf,
    process::{Child, Command},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
use zellij_launchpad_core::commands::Command as ToolCommand;

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
    /// Starts the tool in `path` on this terminal. A missing executable or
    /// directory fails here, before anything is committed.
    pub fn spawn(&self, path: &str, tool: &ToolCommand) -> Result<Child, Failure> {
        // `default_shell` from Launchpad's config arrives as Shell's executable.
        let (program, arguments) = match &tool.executable {
            Some(executable) => (executable.clone(), &tool.arguments[..]),
            None => (
                fallback_shell(std::env::var("SHELL").ok(), cfg!(windows), on_path),
                &[][..],
            ),
        };
        let mut command = Command::new(&program);
        command.args(arguments);
        #[cfg(windows)]
        windows::ignore_console_interrupts();
        let fail = |e| Failure::Rejected(format!("Cannot start {program} in {path}: {e}"));
        // herdr falls back to the pane process's own cwd (Windows) or the
        // foreground group leader's (Unix) when nothing reports one, and that
        // is often Launchpad. Move there too, so splits and git detection
        // follow the tool.
        let previous = std::env::current_dir().ok();
        std::env::set_current_dir(path).map_err(fail)?;
        command
            .current_dir(path)
            .env("PWD", path)
            .spawn()
            .inspect_err(|_| {
                if let Some(previous) = &previous {
                    let _ = std::env::set_current_dir(previous);
                }
            })
            .map_err(fail)
    }
    /// Waits for the tool, passing on termination signals Launchpad receives,
    /// then closes this pane as Zellij's close-on-exit would.
    pub fn finish(&self, mut child: Child, forward: &AtomicUsize) -> Result<(), String> {
        let waited = loop {
            match child.try_wait() {
                Ok(Some(_)) => break Ok(()),
                Ok(None) => (),
                Err(e) => break Err(format!("Wait for tool: {e}")),
            }
            #[cfg(unix)]
            if let signal @ 1.. = forward.swap(0, Ordering::Relaxed) {
                // SAFETY: plain syscall; the child is unreaped, so its PID is
                // still its own.
                unsafe { libc::kill(child.id() as libc::pid_t, signal as libc::c_int) };
            }
            #[cfg(not(unix))]
            let _ = forward.load(Ordering::Relaxed);
            thread::sleep(Duration::from_millis(50));
        };
        let closed = self
            .pane()
            .and_then(|pane| self.call::<serde_json::Value>(&["pane", "close", &pane.pane_id]))
            .map(|_| ())
            .map_err(|e| format!("Close pane: {e}"));
        waited.and(closed)
    }
}

/// Shell's program when no `default_shell` is configured: `$SHELL`, else
/// PowerShell 7, Windows PowerShell or bash, then `/bin/sh`, by what is present.
pub fn fallback_shell(
    env_shell: Option<String>,
    windows: bool,
    present: impl Fn(&str) -> bool,
) -> String {
    if let Some(shell) = env_shell
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
    /// tool keeps the default behaviour.
    pub fn ignore_console_interrupts() {
        unsafe extern "system" fn handler(event: u32) -> BOOL {
            (event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT).into()
        }
        // SAFETY: registers a static function that touches no state.
        unsafe { SetConsoleCtrlHandler(Some(handler), 1) };
    }
}

#[cfg(test)]
mod tests {
    use super::fallback_shell;
    #[test]
    fn shell_prefers_shell_env_then_what_is_installed() {
        let all = |_: &str| true;
        let none = |_: &str| false;
        assert_eq!(
            fallback_shell(Some("/bin/zsh".into()), false, none),
            "/bin/zsh"
        );
        assert_eq!(fallback_shell(Some(" ".into()), false, all), "bash");
        assert_eq!(fallback_shell(None, false, none), "/bin/sh");
        assert_eq!(fallback_shell(None, true, all), "pwsh.exe");
        assert_eq!(fallback_shell(None, true, none), "powershell.exe");
        assert_eq!(
            fallback_shell(Some("C:/Git/bin/bash.exe".into()), true, all),
            "C:/Git/bin/bash.exe"
        );
    }
}

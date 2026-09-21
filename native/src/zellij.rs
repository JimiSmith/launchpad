use serde::Deserialize;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use zellij_launchpad_core::commands::Command as ToolCommand;

#[derive(Debug)]
pub enum Failure {
    Rejected(String),
    Unknown(String),
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(s) | Self::Unknown(s) => f.write_str(s),
        }
    }
}
#[derive(Debug, Deserialize)]
pub struct Pane {
    pub id: u32,
    pub is_plugin: bool,
    pub tab_id: u32,
    pub tab_name: String,
}
pub struct Zellij {
    pub executable: PathBuf,
    pub session: String,
    pub pane_id: u32,
    pub timeout: Duration,
}
impl Zellij {
    pub fn discover() -> Result<Self, String> {
        let session = std::env::var("ZELLIJ_SESSION_NAME")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or("Launchpad requires an existing Zellij session. Run it in a Zellij pane.")?;
        let pane_id = std::env::var("ZELLIJ_PANE_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(
                "ZELLIJ_PANE_ID is missing or invalid; run Launchpad in a Zellij terminal pane.",
            )?;
        let host = Self {
            executable: "zellij".into(),
            session,
            pane_id,
            timeout: Duration::from_secs(5),
        };
        let version = host
            .call(&["--version".into()])
            .map_err(|e| e.to_string())?;
        if !supported_version(&version) {
            return Err(format!(
                "Zellij 0.45.0 or newer is required (found {})",
                version.trim()
            ));
        }
        host.origin().map_err(|e| e.to_string())?;
        Ok(host)
    }
    fn call(&self, args: &[String]) -> Result<String, Failure> {
        self.call_until(args, Instant::now() + self.timeout)
    }
    fn call_until(&self, args: &[String], deadline: Instant) -> Result<String, Failure> {
        let mut child = Command::new(&self.executable)
            .args(["--session", &self.session])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Failure::Rejected(format!("Cannot start Zellij CLI: {e}")))?;
        // Drain both pipes concurrently to avoid deadlocking on CLI diagnostics.
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (output_tx, output_rx) = mpsc::sync_channel(2);
        let error_tx = output_tx.clone();
        thread::spawn(move || {
            let _ = output_tx.send((false, read_output(stdout)));
        });
        thread::spawn(move || {
            let _ = error_tx.send((true, read_output(stderr)));
        });
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                outcome => {
                    let _ = child.kill();
                    let _ = child.wait();
                    // A CLI descendant may still hold a pipe open. Never wait
                    // for its reader thread after the command deadline.
                    return Err(Failure::Unknown(format!(
                        "Launch outcome unknown. Quit/reopen; do not automatically retry. Zellij response: {outcome:?}"
                    )));
                }
            }
        };
        let mut stdout = String::new();
        let mut stderr = String::new();
        for _ in 0..2 {
            let (is_error, output) = output_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| Failure::Unknown("Launch outcome unknown: Zellij output did not complete. Quit/reopen; do not automatically retry.".into()))?;
            if is_error {
                stderr = output;
            } else {
                stdout = output;
            }
        }
        if !status.success() {
            // Replacing an interactive shell's pane can hang up its foreground
            // process group, including this CLI child, after Zellij accepts the
            // action. A signal is not a rejection and must not undo history.
            if status.code().is_none() {
                return Err(Failure::Unknown(format!(
                    "Launch outcome unknown: Zellij CLI interrupted ({status}). Quit/reopen; do not automatically retry."
                )));
            }
            return Err(Failure::Rejected(format!(
                "Zellij rejected the action ({status}): {}",
                clean(&stderr)
            )));
        }
        Ok(stdout)
    }
    pub fn origin(&self) -> Result<Pane, Failure> {
        let deadline = Instant::now() + self.timeout;
        let args = ["action".into(), "list-panes".into(), "--json".into()];
        let output = loop {
            let output = self.call_until(&args, deadline)?;
            if !output.trim().is_empty() || Instant::now() >= deadline {
                break output;
            }
            // Zellij can return an empty response while its session starts.
            // Retry only this read, sharing the original command deadline.
            thread::sleep(
                Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
            );
            if Instant::now() >= deadline {
                break output;
            }
        };
        let panes: Vec<Pane> = serde_json::from_str(&output)
            .map_err(|e| Failure::Unknown(format!("Invalid Zellij pane response: {e}")))?;
        panes
            .into_iter()
            .find(|p| !p.is_plugin && p.id == self.pane_id)
            .ok_or_else(|| {
                Failure::Unknown(
                    "Originating pane unavailable; no launch. Reopen Launchpad.".into(),
                )
            })
    }
    pub fn rename(&self, tab_id: u32, name: &str) -> Result<(), Failure> {
        self.call(&[
            "action".into(),
            "rename-tab".into(),
            "--tab-id".into(),
            tab_id.to_string(),
            "--".into(),
            name.into(),
        ])?;
        let pane = self.origin()?;
        if pane.tab_id != tab_id || pane.tab_name != name {
            return Err(Failure::Rejected(
                "Originating tab changed; no launch. Retry.".into(),
            ));
        }
        Ok(())
    }
    pub fn restore_name(&self, original: &Pane, attempted: &str) -> Result<(), Failure> {
        let pane = self.origin()?;
        if pane.tab_id == original.tab_id && pane.tab_name == attempted {
            self.rename(original.tab_id, &original.tab_name)?;
        }
        Ok(())
    }
    pub fn launch_args(&self, path: &str, tool: &ToolCommand) -> Vec<String> {
        let mut args: Vec<String> = [
            "action",
            "new-pane",
            "--in-place",
            "--close-replaced-pane",
            "--pane-id",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        args.extend([
            format!("terminal_{}", self.pane_id),
            "--cwd".into(),
            path.into(),
        ]);
        if let Some(executable) = &tool.executable {
            args.extend(["--close-on-exit".into(), "--".into(), executable.clone()]);
            args.extend(tool.arguments.clone());
        }
        args
    }
    pub fn launch_default_shell(&self) -> Result<(), Failure> {
        self.launch(
            ".",
            &zellij_launchpad_core::commands::Commands::default().entries[0],
        )
    }
    pub fn launch(&self, path: &str, tool: &ToolCommand) -> Result<(), Failure> {
        let response = self.call(&self.launch_args(path, tool))?;
        if response
            .trim()
            .strip_prefix("terminal_")
            .and_then(|id| id.parse::<u32>().ok())
            .is_some()
        {
            return Ok(());
        }
        // 0.45.x returns success with no pane ID on a missing executable. Once
        // that synchronous action completes, a surviving origin means rejection.
        self.origin()?;
        Err(Failure::Rejected(
            "Zellij did not create a pane. Check the executable and Zellij's PATH, then retry."
                .into(),
        ))
    }
}
fn read_output(reader: impl Read) -> String {
    let mut bytes = Vec::new();
    // Continue draining after the display bound, without retaining unbounded text.
    let mut reader = reader;
    let mut chunk = [0u8; 4096];
    while let Ok(n) = reader.read(&mut chunk) {
        if n == 0 {
            break;
        }
        let keep = n.min((1024 * 1024usize).saturating_sub(bytes.len()));
        bytes.extend_from_slice(&chunk[..keep]);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(500).collect()
}
pub fn supported_version(text: &str) -> bool {
    let mut numbers = text.trim().strip_prefix("zellij ").unwrap_or("").split('.');
    let major = numbers.next().and_then(|n| n.parse::<u32>().ok());
    let minor = numbers.next().and_then(|n| n.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 0 || minor >= 45)
}
pub fn tab_name(path: &str, label: &str) -> String {
    let basename = zellij_launchpad_core::host_path::basename(path);
    format!("{basename} · {label}")
}

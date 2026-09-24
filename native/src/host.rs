//! The terminal multiplexer Launchpad runs in: Zellij or herdr.
use crate::{herdr::Herdr, zellij::Zellij};
use std::{
    io::Read,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{atomic::AtomicUsize, mpsc},
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
/// The originating pane's tab, in the host's own opaque ID spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub tab_id: String,
    pub tab_name: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Kind {
    Zellij,
    Herdr,
}
impl Kind {
    /// `LAUNCHPAD_HOST`, ignoring case; empty means unset.
    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(value) = std::env::var_os("LAUNCHPAD_HOST") else {
            return Ok(None);
        };
        let text = value.to_string_lossy();
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        <Self as clap::ValueEnum>::from_str(text, true)
            .map(Some)
            .map_err(|_| format!("LAUNCHPAD_HOST must be zellij or herdr (found {text:?})"))
    }
}
pub enum Host {
    Zellij(Zellij),
    Herdr(Herdr),
}
pub enum Launched {
    /// The host replaced Launchpad's pane; this process may not survive.
    Replaced,
    /// Launchpad runs the tool itself and closes its pane once it exits.
    Running(Child),
}
impl Host {
    pub fn discover(kind: Option<Kind>) -> Result<Self, String> {
        let zellij = std::env::var_os("ZELLIJ_SESSION_NAME").is_some_and(|v| !v.is_empty());
        let herdr = std::env::var_os("HERDR_ENV").is_some_and(|v| v == "1");
        let kind = match (kind, zellij, herdr) {
            (Some(kind), _, _) => kind,
            (None, true, false) => Kind::Zellij,
            (None, false, true) => Kind::Herdr,
            // Either may run nested in the other; the environment cannot tell
            // which pane is Launchpad's own.
            (None, true, true) => {
                return Err(
                    "Both Zellij and herdr are present; choose one with --host or LAUNCHPAD_HOST (zellij or herdr)."
                        .into(),
                );
            }
            (None, false, false) => {
                return Err("Launchpad requires an existing Zellij session or herdr pane. Run it in a Zellij or herdr pane.".into());
            }
        };
        Ok(match kind {
            Kind::Zellij => Self::Zellij(Zellij::discover()?),
            Kind::Herdr => Self::Herdr(Herdr::discover()?),
        })
    }
    pub fn kind(&self) -> Kind {
        match self {
            Self::Zellij(_) => Kind::Zellij,
            Self::Herdr(_) => Kind::Herdr,
        }
    }
    pub fn origin(&self) -> Result<Origin, Failure> {
        match self {
            Self::Zellij(host) => host.origin().map(|pane| Origin {
                tab_id: pane.tab_id.to_string(),
                tab_name: pane.tab_name,
            }),
            Self::Herdr(host) => host.origin(),
        }
    }
    /// Renames the tab and confirms the originating pane still sees that name.
    pub fn rename(&self, tab_id: &str, name: &str) -> Result<(), Failure> {
        match self {
            Self::Zellij(host) => host.rename(
                tab_id
                    .parse()
                    .map_err(|_| Failure::Unknown(format!("Invalid Zellij tab ID {tab_id}")))?,
                name,
            ),
            Self::Herdr(host) => host.rename(tab_id, name),
        }
    }
    pub fn restore_name(&self, original: &Origin, attempted: &str) -> Result<(), Failure> {
        let now = self.origin()?;
        if now.tab_id == original.tab_id && now.tab_name == attempted {
            self.rename(&original.tab_id, &original.tab_name)?;
        }
        Ok(())
    }
    /// Launches in the originating pane. A Zellij shell needs the caller's
    /// handoff arguments; herdr runs the default shell directly.
    pub fn launch(&self, path: &str, tool: &ToolCommand) -> Result<Launched, Failure> {
        match self {
            Self::Zellij(host) => host.launch(path, tool).map(|()| Launched::Replaced),
            Self::Herdr(host) => host.launch(path, tool),
        }
    }
    /// Waits for a tool Launchpad runs itself, then closes the pane with it.
    /// `forward` holds a pending signal number to pass on to the tool.
    pub fn finish(&self, mut child: Child, forward: &AtomicUsize) -> Result<(), String> {
        match self {
            Self::Herdr(host) => host.finish(child, forward),
            // Zellij replaces the pane instead; see `launch`.
            Self::Zellij(_) => child
                .wait()
                .map(|_| ())
                .map_err(|e| format!("Wait for tool: {e}")),
        }
    }
}

pub(crate) struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}
/// Runs one host CLI call without stdin, bounded by `deadline`. Only a
/// completed call returns `Ok`; its exit status is for the caller to judge.
pub(crate) fn run(mut command: Command, host: &str, deadline: Instant) -> Result<Output, Failure> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Failure::Rejected(format!("Cannot start {host} CLI: {e}")))?;
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
                    "Launch outcome unknown. Quit/reopen; do not automatically retry. {host} response: {outcome:?}"
                )));
            }
        }
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    for _ in 0..2 {
        let (is_error, output) = output_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| Failure::Unknown(format!("Launch outcome unknown: {host} output did not complete. Quit/reopen; do not automatically retry.")))?;
        if is_error {
            stderr = output;
        } else {
            stdout = output;
        }
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
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
pub(crate) fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(500).collect()
}
pub fn tab_name(path: &str, label: &str) -> String {
    let basename = zellij_launchpad_core::host_path::basename(path);
    format!("{basename} · {label}")
}

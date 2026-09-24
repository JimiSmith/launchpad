# Native CLI launches

Launchpad requires an interactive terminal in a Zellij or herdr pane. `--help`
and `--version` work outside both. The host is `--host`, else `LAUNCHPAD_HOST`
(case-insensitive; empty means unset), else detected: `ZELLIJ_SESSION_NAME`
means Zellij and `HERDR_ENV=1` means herdr. Both at once is an error, because
one multiplexer runs inside the other and the environment cannot tell which pane
is Launchpad's own. The Zellij handoff passes `--host zellij` to its second
instance. herdr is described [below](#herdr).

Under Zellij, Launchpad requires a live session, a valid `ZELLIJ_PANE_ID` and
Zellij >= 0.45.0. Every CLI action explicitly selects `ZELLIJ_SESSION_NAME`.

The adapter reads `zellij action list-panes --json` and selects its own terminal
ID, never a focused pane. Before launching it resolves the current stable tab ID,
renames that tab `basename · label`, and reads the name back. Missing identity or
failed rename leaves the form with an error.

Configured tools use separate process arguments:

```text
zellij --session SESSION action new-pane --in-place --close-replaced-pane \
  --pane-id terminal_ID --cwd DIRECTORY --close-on-exit -- EXECUTABLE ARGS...
```

There is no shell interpolation or executable preflight. Replacement preserves
Zellij's tiled/floating placement. Exit 0 and nonzero both close the command pane.
Other panes remain untouched. Closing the final pane can end the session.

## Default shell handoff

With `default_shell` set, Shell is an ordinary configured command and none of
this applies. Otherwise:

Zellij 0.45.x discards `--cwd` when `new-pane` has no command, and rejects
`--close-on-exit` without a command. Launchpad therefore replaces itself with a
short-lived instance of **the same native binary**, passing the chosen cwd as an
explicit command launch. This instance asks Zellij to replace its pane with the
configured default shell. Zellij inherits the helper's real cwd, even when
Launchpad originally ran as a child of an interactive shell. No separate helper
executable, shell script, or `$SHELL` substitution is installed.

The private handoff carries the config path, history attempt ID and tab rename
context. A confirmed default-shell rejection recovers into a dashboard in the
selected directory, rolls back that attempt, and conditionally restores the tab
name. It never retries automatically. An unknown outcome displays an error and
allows quitting, but disables further launches.

## Failure and terminal cleanup

The terminal leaves raw mode, disables mouse/paste capture, and restores its
normal screen **before** replacement; successful replacement can kill Launchpad
before its CLI child returns. Ordinary quit, errors, panic and handled termination
signals also restore terminal state.

Zellij 0.45.x can report a missing executable as exit 0 with no created pane ID.
The adapter checks that the original pane survived the completed action before
reporting rejection and restoring the form. A CLI terminated by a signal is also
an unknown outcome: replacing a pane can hang up the interactive shell's process
group after the launch has succeeded. It must not trigger a history rollback.
Timeouts and malformed/unavailable pane responses are unknown outcomes, never
automatic retries. History is written before replacement because the original
process may not survive a success reply.

Known rejection removes only the attempt's history ID. A previous tab name is
restored only when readback still matches this attempt's rename. There is no
atomic compare-and-swap rename or filesystem validation/spawn guarantee.

## Verification

`tools/verify_native.py` runs real PTYs with isolated HOME, PATH, XDG directories
and session sockets. Its fixture tools record literal argv/cwd and accept only
inert exit instructions. The CLI-only `--probe` verifies the upstream behavior
independently of the native UI. Evidence is kept in `target/native-*`.
The suite also launches from an interactive Bash shell and checks that Shell and
configured-command replacements retain history in tiled and floating panes.

## herdr

The herdr adapter requires `HERDR_ENV=1` and `HERDR_PANE_ID`, including when
`--host herdr` is given. It calls the `herdr` CLI named by `HERDR_BIN_PATH`,
falling back to `herdr` on PATH. On Unix each call runs in its own process
group, so hanging up this pane cannot kill a call before herdr replies. It
resolves its own pane with `herdr pane current --current`, which reads the
inherited `HERDR_PANE_ID`, never the focused pane. It reads the tab label with
`herdr tab get`, renames it with `herdr tab rename TAB LABEL` and reads it back.
herdr keeps a `--` separator as part of the label, so none is passed; hyphenated
labels are accepted as-is.

herdr has no in-place pane replacement, and its panes always start a shell.
On Unix, when `herdr pane process-info` names Launchpad's own PID as the pane's
`shell_pid` (Launchpad is herdr's default shell, or was `exec`ed), Launchpad
execs the tool instead, with the chosen cwd and literal argument array: the
tool keeps the pane's PID, so herdr tracks its cwd as for its own shells and
closes the pane when it exits. No directory report, Launchpad cwd change or pane
close is needed. The search worker's thread ends with the exec; its index cache
is written to a temporary file and renamed, so at worst a stray `.tmp` remains.
An exec failure returns, and is handled as the spawn failure below. Any doubt
about the pane's process, including a failed CLI call, means the path below.

Otherwise, and always on Windows, Launchpad restores the terminal, spawns the
tool itself with the chosen cwd and literal argument array, stops its search
worker, and waits. Launchpad first changes its own cwd to the chosen directory,
restoring it if the spawn fails, and after a successful spawn writes a cwd
report to its terminal: OSC 7 `file:///` with a percent-encoded path on Unix
(herdr ignores other hosts) and OSC 9;9 with the bare path on Windows. herdr
keeps one reported cwd per pane, replaced by each newer report, and prefers it
over process cwds for session state, custom commands and the workspace root; the
invoking shell may have reported its own directory before starting Launchpad.
Without a report, herdr reads the foreground group leader's cwd on Unix (also
first for new panes) and the pane process's on Windows, and either can be
Launchpad. On Unix, Launchpad blocks in `waitid(WNOWAIT)` while a signal thread
forwards SIGTERM and SIGHUP sent to Launchpad to the tool; the exit is reaped
only after that thread stops, so no signal can reach a reused PID. Some macOS
versions were reported to return from `waitid` for a stopped child (Go issue
#19314), so only an exit, kill or core dump `si_code` ends the wait, with a 100
ms pause between checks otherwise. Launchpad resets SIGCHLD to its default at
startup, since an inherited ignored SIGCHLD would let the kernel reap the tool
itself. If the thread cannot start, Launchpad still waits, without forwarding.
SIGINT and SIGQUIT only set Launchpad's stop flag, since the terminal delivers
them to the tool itself. On Windows, Launchpad installs a console handler that
ignores Ctrl+C and Ctrl+Break, and removes it if the spawn fails; handlers are
not inherited, so the tool keeps the default. When the tool exits, with any
status, Launchpad runs `herdr pane close` on its own pane.

Shell spawns directly, with no default-shell handoff: `default_shell` from
Launchpad's config if set, else `$SHELL` unless its file name is Launchpad's own
(herdr's login-shell mode sets `$SHELL` to herdr's default shell), else the
first present of `pwsh.exe` then `powershell.exe` on Windows, or `bash` then
`/bin/sh` elsewhere. Presence means a file of that name in a PATH directory.
Launchpad does not read herdr's configuration.

A stop signal (SIGTERM, SIGHUP, SIGINT, SIGQUIT) that arrives after a tool is
chosen but before it starts, for example during the tab rename, cancels the
launch: the tab name and history attempt are rolled back and Launchpad quits,
so the signal is not passed on to a tool that has just started.

A spawn failure, such as a missing executable or directory, is a
known rejection: the form returns, the tab name is restored and the history
attempt is removed. Once the tool has started there is nothing to roll back.

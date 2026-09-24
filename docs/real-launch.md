# Native CLI launches

Launchpad requires an interactive terminal in a Zellij or herdr pane.
`--help` and `--version` work outside both. The host is `--host`, else
`LAUNCHPAD_HOST`, else detected: `ZELLIJ_SESSION_NAME` means Zellij and
`HERDR_ENV=1` means herdr. Both at once is an error, because one multiplexer runs
inside the other and the environment cannot tell which pane is Launchpad's own.
The Zellij handoff passes `--host zellij` to its second instance. herdr is
described [below](#herdr).

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

herdr has no in-place pane replacement, and its panes always start a shell. So
Launchpad restores the terminal, spawns the tool itself with the chosen cwd and
literal argument array, stops its search worker, and waits. Launchpad first
changes its own cwd to the chosen directory, restoring it if the spawn fails:
without a reported cwd (OSC 7, 9;9 or 1337), herdr reads the foreground group
leader's cwd on Unix and the pane process's on Windows, and either can be
Launchpad. SIGTERM and SIGHUP
sent to Launchpad are forwarded to the tool. On Windows, Launchpad installs a
console handler that ignores Ctrl+C and Ctrl+Break; handlers are not inherited,
so the tool keeps the default. When the tool exits, with any status, Launchpad
runs `herdr pane close` on its own pane.

Shell spawns directly, with no default-shell handoff: `default_shell` from
Launchpad's config if set, else `$SHELL`, else the first present of `pwsh.exe`
then `powershell.exe` on Windows, or `bash` then `/bin/sh` elsewhere. Presence
means a file of that name in a PATH directory. Launchpad does not read herdr's
configuration.

A spawn failure, such as a missing executable or directory, is a
known rejection: the form returns, the tab name is restored and the history
attempt is removed. Once the tool has started there is nothing to roll back.

# Native CLI launches

Launchpad requires a live Zellij session, a valid `ZELLIJ_PANE_ID`, an interactive
terminal, and Zellij >= 0.45.0. `--help` and `--version` work outside Zellij.
Every CLI action explicitly selects `ZELLIJ_SESSION_NAME`.

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

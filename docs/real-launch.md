# Real pane launches (Zellij 0.45.1)

The normal plugin replaces its **own originating pane**, not the last-focused
pane. Agents use `close_replaced_pane = true`; Shell uses
`close_plugin_after_replace = true`: the dashboard is removed, not suspended
behind the terminal. Agent panes close on both zero and nonzero exit and never
bring the dashboard back. Closing the last pane ends the session as usual.
Tiled/floating geometry and other panes are left to Zellij's in-place operation.

| Choice | Host operation | Arguments |
|---|---|---|
| Shell | `open_terminal_in_place_of_plugin(cwd, true)` | Zellij's configured default shell |
| Configured ID | `run_action(Action::NewInPlacePane { … }, context)` | configured executable + parsed literal argv |

Configured commands use `RunCommandAction { command, args, cwd: Some(validated_path),
hold_on_close: false, hold_on_start: false, … }`. The action explicitly targets
`PaneId::Plugin(get_plugin_ids().plugin_id)`, with `close_replaced_pane: true`.
There is no `which`, PATH scan, executable probe, authentication check, shell
interpolation, or command injection via terminal input. Shell is the sole
unconfigured choice; other choices come only from [plugin configuration](configured-commands.md).
A missing command is passed to
Zellij without prevalidation. With close-on-exit, 0.45.1 logs the spawn failure
instead of replacing the dashboard with a held error pane. The original form
shows a generic rejection; fix PATH/install the command and explicitly resubmit.
The plugin does not claim the command successfully executed merely because
Zellij accepted the replacement.

## Invoking cwd and tab naming

Production captures `get_plugin_ids().initial_cwd` before HOME remount and stores
it in instance-local `/data/original-cwd`. HOME bootstrap reload and F5 retain it;
Shell starts selected, with no highlighted suggestion. One Enter validates and
replaces the dashboard even while HOME is still indexing. Longer-than-100-scalar
cwd identities are never truncated. The `simulate_launch` default is unchanged.

Only the exact original cwd (including its HOME-short label) can bypass ordinary
HOME validation. On submission the main instance remounts to that path, waits for
`HostFolderChanged`, and opens `/host` as a directory. The worker remains mounted
to HOME and never scans this exception. Hidden/ignored, outside-HOME and symlink
invoking cwd work; an unavailable/deleted cwd stays visible without fallback.
F5 cancels the validation result but retains the outstanding host remount until
its acknowledgement is consumed. Host remounts have no request IDs, so new
validation waits for that acknowledgement; a stale success cannot launch an old
target or validate the new form. A late invoking-cwd failure is not a HOME
bootstrap failure. Resubmitting a still-deleted cwd gives a recoverable directory
error; restoring it and explicitly resubmitting can launch normally.
This is not general outside-root or relative-to-cwd navigation. The original
host path is passed literally to the shell/command, without a helper or wrapper.
Host path serialization is lossy in 0.45.1: U+FFFD invoking names are conservatively
rejected (even legitimate ones), as are invalid UTF-8, controls and overlong paths.

Before replacement the tab name is `basename · configured label`, for example
`notes · Shell` or `my-app · Claude in Worktree`. Root uses `/`; HOME uses its
basename. History replay resolves the current label. Naming uses the stable tab
ID, never its mutable position or an unrelated focused tab. A synchronous
`get_focused_pane_info` tuple is accepted only when its pane ID equals our plugin;
otherwise one coherent session snapshot maps the plugin ID through pane position
to stable tab ID. A missing mapping fails visibly instead of guessing. Session
snapshots can lag new background panes or pane moves; no atomic pane-move/naming
transaction is claimed.

Use the direct `rename_tab_with_id` command, then synchronous `get_tab_info`
readback before spawn. The two requests use the same host screen queue. **Do not
use `run_action(Action::RenameTabById)`**: SDK 0.45.1 calls it CLI-only and panics
on serialization, despite exposing the enum variant. Positional `rename_tab` is
also unsuitable when tabs reorder.

Known spawn rejection reads the current name and restores the saved previous
name only when it still equals this attempt's write, then reads back the result.
A newer name already visible at that check is preserved. Host rename has **no
compare-and-swap**: a manual rename arriving between the check and restore can
still race; an atomic rollback guarantee needs a host API change. Successful
replacement destroys Launchpad, so subsequent process failure cannot undo naming.
There is likewise no atomic filesystem validation/spawn guarantee.

Live cwd/name evidence and TDD failures are summarized in
`target/cwd-tab-naming/REPORT.md`; the reset/remount follow-up and rebuilt artifact
hash are in `target/cwd-tab-naming/reset-fix/REPORT.md`.

## Permissions and failure

The startup request bundles the existing `ReadSessionEnvironmentVariables`,
`FullHdAccess`, and `ChangeApplicationState` permissions with
`OpenTerminalsOrPlugins` (Shell), `RunActionsAsUser` (agents), and
`ReadApplicationState` (own-pane/tab mapping and name readback). RunActionsAsUser is
shown as **Execute actions as the user**, is broader than `RunCommands`, and is
required by `run_action` even for a command-launch action. Denial retains the
dashboard with a visible error and starts nothing. The bundle must be granted
before HOME initialization or launching. Cached permissions are managed by Zellij.
Full disk access is broader than the application's HOME-only traversal policy.

Shell's synchronous host API `None` response keeps the dashboard and displays a
rejection. Agents' `run_action` returns `()`, not acceptance or a pane ID. The
plugin subscribes to `ActionComplete` and correlates its context with the one
pending request. A matching completion without an affected pane unlocks the
original form and displays a rejection; a new explicit submit is required.
An affected pane ID is not proof of process success or its exit status. Successful
replacement normally destroys the plugin before any callback can be consumed.
Unrelated or stale completions cannot unlock a pending request.

Submitted requests are one-shot, lock further form input, and never enter the
simulated terminal screen. A validated attempt is persisted before replacement;
known rejection removes only its exact record, preserving concurrent updates. No timeout auto-retry is
used: an unacknowledged action might still execute. If the host never completes
an action, close/reopen the plugin rather than assuming it failed. The startup
permission grant gates all submissions; permission denial at the host command
boundary is logged by Zellij and does not itself deliver `ActionComplete`.
Directory failures retain the existing form and never reach the launch API.
Shared history and its failure/consistency contract are described in [history.md](history.md).

The ordinary HOME directory/symlink policy, ignore rules and insertion limits
remain, with only the exact invoking-cwd exception described above.
Filesystem validation and OS spawn are separate operations: this does not promise
an atomic security boundary against concurrent directory/symlink replacement.

A narrow real-mode guard ignores Enter/the launch button while completion or
launch validation is outstanding. Wait for completion, then submit again. This
prevents rapid Tab→Enter from launching the old editor path; it does not otherwise
redesign asynchronous completion.

## Pinned source findings

Verified against `zellij-tile` and `zellij-server` 0.45.1, not only website docs:

- SDK `src/shim.rs::run_action` returns `()`; the older
  `open_command_pane_in_place_of_plugin` forces held command panes and is no
  longer used for agents.
- `zellij-utils/src/plugin_api/action.rs` round-trips the explicit replacement
  pane ID, close-replaced flag, command cwd/argv, and both hold flags. The action
  specifies `near_current_pane: false`, `no_focus: false`, `pane_name: None`,
  `tab_id: None`; 0.45.1's protobuf does not carry the latter focus/tab fields,
  but decodes them to these same defaults.
- Server `src/route.rs`'s `NewInPlacePane` preserves the supplied command and
  prioritizes the explicit pane ID, then sends `SpawnInPlaceTerminal`.
- `src/plugins/zellij_exports.rs::run_action` routes on a separate thread and
  emits `ActionComplete(action, affected_pane_id, context)`. The permission
  mapping requires `RunActionsAsUser`, not `RunCommands`.
- `src/pty.rs`'s `SpawnInPlaceTerminal` missing-command branch with
  `hold_on_close: false` logs the error without replacing the plugin. The dropped
  completion guard results in a completion with no affected pane.
- The unchanged `open_terminal_in_place_of_plugin` uses the configured
  `default_shell` / `path_to_default_shell` with the selected cwd, targets the
  originating plugin ID, and requires `OpenTerminalsOrPlugins`.

The inspected server source is retained locally at
`target/host-scan-probe/source/zellij-server-0.45.1/`.

## Safe verification

`bash tools/verify_plugin.sh` runs serial builds (`CARGO_BUILD_JOBS=1`), Rust tests,
Clippy, explicit simulated search/worker/capacity tests, live-host PTYs,
and actual real-host launches with `tools/verify_launch.py`.

The real launch harness uses owned `target/real-launch/live-*` HOME/config/cache/
data/socket directories, an isolated PATH containing only harmless fixture
executables, an absolute Python interpreter in their shebangs, and a controlled
Zellij default shell (with `SHELL=/bin/false` to distinguish it). The four agent
names are real executed fixtures logging argv/cwd/PID and accepting inert input;
no installed coding agent is reachable. Missing-command cases omit the fixture.

Each run retains `report.json`, `executions.jsonl`, `exits.jsonl`, raw PTY bytes, xterm.js replay
events, and exact host pane lists before/after/exit. Neighbor-focus cases submit
to the originating plugin while a different pane is focused, so asynchronous
validation cannot accidentally replace the neighbor. Assertions compare pane
identity, command, cwd, tab, geometry, floating status, and removal rather than
just relying on a rendered success label. Exits 0 and 17 are checked in tiled and
floating layouts, including neighbor-focus races. The floating focus test also
sends inert keyboard input to the neighbor: 0.45.1's pane list can mark both
layers' selected panes focused even when floating panes are hidden. Post-exit
readback must contain exactly the original other panes (including Zellij's own
suppressed plugins), never the launched terminal or dashboard. Single-pane runs
verify clean client exit and the CLI's `There is no active session!` response.
Missing-command tests assert failure on the original form, no automatic retry,
then install the harmless fixture and verify explicit retry through normal exit.

`simulate_launch "true"` keeps real HOME search and validation but never spawns
a process: the dashboard reports `Launch suppressed (simulate_launch)` and keeps
the attempt in memory only. Search harnesses choose it so their existing
validation/history assertions remain meaningful and safe.

TDD evidence is under `target/real-launch/`: `red-shell.log` shows the original
WASM displaying its placeholder instead of executing the default-shell fixture;
`green-shell.log` records the first actual replacement. `red-guard.log` captures
the old-editor submission race, and `red-ui.log` the misleading real-mode labels.
Final full-run results and artifact hashes are recorded there separately.

Close-on-exit TDD and full-run evidence is under `target/close-on-exit/`, with a
before snapshot preserving the previous artifact and uncommitted changes.
`red-lifecycle.log` reproduces a held pane after exit on the old artifact;
`green-lifecycle.log` verifies removal. `red-async-failure.log` reproduces a stuck
pending form without the completion handler; `green-async-failure.log` verifies
rejection and explicit retry. `full-verification.log` records the complete suite.

## Reload after rebuilding

Existing panes do not hot-reload. Close the old Launchpad pane, then run from a
shell inside Zellij at the repository root (this direct launch is **Shell only**;
use the [configured layout](../examples/configured.kdl) for additional commands):

```sh
zellij action launch-plugin --skip-plugin-cache -- \
  "file:$(pwd)/target/wasm32-wasip1/release/zellij-launchpad.wasm"
```

Allow the new **Execute actions as the user** permission when prompted. Existing
already-launched/held terminals retain their old lifecycle; this affects only
new launches from the rebuilt plugin.

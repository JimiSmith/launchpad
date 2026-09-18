# Home directory search

## Current scope

Both adapters use the same HOME-only directory index and embedded Frizbee matcher.
The native process reads its `HOME`; Zellij reads the session-creation environment
through `get_session_environment_variables()` and retains only `HOME`. Changing
HOME in a later shell does not change the session's HOME. Missing/invalid HOME is
an error, never a fallback to `/host`, process CWD, `/`, or fixture directories.

Initial input is `~`, history is empty, and all launches/availability remain
simulated. Only completed simulation actions add in-memory history. F5 clears
that history/form and rebuilds the index. There is no persistent history, PATH
probe, subprocess, external search helper, file-content read, or network request.

## Host permissions and mapping

Pinned SDK/verified host: Zellij 0.45.1. Request:

- `ReadSessionEnvironmentVariables`: obtain the host session's HOME.
- `FullHdAccess`: required by this host for `ChangeHostFolder`.
- `ChangeApplicationState`: reload once during boot so workers inherit HOME.

The current website describes `ChangeApplicationState` for `change_host_folder`,
but the pinned source (`zellij-server/src/plugins/zellij_exports.rs`, permission
match) and live host reject that combination with `FullHdAccess` denied. The
filesystem permission is broad; this plugin's implementation only reads HOME.
It does not ask for command execution or terminal-opening permissions.

After permission grant, remount `/host` to HOME using `change_host_folder`, then
wait for the matching `HostFolderChanged` acknowledgement. If the initial mount
was not HOME, reload once before accepting input, then require the worker's
`initial_cwd` handshake to equal HOME **before** indexing. See [worker details](async-workers.md).
The default `/host` is an invoking-terminal CWD, not necessarily HOME. Host paths
are kept separate from WASI paths; `/host` is never presented as a user path.
Denial, missing HOME, remount failure, and root read failure stay visible without
retry loops. Reopen after fixing the environment/permissions.

## Policy and bounds

- Breadth-first directory traversal, no exclusion list, including hidden subtrees.
- One tick consumes at most 128 open/read-entry units, with a cooperative 5 ms
  budget. Native polling is cooperative; the plugin keeps traversal, Frizbee and
  validation in a persistent WASM worker. Only capped results reach its UI;
  epochs, query generations and revisions reject stale responses. No filesystem
  IO or matching runs on the plugin's input/render path.
- Stop at 20,000 directory candidates or 200,000 entries, with maximum depth 64.
  Also stop at a conservative 6 MiB retained-path/queue budget or a path over
  4096 bytes. Edited input is capped at 4096 UTF-8 bytes.
  Limits and skipped-entry counts are visible. This is a partial index when capped;
  an explicit valid path can still be entered without being indexed.
- Keep at most 100 ranked suggestions, ordered by Frizbee score and path tie-break.
  Dot-prefixed query components reveal hidden results; HOME's own spelling does
  not classify all descendants as hidden.
- Symlinks are neither candidates nor traversed, even when pointing inside HOME.
  This deliberately narrower policy avoids cycles/escape; internal symlink support
  in the draft spec is deferred. Ancestors are checked again before descent and
  validation. Concurrent hostile filesystem replacement is not an atomic-security
  guarantee; there is no real process launch at this stage.
- Skip invalid UTF-8 and control-character directory names rather than display
  ambiguous/unsafe names. Spaces, Unicode, quotes and shell-looking text are literal.
- `~`, `~/…`, absolute paths under HOME and HOME-relative paths work. `.`/`..`
  are accepted only within HOME; traversing a symlink or escaping HOME is rejected.
- Completion acceptance and submission revalidate actual directories, reject files,
  deleted paths, symlinks and inaccessible paths, and preserve the form on error.
- Results are a snapshot. F5 discovers newly created directories and removes stale
  cached entries. Stale candidates also fail validation at acceptance/submission.

The cooperative time limit cannot interrupt one slow filesystem syscall (for
example an unavailable network mount). No hard input-latency SLA is claimed.
Hidden caches and large build trees count toward the same limits as other folders.

## Development verification

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p launchpad-plugin --target wasm32-wasip1 --locked -- -D warnings
cargo build --locked --release
cargo build --locked --release -p launchpad-plugin --target wasm32-wasip1
# pyte is a development-only dependency in target/verification-venv
PY=target/verification-venv/bin/python
$PY tools/verify_search.py
$PY tools/verify_search.py --deny
$PY tools/verify_search.py --native

$PY tools/verify_zellij.py
$PY tools/verify_pty.py
$PY tools/verify_cleanup.py
```

`verify_search.py` uses private host state and controlled directories, a CWD outside
the test HOME, real permission grant/deny, and readback of the closed plugin pane.
All automated search checks use disposable HOME fixtures beneath `target/`;
there is no mode that uses the invoking user’s HOME. Unsupported flags are rejected
before setup. Evidence is ignored under `target/zj-*`.
The older UI/mouse regression harnesses explicitly select demo mode (`--demo` for
native, plugin configuration `demo=true`); production startup never uses it.

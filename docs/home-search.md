# Home directory search

## Current scope

Both adapters use the same HOME-only directory index and embedded Frizbee matcher.
The native process reads its `HOME`; Zellij reads the session-creation environment
through `get_session_environment_variables()` and retains only `HOME`. Changing
HOME in a later shell does not change the session's HOME. Missing/invalid HOME is
an error, never a fallback to `/host`, process CWD, `/`, or fixture directories.

Initial input is `~` and history is empty. Normal plugin launches now
[replace the originating pane](real-launch.md); native/demo launches remain
simulated. Only completed simulation actions add in-memory history. F5 clears
that history/form and rebuilds the index. Search uses no persistent history, PATH
probe, subprocess, external search helper, or network request. Indexing reads
HOME-local `.gitignore` and `.ignore` contents, but not ordinary project files.

## Host permissions and mapping

Pinned SDK/verified host: Zellij 0.45.1. Request:

- `ReadSessionEnvironmentVariables`: obtain the host session's HOME.
- `FullHdAccess`: required by this host for `ChangeHostFolder`.
- `ChangeApplicationState`: reload once during boot so workers inherit HOME.

The current website describes `ChangeApplicationState` for `change_host_folder`,
but the pinned source (`zellij-server/src/plugins/zellij_exports.rs`, permission
match) and live host reject that combination with `FullHdAccess` denied. The
filesystem permission is broad; this plugin's implementation only reads HOME.
Normal launches additionally request `RunActionsAsUser` and `OpenTerminalsOrPlugins`;
the explicit `simulate_launch "true"` search harness needs only the three above.

After permission grant, remount `/host` to HOME using `change_host_folder`, then
wait for the matching `HostFolderChanged` acknowledgement. If the initial mount
was not HOME, reload once before accepting input, then require the worker's
`initial_cwd` handshake to equal HOME **before** indexing. See [worker details](async-workers.md).
The default `/host` is an invoking-terminal CWD, not necessarily HOME. Host paths
are kept separate from WASI paths; `/host` is never presented as a user path.
Denial, missing HOME, remount failure, and root read failure stay visible without
retry loops. Reopen after fixing the environment/permissions.

## Policy and bounds

- The pinned `ignore = 0.4.33` **serial** depth-first walker replaces the custom
  breadth-first queue. No thread/parallel walker is used. Hidden directories and
  exact `node_modules` names (including `.git` through the hidden rule) are pruned
  at every depth, regardless of ignore-file negations. Similar visible names such
  as `node_modules_backup` remain eligible. Exclusions affect discovery, not
  explicit-path validation; a dot in a query does not reveal pruned candidates.
- HOME-local `.gitignore` and `.ignore` rules support nested patterns and negation,
  including outside Git repositories. `.ignore` outranks `.gitignore`; within a
  rule type the nearest matching file wins. An excluded parent cannot be reopened
  by a descendant rule. No excluded paths or skipped counter are retained.
- One scan slice requests at most 128 iterator results, checking a cooperative 5 ms
  budget between calls. `next()` may internally consume many ignored entries;
  these limits are not exact syscall or wall-clock budgets. Native polling is cooperative; the plugin keeps traversal, Frizbee and
  validation in a persistent WASM worker. Only capped results reach its UI;
  epochs, query generations and revisions reject stale responses. No filesystem
  IO or matching runs on the plugin's input/render path.
  The worker schedules its next slice immediately, keeping at most one queued
  continuation so queries/validation/refresh can interleave. Progress is emitted
  at most every 100 ms, with immediate final status. UI timers only check the
  watchdog; they do not pace indexing. No persistent index cache is used.
- Stop at 20,000 directory candidates or 200,000 entries, with maximum depth 64.
  Also stop at a conservative 6 MiB retained-path/rule budget or a path over
  4096 bytes. Typed/pasted input and fuzzy queries are capped at 100 Unicode
  scalar values; completed paths are preserved without truncation.
  Limits are visible; ignored entries are not counted or retained. This is a partial index when capped;
  an explicit valid path can still be entered without being indexed.
- Keep at most 100 ranked suggestions, ordered by Frizbee score and path tie-break.
  HOME's own spelling, including a hidden physical fixture/mount root, does not
  classify its normal descendants as hidden.
- Symlinks are neither candidates nor traversed, even when pointing inside HOME.
  This deliberately narrower policy avoids cycles/escape; internal symlink support
  in the draft spec is deferred. The walker does not follow directory links;
  acceptance/submission still check every literal path component. The old repeated
  ancestor revalidation on each scan descent is removed. Concurrent hostile
  filesystem replacement is not an atomic-security guarantee; there is no real
  process launch at this stage.
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
Hidden/ignored subtrees do not consume catalogue, entry or retained-path limits.
Visible, non-ignored build trees still do.

### Rule loading and filesystem boundaries

`ignore` 0.4.33's automatic Git discovery opens parent ignore files even when
`parents(false)` disables their *effects*. A native inotify regression proves this
upstream behaviour and the boundary fix. All automatic rule discovery is therefore
disabled. A small DFS-ancestry rule stack uses the released crate's `GitignoreBuilder`
and matcher, loading only `.ignore`/`.gitignore` in accepted HOME directories. It
is not a second directory walker. Parent/global Git configs, `.git/info/exclude`,
Git worktree pointers and repository metadata are not read. Symlinked rule files
(internal or external), FIFOs and devices are not opened; ordinary rule files are
read after a no-follow metadata check. As with path validation, replacement races
between checking and opening are not atomically prevented.

Rules share the existing byte allowance: cumulatively charge 16 times source
bytes plus 2 KiB per source line for parser/matcher overhead. Each rule file is
also bounded to 64 KiB before parsing. Exceeding either allowance stops indexing
with the usual visible limit state, rather than continuing with incomplete rules.
This is conservative accounting, not a guarantee on allocator/regex peak memory.
Malformed patterns and unreadable rule files are ignored like the crate's normal
best-effort loader. Ancestor matchers are released as the serial walker leaves
their subtree; no list of excluded paths is stored.

The serial walker's underlying `walkdir` may open a pruned directory handle before
its filter runs. It then skips the subtree: descendants and their ignore files
are not visited/read, counted or catalogued. Do not interpret pruning as a promise
of zero metadata/open syscalls for the excluded directory itself.

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
$PY tools/verify_workers.py
$PY tools/verify_input_limit.py
$PY tools/verify_input_limit.py --native
$PY tools/verify_index_benchmark.py --wasm target/wasm32-wasip1/release/launchpad-plugin.wasm

$PY tools/verify_zellij.py
$PY tools/verify_pty.py
$PY tools/verify_cleanup.py
```

`verify_search.py` uses private host state and controlled directories, a CWD outside
the test HOME, real permission grant/deny, and readback of the closed plugin pane.
The benchmark accepts an explicit release WASM, constructs the same 18,110-directory
visible tree for each run, verifies the loaded URL/hash and catalogue count, and
reports time from first visible indexing to completion separately from edit and
result latency. These are PTY-observed timings, not cold-disk or pure-walker timings.
All automated search checks use disposable HOME fixtures beneath `target/`;
there is no mode that uses the invoking user’s HOME. Unsupported flags are rejected
before setup. Evidence is ignored under `target/zj-*`.
The older UI/mouse regression harnesses explicitly select demo mode (`--demo` for
native, plugin configuration `demo=true`); production startup never uses it.

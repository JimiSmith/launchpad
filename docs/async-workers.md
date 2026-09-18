# Async HOME indexing and search

The committed synchronous baseline is `3332bfee73f833a88081fc5260b4846c13d7767a`.
Async work was implemented and verified in two stages without committing it.
See [the measured report](../verification/async/REPORT.md).

## Boot: HOME is not the invoking CWD

Zellij 0.45.1 creates worker WASM/WASI instances **before** the main plugin's
`load()`. Changing the main `/host` mapping does not remount workers. Worker
permissions are separate; trying the main instance's remount API from a worker
is not a solution.

The main instance obtains only HOME from the session environment after permission
grant. If its original CWD differs, it remounts HOME, waits for the exact
`HostFolderChanged` acknowledgement, writes a one-attempt marker in private
`/data`, and calls `reload_plugin_with_id` for its own ID. The host reloads from
the main instance's current plugin CWD. The new worker reports its own
`get_plugin_ids().initial_cwd`; traversal starts only if it equals HOME. The
original launch command works from arbitrary CWD. No shell helper or explicit
`--cwd "$HOME"` launch recipe is needed.

Input is gated during this boot sequence, with visible status; Ctrl+Q still
works. The application never reloads after editing starts. The marker prevents
a failed remount/reload from looping. Mapping disagreement, permission denial,
and a 15-second missing-response watchdog have visible failures, not a scan of
the wrong directory. Full disk access is a broad **host permission**, not a
HOME-only OS sandbox. Change application state is an additional permission for
self-reload. Plugin-private `/data` is used only for the reload marker, not history.

Source checked against upstream tag v0.45.1, commit
`efd8fd5a89a20c07a111d248ad7fce53848d2c18`:

- [plugin_loader.rs](https://github.com/zellij-org/zellij/blob/efd8fd5a89a20c07a111d248ad7fce53848d2c18/zellij-server/src/plugins/plugin_loader.rs): workers before load (168–230), independent environment (351–418), **16 MiB per linear memory** limit (479–487).
- [wasm_bridge.rs](https://github.com/zellij-org/zellij/blob/efd8fd5a89a20c07a111d248ad7fce53848d2c18/zellij-server/src/plugins/wasm_bridge.rs): remount main (1008–1106), reload using current CWD (625–651, 1698–1706).
- [plugin_worker.rs](https://github.com/zellij-org/zellij/blob/efd8fd5a89a20c07a111d248ad7fce53848d2c18/zellij-server/src/plugins/plugin_worker.rs): serial unbounded host queues. Application backpressure is necessary.
- [zellij_exports.rs](https://github.com/zellij-org/zellij/blob/efd8fd5a89a20c07a111d248ad7fce53848d2c18/zellij-server/src/plugins/zellij_exports.rs): plugin IDs report the environment CWD (933–945), permission checks and message routing.
- Cached SDK `zellij-tile-0.45.1/src/lib.rs` 197–218: `register_worker!`
  retains a thread-local `RefCell`; it does **not** serialize state per message.
  `#[serde(skip)] Option<HomeIndex>` therefore retains an open `ReadDir`.
  Export `index_worker` is addressed as `index` using `PluginMessage`.

The throwaway `worker-probe` binary and `tools/probe_worker_home.py` verify an
actual directory witness and actual worker CWD with HOME different from CWD,
plus denial and no reload loop. It is a development probe, not a runtime helper.

## Stage 1: indexing worker

The first working stage moved `HomeIndex::step(128)` into the worker. Main received
bounded deltas and still matched/validated synchronously. Its saved binary is
`target/perf-index-worker/launchpad-plugin.wasm`; `source.patch`, `workers.rs`
and `worker-tests.rs` in the same folder preserve this intermediate stage.
It passed the real search/mouse/Unicode/refresh harness before stage 2 began.
The measurements showed virtually no cached-input improvement: matching, not
only traversal, was the remaining main-thread cost. This intermediate design is
not the current UI implementation.

## Stage 2: matching and validation off the UI

`plugin/src/workers.rs::Engine` owns the only populated catalogue and runs
Frizbee and directory revalidation. Indexing, matching and validation share one
persistent worker. This deliberately avoids duplicating the catalogue or sending
large index deltas between sibling instances under the host's memory ceiling.
The native adapter retains its cooperative synchronous backend.

`plugin/src/workers.rs` drives scanning with immediate self-messages, with at
most one continuation queued. Each handler requests at most 128 iterator results
within a cooperative 5 ms budget, then yields to the host queue. The worker does
not wait for a UI timer or progress acknowledgement and does not complete the
entire traversal in one handler. Refresh replaces the index/epoch without adding
a second continuation; an already queued old-epoch step only wakes the current
scan. Queries and validation can interleave between scan slices.

Only periodic progress (at most once per 100 ms) reaches the UI; completion,
including a limit or IO error, is immediate. The main timer is a one-second
watchdog check, not scan pacing. Start and progress reset its existing 15-second
missing-response deadline. The worker asks the host for its CWD only at Start.

`plugin/src/main.rs` permits one outstanding query/validation request.
`src/remote.rs` and `src/app.rs` keep only the latest unsent demand, so edits
replace pending queries instead of filling Zellij's unbounded queue. No persistent
index cache, extra worker, external scanner or artificial scan sleep is used.

- Refresh uses an index epoch; both keyboard and mouse reset start a new epoch.
- Query/action generations reject obsolete results and validation completions.
- Catalogue revisions avoid re-matching unchanged traversal progress and reject
  results older than known progress.
- Dismissal, completion cycling, history copy, reset, help, closed/terminal
  screens and quit prevent late work from reopening or overwriting the form.
- Validation is asynchronous for mouse/keyboard completion, direct submission
  and history replay. Simulation checks fixture availability; real launches do
  not check command availability.
- UI state contains at most 100 returned paths, not the index. Result path bytes
  are capped at 64 KiB; JSON encoding adds framing/escaping overhead.
- Directory count/entry/depth limits remain 20,000 / 200,000 / 64. A conservative
  6 MiB accounting limit includes retained paths and HOME-local ignore rules.
  This is not an exact allocator/RSS meter. Filesystem paths are limited to
  4096 UTF-8 bytes; typed/pasted input and fuzzy queries to 100 Unicode scalars.
  Completed paths remain intact. Limit status is visible.
- Missing worker replies stop further dispatch after 15 seconds. Once initialized,
  the UI stays editable but cannot claim validation success. There is no silent
  synchronous fallback. Reopen to replace a failed worker.

### Limitations

A running Frizbee request is still a whole-catalogue computation. It is not
preemptively cancellable. New edits are coalesced and stale results discarded,
but validation and scanning can wait behind that computation in the shared
worker. A slow filesystem syscall also cannot be interrupted. Moving work off
the UI does not create a hard latency SLA or make final ranked results immediate.
The measured improvement is **key-to-visible-edit latency**, not matcher speed.
The improved observed index duration also reflects less redundant matching of
unchanged catalogue revisions, not a faster disk traversal primitive.

Normal plugin launches now [replace their originating pane](real-launch.md).
The search/worker harnesses explicitly simulate launches. Symlink/TOCTOU restrictions remain as documented
in [HOME search policy](home-search.md).

## Repeatable verification

```sh
bash tools/verify_plugin.sh
PY=target/verification-venv/bin/python
$PY tools/probe_worker_home.py
$PY tools/probe_worker_home.py --deny
$PY tools/verify_workers.py
$PY tools/benchmark_workers.py baseline target/perf-baseline/launchpad-plugin.wasm
$PY tools/benchmark_workers.py index-worker target/perf-index-worker/launchpad-plugin.wasm
$PY tools/benchmark_workers.py async-final target/wasm32-wasip1/release/launchpad-plugin.wasm
```

The PTY tools reuse `verify_search.py`'s isolated setup/emulator prefix, changing
only artifact/layout and controlled test-tree creation. They preserve raw ANSI
and xterm replay records beneath `target/zj-*`; no private HOME listings are
copied into tracked evidence. All three performance runs enumerate 18,110 dirs.

The `worker-faults` feature is development-only and absent from normal builds.
It can deliberately silence replies after the mapping handshake to verify the
real watchdog, editable failed state, launch rejection and pane unload:

```sh
cargo build --locked --release -p launchpad-plugin --bin launchpad-plugin \
  --target wasm32-wasip1 --features worker-faults
$PY tools/verify_workers.py --fault
# Always restore the ordinary artifact afterwards:
cargo build --locked --release -p launchpad-plugin --bin launchpad-plugin \
  --target wasm32-wasip1
```

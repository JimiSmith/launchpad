# Async worker verification — Raspberry Pi / Zellij 0.45.1

## Outcome

Both stages work in a real Zellij PTY. The final plugin performs HOME traversal,
Frizbee matching and completion/submission validation in a persistent worker;
the UI receives capped rows and status only. The native adapter remains
cooperative/synchronous. No real launch, external helper, or non-HOME traversal
was added. Nothing was committed or pushed; baseline HEAD remains
`3332bfee73f833a88081fc5260b4846c13d7767a`.

## Prior performance measured on this host (before UI-state corrections)

Host: Linux aarch64, `6.18.34+rpt-rpi-2712`; Rust/Cargo 1.98.1, Zellij 0.45.1.
Release WASI builds, real 80×24 PTY, isolated Zellij sessions, controlled HOME
outside invocation CWD. Each completed index contained **18,110 directories**
(see the three `*-indexed.txt` snapshots). The script creates 18,100 benchmark
directories plus its small policy fixture. This is a newly built application
index, **not a cold filesystem/page-cache benchmark**.

| Artifact | During indexing: median / max ms (12 edits) | Cached: median / max ms (20 edits) | Observed index completion s |
|---|---:|---:|---:|
| Committed baseline | 261.5235 / 304.621 | 1179.92 / 5381.121 | 234.288 |
| Indexing-only worker | 241.88 / 278.492 | 1180.6525 / 5410.24 | 238.577 |
| Prior async matching + validation | 35.363 / 113.211 | 33.9995 / 48.629 | 49.364 |

These are **key/paste write to visible edited field** timings, including Zellij,
rendering, PTY transport and pyte parsing. Clear is sent separately; the same
sequence and tree shape are used for each build. The cached stage proves field
editing no longer waits for Frizbee. It does **not** prove faster final ranking.
Indexing-only offload did not materially improve the cached path. Avoiding
redundant ranking on unchanged catalogue progress also reduces observed total
index duration in the final stage. Input pacing differs as a consequence of
latency; no isolated filesystem throughput claim is made. One run per artifact,
no CPU pinning, and no hard latency SLA. Raw per-edit samples are in the JSONs.

An earlier async run also completed successfully at
`target/zj-rch-48c3ee8d/benchmark.json` (33.3895 ms indexing median, 33.7825 ms
cached median), before navigation/reset safeguards. The table uses the
**prior measured artifact** `5913d562…`, not that earlier build and not the
corrected final artifact below. A parent-agent rerun of the corrected build
(`58882c902a75fc6df07727bdc148edb866651717cfa2219f71d3a361d2c7bf79`)
measured **33.162 ms** median editing latency during indexing and **32.6485 ms**
after indexing; maxima were 55.634 ms and 37.643 ms respectively. Observed index
completion was 49.855 seconds. Command: `target/verification-venv/bin/python
tools/benchmark_workers.py parent-fixed target/wasm32-wasip1/release/launchpad-plugin.wasm`.
Raw samples: `target/zj-rch-8313b0d8/benchmark.json`. The same benchmark caveats apply.

## Prior verified gates (before UI-state corrections)

1. **Worker HOME mapping:** `tools/probe_worker_home.py` passed with repository
   CWD and a different controlled HOME. Worker reported its own actual CWD and
   successfully found `/host/worker-home-witness`. After remount acknowledgement,
   a single reload created workers in HOME; the UI remained stable with no loop.
   Denial also passed. Evidence: `home-mapping.json`, `home-mapping-denied.json`.
   The controlled-host log contains:
   `MAPPING_PROBE initial=.../zj-rch-528f6203/home home=.../zj-rch-528f6203/home worker=.../zj-rch-528f6203/home|true`.
2. **Indexing-only stage:** live search harness passed 22 assertions before
   moving matching. Intermediate binary and source patch retained under
   `target/perf-index-worker/`; original live report is
   `target/zj-rch-3d7c7b32/report.json`. Benchmark is `index-worker.json`.
3. **Final async stage:** `bash tools/verify_plugin.sh` completed with exit 0:
   - 64 Rust tests passed; the one PTY-only cleanup test is separately exercised.
   - fmt, native/WASM clippy with `-D warnings`, native/WASM release builds passed.
   - Native UI and cleanup PTYs passed; full live UI harness passed 45 assertions
     in each of two independent sessions, preserving ordinary shell panes.
   - Controlled real search: plugin 22 / native 21 assertions passed.
   - Permission denial: 6 assertions passed.
   - Real HOME smoke: plugin 7 / native 6 assertions passed, storing only
     assertions and removing the disposable probe, not private listings.
4. **Worker stress:** `tools/verify_workers.py` passed 13 assertions against the
   prior measured hash: editing/backspaces during indexing, latest-query results,
   dismissal, F5 during in-flight work, new epoch, **mouse refresh discovering a
   newly created directory**, asynchronous validation, history copy, and exact
   pane-list readback after closing with outstanding work.
5. **Missing responses:** a development-only `worker-faults` build silences the
   worker after its mapping handshake. The real 15-second watchdog fired; field
   editing remained possible, submission did not launch, and unload passed
   exact pane readback (6 assertions). Normal release was rebuilt afterwards.
   An initial harness assertion used a fixed 180 ms delay for a multi-key burst;
   its partially drawn field showed that was not a valid completion wait. The
   final test uses bracketed paste plus a bounded content wait and passes.
6. **Visual check:** real PTY captures replayed in xterm.js. `real-search.png`
   shows nested fuzzy results, Unicode paths, readable status and intact borders;
   `worker-timeout.png` shows the preserved editor, visible failure and zero
   history events. These are actual rendered captures, not reconstructed UI.

Unit regressions additionally cover stale query/action generations, revision
rejection, bounded rows/bytes, denied worker mapping, stale epochs, completion
versus submission, tool/history state, reset/quit/failure, and navigation not
resubmitting searches or losing the selected path.

## Async UI-state corrections — corrected final artifact

Only `src/app.rs` and `tests/async_search.rs` changed for these two fixes; the
async worker implementation and `spikes/` were preserved.

- **Selected result followed by worker failure:** failure now clears highlight
  and completion cycle; Enter uses checked suggestion lookup. The exact
  results → Down → failure → Enter regression first failed with
  `index out of bounds: the len is 0 but the index is 0`, then passed. A separate
  stale-highlight test independently failed before adding the checked lookup.
- **Validation followed by navigation:** path cursor movement and same-focus
  navigation retain pending validation. Focus/tool/action changes cancel it,
  clear pending status and stale cycles, and allow a fresh query; Escape/help/
  quit still suppress work appropriately. Repeated Tab preserves cycling while
  replacing validation. The delayed Tab → Left → successful reply test first
  failed on rejected completion; cancellation via Focus(Tools) separately failed
  on the stuck `Validating directory…` message. Both passed after their fixes.
- Eight additional regressions bring `async_search` to **13 passing tests**.
  They cover delayed/unsent completion and launch, cursor/focus/tool changes,
  editing, dismissal/help/reset/quit, stale replies, timeout after a selected
  result, timeout during completion, fresh suggestions and repeated Tab.
  **Post-selection timeout is deterministically injected at the App boundary;
  no new live post-selection watchdog/fault run is claimed.** The prior live
  fault harness silenced the worker before results, as described above.

Corrected-build verification (all commands exited 0):

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p launchpad-plugin --target wasm32-wasip1 --locked -- -D warnings
cargo build --release --locked
cargo build -p launchpad-plugin --target wasm32-wasip1 --release --locked
target/verification-venv/bin/python tools/verify_workers.py
target/verification-venv/bin/python tools/verify_search.py
target/verification-venv/bin/python tools/verify_search.py --native
target/verification-venv/bin/python tools/verify_cleanup.py
```

Workspace tests: **72 passed, 0 failed, 1 PTY-only test ignored**; the separate
cleanup harness passed its **3 cases**. Real worker stress passed **13 assertions**
(`target/zj-rch-e2f04a6f`); controlled real plugin search passed **22**
(`target/zj-rch-3b688486`); native search passed **21** (`target/zj-rch-b0b478fc`).
The worker report records the corrected WASM hash shown below. Exact pane-list
readback confirmed plugin unload in both real plugin harnesses. Logs are
`state-fixes-checks.log`, `state-fixes-workers.log`, `state-fixes-search.log`,
`state-fixes-search-native.log` and `state-fixes-cleanup.log` in this directory.
Actual PTY captures were replayed with `tools/capture_zellij.py`; visual review
of `target/zj-rch-3b688486/01-search.png` confirmed readable nested/Unicode paths,
status and intact layout. The earlier full live-UI/performance gates above were
not rerun wholesale and remain evidence for their prior artifact only.

## Artifacts and reproduction

All paths below are relative to `/home/piclaw/Projects/zellij-launchpad`.

| Build | Saved WASM | SHA-256 |
|---|---|---|
| Baseline | `target/perf-baseline/launchpad-plugin.wasm` | `9b2185845f72e8228d77426145c1dff6a5dd1a97b8f0bbca71248f54164af04d` |
| Indexing-only | `target/perf-index-worker/launchpad-plugin.wasm` | `5f966779455e94dd7b86cee3a2ef853cd5e1cb49cb8eb6030fe792548350095c` |
| Prior measured async | `target/perf-async-worker/launchpad-plugin.wasm` | `5913d562883b47180e40f949c84eeafe7d3f71586e8fa34c2e091956b02fe3ea` |
| Corrected final | `target/wasm32-wasip1/release/launchpad-plugin.wasm` | `58882c902a75fc6df07727bdc148edb866651717cfa2219f71d3a361d2c7bf79` |

The prior measured artifact is preserved separately from the corrected final
artifact. `complete-run.log` contains the prior complete verification output.
`worker-stress.json`, `worker-fault.json` and individual search logs retain
assertions. Full PTY ANSI and replay events remain in the referenced ignored
`target/zj-*` directories; controlled large benchmark trees were removed.

```sh
bash tools/verify_plugin.sh
PY=target/verification-venv/bin/python
$PY tools/verify_workers.py
$PY tools/benchmark_workers.py baseline target/perf-baseline/launchpad-plugin.wasm
$PY tools/benchmark_workers.py index-worker target/perf-index-worker/launchpad-plugin.wasm
$PY tools/benchmark_workers.py async-final target/perf-async-worker/launchpad-plugin.wasm
```

[Implementation, pinned source references, permissions and fault-test commands](../../docs/async-workers.md).

## Remaining limits

One shared worker owns the catalogue to avoid duplication under Zellij's 16 MiB
linear-memory ceiling. Scan messages are bounded; a running Frizbee request is
not cancellable and can delay the next scan/validation in that worker. One
filesystem syscall can also block beyond the cooperative step budget. The UI
stays asynchronous, but final results are not guaranteed immediate. Limits may
truncate a large HOME visibly. Permission is broad even though traversal policy
is HOME-only. Symlink race limitations are unchanged. Launches remain simulated.

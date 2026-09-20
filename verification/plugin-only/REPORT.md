# Plugin-only verification — 2026-09-20

Records the refactor that removed the native binary and the fixture/demo
simulation layer, and renamed the crates to `zellij-launchpad-core` and
`zellij-launchpad`.

- Artifact: `target/wasm32-wasip1/release/zellij-launchpad.wasm`
- Host/SDK: Zellij 0.45.1 / zellij-tile 0.45.1, Linux aarch64
- Rust: 1.98.1, target `wasm32-wasip1`

## Build gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | passed |
| `cargo test --workspace --locked` | 121 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo clippy -p zellij-launchpad --target wasm32-wasip1 -- -D warnings` | clean |
| release WASM build | 3,318,333 bytes |

## Live-host suites run

| Suite | Checks | Result |
|---|---|---|
| `tools/verify_zellij.py` (live UI, keyboard + mouse) | 46 | passed |
| `tools/verify_history.py` (shared journal, concurrency) | 190 | passed |
| `tools/verify_search.py` | 37 | passed |
| `tools/verify_search.py --deny` | 6 | passed |
| `tools/verify_workers.py` | 13 | passed |
| `tools/verify_launch.py --tool Shell` | 24 | passed |
| `tools/verify_launch.py --commands-case agreed --tool Claude --different-cwd --refresh` | 26 | passed |

`verify_zellij.py` no longer has fixture directories to read. It now seeds a
disposable HOME, configures four commands in the layout, and runs the real
plugin under `simulate_launch "true"`, so every directory in the screenshots
below was actually indexed and validated by the worker.

## Not re-run in this pass

Two of `verify_launch.py`'s roughly thirty real-host cases were re-run, covering
the built-in Shell path and a configured command with an outside-HOME invoking
cwd plus F5. The remaining launch cases, and `tools/verify_input_limit.py`
(a 20,000-directory capacity run), were not re-run here. Run
`bash tools/verify_plugin.sh` for the complete bounded suite before tagging a
release.

## Screenshots

Captured by replaying this run's actual PTY bytes through xterm.js 6.0.0; they
are not redrawn text. The header reads "launch suppressed" because the harness
uses `simulate_launch`.

- `dashboard-80x24.png`
- `dashboard-120x36.png`
- `dashboard-40x10.png`

`live-ui-report.json` is the harness's own check list for the 46 passing checks.

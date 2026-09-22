---
name: run-tests
description: Run and diagnose zellij-launchpad tests, Rust checks, and isolated live Zellij verification. Use when asked to test this repository or verify a change before release.
---

# Running Tests

Run commands from the repository root. Read `tools/verify_native.sh` and
`.github/workflows/ci.yml` for the current checks; `README.md` documents setup.
Use the Rust toolchain pinned in `rust-toolchain.toml`.

## Full verification

On Unix, the canonical command is:

```sh
bash tools/verify_native.sh
```

It runs formatting, workspace tests, Clippy with warnings denied, a release build,
the Zellij capability probe, and the live PTY suite. It defaults
`CARGO_BUILD_JOBS` to 1; respect an existing override. Avoid running the same
checks separately before this command unless diagnosing a failure.

The live suite requires Bash, Python 3, `pyte==0.8.2`, and Zellij on PATH.
Use the Zellij version pinned in CI when reproducing CI results; the documented
minimum is 0.45.0. If the verification environment is missing, set it up with:

```sh
python3 -m venv target/verification-venv
target/verification-venv/bin/pip install pyte==0.8.2
```

The script uses `target/verification-venv/bin/python` by default. Set `PYTHON`
to another interpreter only if it has the required dependency. Follow CI's
download and checksum steps if a local Zellij installation is needed, keeping
the binary under `target/` and adding its directory to PATH for the test command.
The live harness expects `target/release/zellij-launchpad`; build for the host
with the default target directory, without a cross-compilation target override.

## Focused checks and Windows

For a targeted request, select the relevant Cargo test rather than always
running the live suite. Shared-core integration tests are in `tests/`; native
runtime tests are in `native/`. For example:

```sh
cargo test --locked -p zellij-launchpad-core --test configured_commands
cargo test --locked -p zellij-launchpad
```

For all Rust checks without live verification, including on Windows, run:

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p zellij-launchpad
```

The Bash/Python live suite requires Unix. Report explicitly when only Rust checks
ran; a missing live prerequisite does not count as a passing full verification.

## Failures and evidence

The live harness creates disposable HOME, PATH, XDG directories, sockets, and
Zellij sessions with harmless fixture executables. Preserve that isolation;
do not substitute real coding agents or use the user's active session.

Inspect `target/native-*/screen.txt`, `session.ansi`, and `executions.jsonl` for
live failures. For a focused rerun after a current release build, use the selected
Python interpreter with `tools/verify_native.py --probe` for capability checks
or `tools/verify_native.py` for the live suite. Keep generated evidence under
`target/`. The `verification/plugin-only/` and `verification/async/` records are
historical plugin evidence, not checks of the current native runtime.

Report commands run, their outcomes, skipped checks and reasons, and relevant
failure evidence paths. Distinguish an environment blocker from a test failure.

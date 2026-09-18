#!/usr/bin/env bash
# Complete bounded verification, with all artifacts kept beneath target/.
set -euo pipefail
export CARGO_BUILD_JOBS=1
cd "$(dirname "$0")/.."
PYTHON=target/verification-venv/bin/python
mkdir -p target/plugin-verification
run() {
    local name=$1; shift
    printf '\n$'; printf ' %q' "$@"; printf '\n'
    "$@" 2>&1 | tee "target/plugin-verification/$name.log"
}

run fmt cargo fmt --all --check
run tests cargo test --workspace --locked
run clippy-native cargo clippy --workspace --all-targets --locked -- -D warnings
run clippy-wasm cargo clippy -p launchpad-plugin --target wasm32-wasip1 --locked -- -D warnings
run native-build cargo build --release --locked
run wasm-build cargo build -p launchpad-plugin --target wasm32-wasip1 --release --locked
cargo tree -p launchpad-plugin --target wasm32-wasip1 --locked --edges normal > target/plugin-verification/wasm-deps.txt
"$PYTHON" -c 'from pathlib import Path; assert "crossterm" not in Path("target/plugin-verification/wasm-deps.txt").read_text()'
run native-pty "$PYTHON" tools/verify_pty.py
run native-cleanup "$PYTHON" tools/verify_cleanup.py
run search-native "$PYTHON" tools/verify_search.py --native
run search-plugin "$PYTHON" tools/verify_search.py
run search-denied "$PYTHON" tools/verify_search.py --deny
run workers "$PYTHON" tools/verify_workers.py
run input-limit "$PYTHON" tools/verify_input_limit.py
for tool in Shell Claude Codex Copilot Hermes; do
    run "launch-$tool" "$PYTHON" tools/verify_launch.py --tool "$tool"
done
run launch-tiled "$PYTHON" tools/verify_launch.py --tool Claude --layout tiled --race
run launch-floating "$PYTHON" tools/verify_launch.py --tool Codex --layout floating
run launch-tiled-nonzero "$PYTHON" tools/verify_launch.py --tool Claude --layout tiled --race --exit-code 17
run launch-floating-nonzero "$PYTHON" tools/verify_launch.py --tool Codex --layout floating --race --exit-code 17
run launch-only-nonzero "$PYTHON" tools/verify_launch.py --tool Hermes --exit-code 17
run launch-shell-race "$PYTHON" tools/verify_launch.py --layout tiled --race
run launch-remount "$PYTHON" tools/verify_launch.py --tool Hermes --different-cwd
run launch-denied "$PYTHON" tools/verify_launch.py --layout tiled --deny
run launch-missing "$PYTHON" tools/verify_launch.py --tool Copilot --layout tiled --missing
run live-1 "$PYTHON" tools/verify_zellij.py
run live-2 "$PYTHON" tools/verify_zellij.py
OUT=$("$PYTHON" -c 'import json;print(json.load(open("target/plugin-verification/live-2.log"))["evidence"])')
run screenshots "$PYTHON" tools/capture_zellij.py "$OUT"
printf '\nVerified. Final PTY records, host logs and screenshots: %s\n' "$OUT"

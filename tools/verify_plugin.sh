#!/usr/bin/env bash
# Complete bounded verification, with all artifacts kept beneath target/.
set -euo pipefail
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
run live-1 "$PYTHON" tools/verify_zellij.py
run live-2 "$PYTHON" tools/verify_zellij.py
OUT=$("$PYTHON" -c 'import json;print(json.load(open("target/plugin-verification/live-2.log"))["evidence"])')
run screenshots "$PYTHON" tools/capture_zellij.py "$OUT"
printf '\nVerified. Final PTY records, host logs and screenshots: %s\n' "$OUT"

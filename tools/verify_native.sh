#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p launchpad
"${PYTHON:-target/verification-venv/bin/python}" tools/verify_native.py --probe
"${PYTHON:-target/verification-venv/bin/python}" tools/verify_native.py

# Native migration verification — 2026-09-21

The native executable replaces the WASM plugin while retaining the Rust UI/search
core. Validation used Rust 1.98.1 on Linux x86_64.

- `cargo fmt --all --check`: passed.
- `cargo test --workspace --locked --offline`: 111 tests passed.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passed.
- Native release build and local archive/checksum verification: passed.
- Official Zellij 0.45.1 binary checked against its published SHA-256 checksum.
- Real integration scenarios passed on Zellij 0.45.0 and 0.45.1 using disposable
  HOME/PATH/XDG/session sockets and harmless fixtures. No installed agents ran.

The integration suite covers terminal keyboard/paste/mouse/resize, normal and
SIGTERM cleanup, tiled/floating replacement while a neighbor has focus, initial
and changed literal cwd, literal argument arrays, exits 0/nonzero, default-shell
and configured-command rejection/retry, deleted cwd, F5, shipped layouts,
history reopen/copy/delete, outside-HOME invoking cwd and final-pane session exit.

A CLI-only probe established two upstream details: `--close-on-exit` requires an
explicit command, and default-shell launches discard `--cwd`. The same-binary
shell handoff preserves Zellij's configured default shell and selected directory.
A missing executable can return exit 0 with no pane ID; readback fences rejection.

Native executable SHA-256:

```text
947328019d709f61ae0ca084652796db9a72cfe9ac9287227c4f459e442c8412
```

Local evidence: `target/native-tests.log`, `target/native-clippy.log`,
`target/native-live-0.45.0.log`, `target/native-live-0.45.1.log`, and per-session
`target/native-*/{screen.txt,session.ansi,executions.jsonl}`. The repeatable entrypoint
is `bash tools/verify_native.sh`. Earlier plugin records are historical.

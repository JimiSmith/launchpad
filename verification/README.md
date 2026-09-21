# Verification records

[Native migration results](native/REPORT.md).

Current native checks run through `bash tools/verify_native.sh`. Unit/check logs
and live PTY evidence are generated under `target/`; sessions use disposable HOME,
PATH, XDG state/config and sockets, with harmless fixture executables.

The `plugin-only/` and `async/` directories are historical plugin evidence. Their
logs and measurements predate the native migration and do not describe the native
runtime. They are retained unedited for provenance.

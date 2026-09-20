# Verification records

Dated evidence from bounded, isolated verification runs. Each record describes
the build it was produced from, not necessarily the current one.

- `plugin-only/` — the current record: the plugin-only refactor (2026-09-20),
  covering build gates and the live-host suites that were re-run.
- `async/` — the asynchronous worker migration, including the only performance
  measurements taken on this host. Referenced from
  [docs/async-workers.md](../docs/async-workers.md).
- `versions.json` — pinned toolchain and Ratatui provenance.

`async/` holds verbatim captured logs and JSON from runs that predate the crate
rename, so they name `zellij-launchpad-prototype` and `launchpad-plugin.wasm`.
That is the historical record and is left unedited.

Records of the removed native binary and of the fixture-only demo plugin were
deleted once both were removed from the codebase; `git log` retains them.

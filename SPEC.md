# Launchpad native specification

Launchpad is a Linux x86_64 native Rust executable requiring Zellij >= 0.45.0.
The dashboard uses Ratatui/Crossterm with a reusable pure application core.

The behavioral contract and configuration are documented in [README.md](README.md).
Implementation contracts:

- [Own-pane launches, default-shell handoff and failure handling](docs/real-launch.md)
- [TOML commands and colours](docs/configured-commands.md)
- [HOME search and validation](docs/home-search.md)
- [Native background worker](docs/async-workers.md)
- [Shared recent history](docs/history.md)
- [Native build and release packaging](docs/releases.md)

Preserve the existing centered dashboard, keyboard/mouse controls, Unicode input,
HOME-only search policy, exact invoking-cwd exception, ten-pair tool–directory history,
stable command IDs and close-on-exit tool lifecycle. Configuration lives outside
Zellij in XDG TOML; history lives in XDG state. Existing plugin caches are untouched.

Outside Zellij the executable reports an error; help/version remain available.
Do not create/attach sessions automatically or directly launch tools outside
Zellij. Do not use shell interpolation, terminal command injection, automatic
launch retries, or a focused-pane fallback. Quit restores the invoking terminal.

The release contains no WASM plugin or separate companion helper executable.
The internal default-shell handoff uses the same binary. Plugin compatibility,
old-history import and non-Linux releases are outside this migration.

# Launchpad

A new-tab dashboard for Zellij. Fuzzy-find a directory, choose a shell or coding
agent, and revisit your last ten launches.

Directory search is real and restricted to HOME. Tool availability and launches
are still simulated; history starts empty and is kept only in memory.

![Launchpad](verification/plugin/01-initial-80x24.png)

## Run in Zellij

Requires Rust (via rustup) and Zellij. Tested with Zellij 0.45.1.
Run these commands from the repository root:

```sh
rustup target add wasm32-wasip1
cargo build --locked --release -p launchpad-plugin --target wasm32-wasip1
```

Then, from a shell inside Zellij:

```sh
zellij action launch-plugin --skip-plugin-cache -- \
  "file:$(pwd)/target/wasm32-wasip1/release/launchpad-plugin.wasm"
```

The plugin is a single `.wasm` file with no companion executable. After rebuilding,
close the old plugin pane and launch it again with the command above.
Zellij 0.45.1 requires session-environment access, **Full disk access**, and
**Change application state** (one startup reload to map workers to HOME).
Indexing, matching, and validation run off the UI; no commands are executed.
The worker continuously schedules bounded scan slices without UI timer pacing,
yielding between slices for search/validation and publishing periodic progress.

Use Zellij's locked mode (normally `Ctrl+G`) so shortcuts reach the plugin.
`Ctrl+Q` closes the plugin pane, not the session.

## Controls

- `Ctrl+P` / `Ctrl+T` / `Ctrl+R`: focus the path, tool selector, or history.
- Type `notes` or `nts` to try fuzzy directory matching. Use arrows and `Enter`
  to accept a suggestion, then choose a tool and press `Enter` to simulate a launch.
- Click to select directories, tools, or history; click the launch/replay action
  to run the simulation. Scroll to browse lists.
- `Esc`: go back or dismiss suggestions. `F1`: full keyboard help.
- `F5`: refresh HOME and reset the form/history. `F6`: toggle simulated Copilot availability.

The UI is centered and capped at 160 terminal columns. Relative paths start at
HOME. The index respects `.gitignore` and `.ignore`, and prunes hidden directories,
`.git`, and `node_modules`. Hidden/ignored paths can still be entered literally;
symlinks are rejected. Indexing is incremental and capped, with visible limits.

Typing and paste are capped at **100 Unicode scalar values** (not UTF-8 bytes or
visual graphemes); excess input is ignored. Completed/history paths are kept
intact even when longer, and remain valid launch targets; delete or clear them
before inserting more text. Fuzzy queries over 100 scalars, including after HOME
expansion, return no suggestions rather than allocating oversized matcher
buffers. Bare short queries still find long paths; literal path validation and
the separate 4096-byte filesystem safety limits are unchanged.

## Standalone prototype

The same UI also runs outside Zellij:

```sh
cargo run --locked --release
```

## Development

Built with Ratatui, the serial `ignore` walker, and embedded Frizbee matching. The native executable and Zellij
plugin share the application state and renderer.

```sh
cargo test --workspace --locked
```

See the [spec](SPEC.md), [HTML mockup](design/launchpad.html), and
[search implementation notes](docs/home-search.md) for policy and verification.

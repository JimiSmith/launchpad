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
Zellij 0.45.1 requires session-environment access and **Full disk access** to
resolve HOME and mount it. Launchpad searches only HOME and executes no commands.

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
HOME. Hidden results require a dot-prefixed component; symlinks are skipped.
Indexing is incremental and capped; its status shows when a limit is reached.

## Standalone prototype

The same UI also runs outside Zellij:

```sh
cargo run --locked --release
```

## Development

Built with Ratatui and embedded Frizbee matching. The native executable and Zellij
plugin share the application state and renderer.

```sh
cargo test --workspace --locked
```

See the [spec](SPEC.md), [HTML mockup](design/launchpad.html), and
[search implementation notes](docs/home-search.md) for policy and verification.

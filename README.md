# Launchpad

A native directory-and-tool dashboard for Zellij. Fuzzy-find a directory, choose
Shell or a configured command, and replace the dashboard's pane with that tool.
Launchpad names the originating tab `directory · command label`; tool panes close
on exit without returning to the dashboard.

Runs on Linux and Windows with **Zellij 0.45.0 or newer**. Launchpad must run inside a
Zellij terminal pane. No plugin, WASM runtime, or plugin permissions are needed.

## Install and run

Build from this checkout with the pinned Rust toolchain:

```sh
cargo build --release --locked -p zellij-launchpad
install -Dm755 target/release/zellij-launchpad ~/.local/bin/zellij-launchpad
```

On Windows, build with the same Cargo command, then run from PowerShell inside
Zellij:

```powershell
.\target\release\zellij-launchpad.exe
```

Launchpad uses `HOME`, falling back to `USERPROFILE`, then `HOMEDRIVE` +
`HOMEPATH`. Windows paths accept drive letters, UNC shares, and either slash style;
`~` refers to that home directory. Drive-relative paths such as `C:notes` are
rejected. The published release archive currently targets Linux; build Windows
from source.

Run `zellij-launchpad` inside Zellij, or start it in its own pane:

```sh
zellij run --close-on-exit -- zellij-launchpad
```

For a new tab, use [examples/launchpad.kdl](examples/launchpad.kdl):

```sh
zellij action new-tab --layout "$(pwd)/examples/launchpad.kdl"
```

Ensure `zellij-launchpad` is on the Zellij server's PATH. Layouts also accept an
absolute executable path. Use Zellij's locked mode (normally Ctrl+G) so shortcuts
reach Launchpad. Quit returns to the invoking shell, or closes a dedicated pane
started with `--close-on-exit` / `close_on_exit true`.

The input starts at the invoking cwd; **Enter once launches Shell** even while
HOME is indexing. Shell uses Zellij's configured default shell. Configured tools
receive the chosen directory and literal argument arrays. No command availability
or authentication checks run before launch.

## Configure commands and colours

The default file is `$XDG_CONFIG_HOME/zellij-launchpad/config.toml`, falling back
to `~/.config/zellij-launchpad/config.toml`. Override it with
`zellij-launchpad --config /path/to/config.toml`. Reopen to apply edits; F5
preserves the loaded configuration. Missing default configuration means Shell-only.
An explicit missing file, unreadable file, or malformed TOML is a startup error.

```toml
[[commands]]
id = "claude"
label = "Claude in Worktree"
executable = "claude"
arguments = ["-w"]

[[commands]]
id = "hermes"
executable = "hermes"

[[commands]]
id = "codex"
label = "Codex"
executable = "codex"

[theme]
background = "default"
surface = "#1e2030"
raised = "#363a4f"
border = "#494d64"
text = "#cad3f5"
muted = "#a5adcb"
accent = "#c6a0f6"
on_accent = "#181926"
error = "#ed8796"
```

Shell stays first. Commands follow file order; the first duplicate ID wins.
Labels default to IDs. Invalid individual definitions are skipped with a visible
error, while valid commands stay usable. F1 lists all configuration errors.
Colours default to Catppuccin Macchiato over the terminal background; invalid
colour values keep their defaults.

Arguments never expand `$HOME`, `~`, globs, or shell operators. Use an explicit
shell executable with `arguments = ["-c", "your script"]` if you want shell
interpretation. [Configuration details](docs/configured-commands.md).

## Controls and directory policy

- Ctrl+P / Ctrl+T / Ctrl+R: focus path, tools, or history.
- Type a fuzzy query; arrows select suggestions, Enter accepts, Tab completes.
  Enter without a highlighted suggestion launches the selected tool.
- Left/Right choose a tool. Mouse clicks select; the launch button launches.
- F1 opens help. F5 refreshes HOME and shared history and restores the invoking cwd.
- In history: Tab copies, Enter revalidates/replays, Delete removes, Ctrl+L twice
  clears; Esc cancels confirmation.
- Ctrl+Q / Ctrl+C quits. Esc dismisses transient UI, then quits an untouched dashboard.

Search is HOME-only; relative paths start at HOME. The index respects local
`.gitignore` / `.ignore`, excludes hidden directories, `.git`, and `node_modules`,
and never follows symlinks. Hidden and ignored directories can be entered
literally. The exact invoking cwd is the sole exception for outside-HOME or
symlink directories. Deleted/inaccessible directories fail visibly without fallback.

Typing/paste is capped at 100 Unicode scalar values; completed, invoking, and
history paths are never truncated. Index limits are 200,000 directories,
2,000,000 entries, depth 64, and 60 MiB of estimated retained index data. Search,
traversal, and validation run on a bounded background worker so input remains
responsive. [Search policy](docs/home-search.md).

Recent history remembers ten unique directories and their last command IDs under
`$XDG_STATE_HOME/zellij-launchpad` (fallback `~/.local/state/zellij-launchpad`).
It is shared across panes and sessions. F5 reloads other instances' changes.
Replay uses the current configuration; removed IDs remain unavailable.
History records validated attempts, not successful tool execution.
[Persistence details](docs/history.md).

## Migrating from the plugin

Replace each `plugin location="…wasm"` block with an ordinary pane command;
see the layouts in `examples/`. Move command definitions into TOML:
`command_claude "claude"` becomes `executable = "claude"`,
`arguments_claude "-w"` becomes `arguments = ["-w"]`, and theme keys move into
`[theme]` without their `theme_` prefix. Native history starts fresh; existing
plugin caches are left untouched. WASM builds and plugin installation are no
longer supported.

## Development

`zellij-launchpad-core` owns the state machine, renderer and search implementation;
`native/` owns terminal input, TOML, worker threads, persistence, and CLI launches.

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p zellij-launchpad
python3 -m venv target/verification-venv
target/verification-venv/bin/pip install pyte==0.8.2
bash tools/verify_native.sh
```

The live suite uses isolated Zellij sessions, disposable HOME/state directories,
and harmless executable fixtures; it never launches installed coding agents.
The Bash/Python live suite requires Unix; Windows runs native builds, tests, and
Clippy in CI.
See [launch behavior](docs/real-launch.md), [worker design](docs/async-workers.md),
and [release packaging](docs/releases.md).

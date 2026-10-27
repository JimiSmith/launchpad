# Launchpad

Launchpad is a directory and tool picker for [Zellij](https://zellij.dev),
[herdr](https://herdr.dev) and [tuios](https://tuios.dev). You choose a directory
and a tool, and Launchpad runs the tool in its own pane. It runs on Linux, Windows
and macOS with Zellij 0.45.0 or newer, or inside herdr (tested with 0.9.0). It
also runs in tuios (tested with 0.11.0 on Linux).

## Install

Download the archive for your platform from the
[releases page](https://github.com/JimiSmith/launchpad/releases). Make sure that
the archive matches its `.sha256` file. Then put `launchpad` on your PATH.

To build from source, install [Rust](https://rustup.rs) and run:

```sh
cargo build --release --locked -p launchpad
install -m755 target/release/launchpad ~/.local/bin/launchpad
```

If you use herdr or tuios, you can install the plugin instead. See
[herdr and tuios](#herdr-and-tuios).

## Zellij key binding

Add this binding to your Zellij `config.kdl`. The key opens Launchpad in a new
tab. Use the absolute path of `examples/launchpad.kdl` from this repository or
from the release archive.

```kdl
keybinds {
    shared {
        bind "Alt l" { NewTab { layout "/absolute/path/to/launchpad.kdl"; }; }
    }
}
```

Use Zellij's locked mode (normally Ctrl+G) while Launchpad is open. Otherwise
Zellij gets the keys before Launchpad.

## herdr and tuios

Launchpad is a plugin for herdr. tuios runs herdr plugins, so the same plugin
works in tuios.

Install the plugin with herdr:

```sh
herdr plugin install JimiSmith/launchpad
```

Install the plugin with tuios, from a terminal outside tuios:

```sh
tuios plugins install JimiSmith/launchpad
```

If you installed the plugin with herdr, tuios finds it. To use it in tuios,
run `tuios plugins enable launchpad` from a terminal outside tuios.

The plugin does not put `launchpad` on your PATH. To update the plugin, run the
install command again.

The plugin has two actions. `launchpad.open` opens Launchpad in a new tab.
`launchpad.open-workspace` opens Launchpad in a new workspace.

For herdr, add these bindings to herdr's `config.toml`, then run
`herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+alt+l"
type = "plugin_action"
command = "launchpad.open"
description = "launchpad"

[[keys.command]]
key = "prefix+alt+w"
type = "plugin_action"
command = "launchpad.open-workspace"
description = "launchpad in a new workspace"
```

For tuios, add these bindings to tuios's `config.toml`, then run
`tuios config apply`. Run `tuios config path` to find the file.

```toml
[[keybindings.command]]
key = "prefix+alt+l"
type = "plugin_action"
command = "launchpad.open"
description = "launchpad"

[[keybindings.command]]
key = "prefix+alt+w"
type = "plugin_action"
command = "launchpad.open-workspace"
description = "launchpad in a new workspace"
```

In herdr, you can also bind a key without the plugin if `launchpad` is on your
PATH:

```toml
[[keys.command]]
key = "prefix+alt+l"
type = "pane"
command = "exec launchpad"
description = "launchpad"
```

On Windows, write `command = "launchpad"`.

## Configuration

Launchpad reads `~/.config/launchpad/config.toml`. See
[examples/config.toml](examples/config.toml) and
[configured commands](docs/configured-commands.md). Press F1 in Launchpad to
see all keys.

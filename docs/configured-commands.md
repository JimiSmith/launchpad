# Configured commands (Zellij / SDK 0.45.1)

Only the plugin's Zellij configuration defines production commands. A direct
unconfigured WASM launch shows **Shell only**, initially selected. Shell uses
`open_terminal_in_place_of_plugin`, so Zellij's configured default shell wins over
`SHELL`. There are no built-in production agents, discovery, PATH probes, or
availability filtering. An unconfigured plugin shows Shell alone.

See [the complete KDL layout](../examples/configured.kdl). Inside a plugin block:

```kdl
commands "claude,hermes,codex"
command_claude "claude"
arguments_claude "-w"
label_claude "Claude in Worktree"
command_hermes "hermes"
label_hermes "Hermes"
command_codex "codex"
label_codex "Codex"
```

`commands` is an ordered comma-separated ID list. Trim each ID, ignore empty
segments, and keep its first occurrence (even if invalid). Unlisted definitions
are ignored. IDs are case-sensitive; `shell` is reserved and cannot be overridden.
`Shell` is a distinct user ID, not a spelling alias for the built-in.

Each listed ID requires a nonblank `command_<id>`. Its value is **one literal
executable name/path**, not a command line: spaces in a path are allowed and
preserved, and Launchpad never splits the executable field. Thus `claude -w`
means an executable literally named `claude -w`, not `claude` with a flag.
Use `arguments_<id>` for arguments. Missing arguments mean an empty argv; missing
`label_<id>` uses the ID. Multiple IDs can share an executable.

Argument parsing uses pinned `shlex` 2.0.1 (POSIX-style lexical quoting/escaping):

```kdl
arguments_claude "-w --model \"some model\" ''"
```

This passes `-w`, `--model`, `some model`, and one empty argument. Quoted empty
strings are preserved, adjacent quoted/unquoted fragments concatenate, backslash
escapes follow shlex rules, and a `#` beginning an unquoted word starts a comment.
There is **no expansion or implicit shell**: variables, tilde, globs, pipes,
redirection and command substitution remain literal argv text. A command
substitution containing spaces must be quoted to remain one argument, just like
other text. To deliberately use shell interpretation, configure it explicitly:

```kdl
commands "script"
command_script "sh"
arguments_script "-c 'printf \"%s\\n\" \"$PWD\"'"
label_script "Explicit shell script"
```

The selected validated directory remains structured cwd in every case.

## Bounds and diagnostics

- ID: 1–64 ASCII letters, digits, `_`, `.`, or `-` (no leading-character restriction).
- `commands`: at most 8192 UTF-8 bytes; oversized lists give Shell only and an error.
- First 64 distinct nonempty listed IDs are considered, including invalid/reserved
  IDs; remaining entries are skipped with an error. Duplicates do not consume slots.
- Executable: at most 4096 UTF-8 bytes, nonblank, no control characters.
- Raw arguments: at most 16384 UTF-8 bytes, no NUL; at most 256 parsed arguments.
- Label: at most 256 UTF-8 bytes, nonblank, no control characters.

Invalid entries are excluded with actionable field diagnostics; valid entries
and Shell stay usable. Errors do not expose raw invalid ID/control text. The
first error/count is visible on the dashboard when no transient status is active;
F1 lists all errors and all full configured labels. No executable is checked until
Zellij attempts the launch. Fix installation/PATH and explicitly retry after a
host rejection; there is no automatic fallback.

The selector wraps into at most two rows and follows keyboard selection. At short
sizes with errors/history focus it uses one row to retain other content. Overflow
gets a position/count and clickable ‹/› controls. Labels clip by terminal cells;
F1 scrolls rendered wrapped rows, exposing full text even at 40×10. Below
40×10 launch controls remain disabled. All views
retain the 160-column cap and noninteractive gutters.

## Lifetime and history

Configuration is parsed at load, retained across form reset/F5 app recreation,
and supplied again by Zellij during the guarded HOME worker reload. Open a newly
configured instance to change settings; there is no file watcher/settings editor.

History version 2 stores stable IDs only. Replay resolves against the current
instance: label changes preserve identity, while changed executable/arguments
apply to subsequent replay. Removed/invalid IDs retain a leading `!` even when
the tool column clips and the age/status column is hidden.
Tab can copy their directories, but preserves the unavailable ID until the user
explicitly selects a valid command. There is no substitution of Shell/another ID.
Only version-2 journal records are supported; old cache is ignored without
migration. See [history.md](history.md).

## Host configuration caveats

Use a layout or a KDL plugin alias for multi-command lists. Zellij 0.45.1's CLI
`--configuration` parser unconditionally splits at **every comma**, including
commas inside quotes. It cannot encode the multi-ID value above. This is a host
CLI limitation, not Launchpad argument parsing. KDL values are forwarded intact.

For aliases, add a `plugins { launchpad location="file:/absolute/plugin.wasm" {
…settings… } }` block to your Zellij config, then use
`zellij action launch-or-focus-plugin launchpad` (fresh configs create an instance;
otherwise this may focus the existing matching instance). The 0.45.1
`action launch-plugin` CLI requires a URL rather than a bare alias. The complete
layout recipe avoids both CLI limitations and always creates a new instance.

## Verification

`tests/configured_commands.rs` checks parsing, bounded invalid definitions,
structured validated requests, and selected-control hit maps across all accepted
commands at 40×10, 40×12, 80×24 and 200×36. Plugin tests exercise HOME/reset state,
labels/errors, and the actual pinned KDL parser. Store tests cover stable IDs,
unsupported old-format cache, bounds and existing concurrent persistence.
Real-host tests use only disposable HOME/PATH fixtures beneath `target/`, never
actual coding agents. Evidence and exact artifact hashes are summarized in
`target/configured-commands/REPORT.md`.

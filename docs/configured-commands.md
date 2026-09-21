# TOML configuration

Read `$XDG_CONFIG_HOME/zellij-launchpad/config.toml`, falling back to
`~/.config/zellij-launchpad/config.toml`, or supply `--config PATH`. Relative
explicit paths resolve from the invoking cwd. Relative XDG environment values
are ignored. The file is limited to 1 MiB and loaded once per invocation.

An ordered `[[commands]]` array holds `id`, `executable`, optional `label`, and
optional `arguments` (a string array). Shell is always first; `shell` is reserved.
Labels default to IDs and arguments default to an empty array. First duplicate
ID wins; at most 64 unique configured IDs are accepted.

- IDs: 1–64 ASCII letters, digits, `_`, `.` or `-`; case-sensitive.
- Executable: one nonempty literal name/path, at most 4096 bytes, no controls.
- Arguments: at most 256 values / 16384 total bytes, no NUL. Empty arguments,
  spaces and shell metacharacters remain literal. No expansion or shell splitting.
- Label: nonempty, at most 256 bytes, no controls.

Invalid individual definitions produce visible errors and are skipped. Missing
default files mean Shell-only; explicit missing files, unreadable files, malformed
TOML and invalid top-level structure fail startup. Unknown top-level settings are
errors. F1 lists per-command and theme errors.

`[theme]` accepts `background`, `surface`, `raised`, `border`, `text`, `muted`,
`accent`, `on_accent`, and `error`, with `#RRGGBB` or `default` strings. Invalid
colours retain the corresponding default and appear in configuration errors.
Unknown colour names are reported. See [the complete example](../examples/config.toml).

Stable IDs are stored in history; executables and argument lists are never stored
there. Current configuration controls replay. Removed IDs remain unavailable.
F5 preserves commands and colours; reopen after editing configuration.

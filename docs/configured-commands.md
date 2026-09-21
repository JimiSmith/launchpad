# TOML configuration

Read `$XDG_CONFIG_HOME/zellij-launchpad/config.toml`, falling back to
`~/.config/zellij-launchpad/config.toml`, or supply `--config PATH`. Relative
explicit paths resolve from the invoking cwd. Relative XDG environment values
are ignored. The file is limited to 1 MiB and loaded once per invocation.

An optional top-level `ignore` array lists absolute directory paths to exclude
from indexing, including all descendants. Declare it before any `[[commands]]`
or `[theme]` tables, for example:

```toml
ignore = ["/home/james/cache", "/home/james/old-projects"]
```

Entries are literal paths, not patterns: no expansion of `~`, environment
variables, or globs. Separators, trailing separators, `.` and `..` are normalized
lexically without resolving symlinks or requiring directories to exist. Matching
uses path components and existing host case rules (case-sensitive on Unix,
ASCII case-insensitive on Windows, including drive and UNC paths).
Ignoring HOME or an ancestor empties the index; unrelated paths outside HOME
have no effect. Duplicate and overlapping exclusions are harmless.

Invalid entries (including non-strings, relative paths, paths over 4096 bytes,
and paths with controls) are skipped with visible configuration errors and F1
details; valid entries remain active. Error positions are one-based. A non-array
`ignore` value fails startup. Omitted or empty arrays add no exclusions.
Literal path entry and history launches retain their usual validation rules.

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
errors. F1 lists per-command, theme and ignore errors.

`[theme]` accepts `background`, `surface`, `raised`, `border`, `text`, `muted`,
`accent`, `on_accent`, and `error`, with `#RRGGBB` or `default` strings. Invalid
colours retain the corresponding default and appear in configuration errors.
Unknown colour names are reported. See [the complete example](../examples/config.toml).

Stable IDs are stored in history; executables and argument lists are never stored
there. Current configuration controls replay. Removed IDs remain unavailable.
F5 preserves commands, colours and exclusions; reopen after editing configuration.

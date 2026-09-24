# TOML configuration

Read `$XDG_CONFIG_HOME/launchpad/config.toml`, falling back to
`~/.config/launchpad/config.toml`, or supply `--config PATH`. Relative
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
The top-level `default_shell` string sets Shell's executable, with the same
rules as a command's `executable` and no arguments. An invalid value is a visible
configuration error and leaves the host default in place.
Labels default to IDs and arguments default to an empty array. First duplicate
ID wins; at most 64 unique configured IDs are accepted.

- IDs: 1–64 ASCII letters, digits, `_`, `.` or `-`; case-sensitive.
- Executable: one nonempty literal name/path, at most 4096 bytes, no controls.
- Arguments: at most 256 values / 16384 total bytes, no NUL. Empty arguments,
  spaces and shell metacharacters remain literal. No expansion or shell splitting.
- Label: nonempty, at most 256 bytes, no controls.
- Shortcut: optional key combination; see [Shortcuts](#shortcuts).

Invalid individual definitions produce visible errors and are skipped. Missing
default files mean Shell-only; explicit missing files, unreadable files, malformed
TOML and invalid top-level structure fail startup. Unknown top-level settings are
errors. F1 lists per-command, theme and ignore errors.

## Shortcuts

`shortcut = "alt+c"` launches the command without selecting it first. From the
path or tools section it uses the typed directory text, never a highlighted
suggestion; from the recent section it uses the selected row's directory and
this command instead of the row's tool. It validates like Enter; on failure the
form keeps this command selected. Shortcuts are ignored while the keys screen
is open; F1 lists every configured shortcut in its Launch group.

Syntax is `modifier+…+key`, case-insensitive:

- Modifiers: `ctrl`, `alt`, `super`, `shift`. At least one of Ctrl, Alt or
  Super is required; Shift alone is typing.
- Keys: one visible character; `f1`–`f24`; or `enter`, `tab`, `space`,
  `backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, `up`,
  `down`, `left`, `right`, `esc`. `alt++` names the plus key.
- An uppercase letter means Shift: `alt+C` equals `alt+shift+c`. Shift is
  rejected with other characters; terminals disagree on how they report
  shifted punctuation, so prefer letters, digits and named keys.

A shortcut that a built-in key already handles (for example `ctrl+p`,
`ctrl+left` or `ctrl+shift+u`) or that an earlier command already uses is
dropped with a configuration error; the command remains available. Shell has
no shortcut because Enter launches it by default.

Launchpad requests the Kitty keyboard protocol's disambiguation level, which
Zellij forwards. With a host terminal that supports it (kitty, WezTerm, foot,
Ghostty), combinations such as Ctrl+I, Ctrl+M and
Ctrl+Shift+letter are distinct. Legacy terminals send Ctrl+I as Tab, so such a
shortcut never fires there; it cannot trigger by accident. Launchpad cannot
detect the host terminal's support, because Zellij answers on its behalf. Alt
shortcuts work everywhere; on macOS, enable Option as Meta. The protocol is
popped before a launch so tools inherit the normal encoding.

`[theme]` accepts `background`, `surface`, `raised`, `border`, `text`, `muted`,
`accent`, `on_accent`, and `error`, with `#RRGGBB` or `default` strings. Invalid
colours retain the corresponding default and appear in configuration errors.
Unknown colour names are reported. See [the complete example](../examples/config.toml).

Stable IDs are stored in history; executables and argument lists are never stored
there. Current configuration controls replay. Removed IDs remain unavailable.
F5 preserves commands, colours and exclusions; reopen after editing configuration.

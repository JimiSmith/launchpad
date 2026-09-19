# Zellij Launchpad — product specification

Sections 2–10 describe the current product contract. Deferred features and
verification gaps are listed separately in section 11; they are not claims of
implemented behaviour. The production plugin provides HOME-only fuzzy directory
search, configured commands, own-pane launches, and shared recent history.
Native/demo adapters simulate launches and do not access persistent history.

Implementation details: [directory search](docs/home-search.md),
[commands](docs/configured-commands.md), [launching](docs/real-launch.md), and
[history](docs/history.md). Zellij host/SDK baseline: **0.45.1**, verified on Linux.

## 1. Existing solutions and differentiation

Research found adjacent tools, but no verified exact match for a new-tab dashboard combining path autocomplete, installed-tool selection, and ten recent launches. This is a bounded finding, not proof that none exists.

The community catalogue describes **zellij-newtab-plus** as named-tab creation with zoxide navigation, **ghost** as a floating command-terminal launcher, and **zellij-ai-session** as finding and resuming coding sessions by project. These descriptions were read in the catalogue; their individual implementations were not verified.[1]

**zellij-claude** discovers running Claude sessions, filters them by directory/name/status, and supports switching across Zellij sessions. Its documented focus is managing existing sessions rather than this launch workflow.[4]

**Product distinction:** directory-first, multi-tool, disposable new-tab launcher. Not an agent monitor, conversation browser, or project/session manager.

## 2. Product contract

Opening a dashboard tab gives the user an immediately focused directory input, a configured-command selector, and recent launch history. Selecting a command launches it in the chosen directory and replaces the dashboard pane in the same tab.

Current defaults:

- Platform: local Linux host running Zellij. macOS verification remains outstanding.
- Exactly one launch per activation; no automatic agent resume.
- Initial directory input: the original host-supplied invoking cwd, abbreviated under HOME. Capture it before the HOME worker remount/reload; F5/reset restores it. One Enter when ready validates and opens Shell there, including during indexing; no suggestion is initially highlighted. No configured starting-directory fallback.
- Initial tool: default shell. Do not silently start a remembered agent.
- Command order: built-in Shell first, followed by the IDs listed in `commands`, in user-specified order. There are no built-in agent commands.
- Recent list: ten unique absolute directories, newest first, remembering the last tool.
- History persists across panes and Zellij sessions sharing the same plugin URL and
  Zellij cache. Changing the URL or deleting that cache starts separate history.
- No required fzf, zoxide, Python, or shell framework.

The selected directory is the working directory. Executable resolution is left to Zellij's normal launch environment; Launchpad does not probe PATH, run availability checks, or hide commands because an executable is missing.

## 3. Screen

Illustrative only; configured labels and history vary:

```text
  Launchpad

  Directory
  > ~/Projects/my-app/_________________________________
    my-app/                 my-app-tests/

  Launch with
  [ Shell ]   Claude   Codex   Copilot   Hermes

  Recent launches
    Claude    ~/Projects/my-app                 12m ago
    Shell     ~/Projects/notes                   1h ago
    Codex     ~/Projects/service                 3h ago

  Tab complete · ↓ tools · Ctrl+R recent · Enter launch
```

The input is the primary visual element. Suggestions occupy a capped region immediately below it; commands stay below that region and history below commands. Display configured labels in the configured order after Shell. Configuration errors must be visible without making Shell unusable; there is no installed-tool discovery phase.

Responsive behaviour:

- Maximum UI width: 160 terminal columns, including horizontal padding. Center the UI in wider terminals; side gutters are non-interactive. Apply the same cap to the dashboard, help, and simulated terminal/closed views.
- At least 80×24: full layout and up to ten history rows.
- Smaller viewports: scroll history, wrap tool choices, preserve path input and errors.
- Below 40×10: explicit compact/resize notice; never launch because controls are hidden.
- Measure and truncate by terminal cells, not bytes. Exclude invalid UTF-8 and control-character directory names instead of displaying ambiguous paths. Preserve valid literal paths separately from clipped display labels.

## 4. Directory input and autocomplete

- Search recursively beneath HOME using embedded Frizbee (`neo_frizbee`), not a custom ranking algorithm or external executable. A query such as `notes`, `nts`, or `Projects/notes` finds matching indexed directories without requiring shell-style relative-path navigation. Accepting a completion fills the actual directory path; completion itself never launches.
- Accept explicit `~`, `~/…`, and absolute paths under HOME. Existing literal validation also resolves HOME-relative input and `.`/`..` within HOME; it does not use the invoking cwd. Adding invoking-cwd-relative resolution is out of scope, not deferred work.
- Expand only the leading home shortcut. No environment-variable, glob, command-substitution, or arbitrary shell-expression expansion. Spaces and quotes in paths are literal; do not insert shell quoting.
- Search only HOME. Ordinary validation rejects outside-HOME paths and all symlink components. The exact original invoking cwd is the sole exception: validate it via a main-instance remount, permitting outside-HOME/symlink cwd without indexing there or allowing other outside paths. Do not create missing directories. Failed cwd never falls back to HOME.
- Prune hidden directories (including `.git`) and exact `node_modules` subtrees at every depth. Apply HOME-local `.gitignore` and `.ignore` rules. A dot-prefixed query does not reveal excluded candidates; explicit literal hidden/ignored directories can still validate and launch. Excluded entries are not catalogued or counted; no skipped list/count is shown.
- Exclude invalid UTF-8 and control-character directory names. Reject U+FFFD in the invoking cwd because the pinned host serializes paths lossily, even if that character was legitimate; never open a replacement twin. Validate the actual directory on completion acceptance and again on launch, preserving the form on failure. Revalidation is not an atomic guarantee against concurrent filesystem replacement.
- Run indexing, matching, and ordinary HOME validation in a persistent WASM worker. The exact invoking-cwd exception is validated by the main instance after a host remount acknowledgement. Autonomous bounded scan slices publish progress; query coalescing and generation/revision checks prevent stale results from replacing current ones. Keep previous suggestions visible until the accepted reply arrives.
- Keep the index in memory. Reopen/F5 rebuilds it; persistent index caching is not implemented. Never rescan on each keystroke.
- Bound indexing to 20,000 directories, 200,000 entries, depth 64, a conservative 6 MiB retained-path/rule budget, and 4096-byte paths. Limits are visible. Return at most 100 ranked suggestions; an explicit valid path need not appear in the partial index.
- Cap typed/pasted input and fuzzy queries at 100 Unicode scalar values. Preserve longer invoking/completed/history paths without truncating them into different targets. See [search details](docs/home-search.md) for rule budgets and matching bounds.

Rendering performs no filesystem I/O. Worker scheduling keeps search work off
plugin input handling, but a filesystem syscall cannot be interrupted by a slice
budget. History persistence also performs bounded synchronous I/O outside render.
No hard input-latency or slow-mount guarantee is claimed. The single WASM artifact
and embedded matcher have been exercised in real Zellij; the full fff engine is
not used.[18][19]

## 5. Keyboard contract

Three focus areas: path, tools, history. Footer help reflects current focus.

| Focus | Key | Action |
|---|---|---|
| Path | Tab / Shift+Tab | Accept next / previous completion; never launch |
| Path | Up / Down with suggestions | Move suggestion highlight |
| Path | Enter with highlighted suggestion | Accept suggestion and dismiss suggestions; do not launch |
| Path | Enter without highlighted suggestion | Validate and launch currently selected tool |
| Path | Down without suggestions | Focus tools |
| Tools | Left / Right | Select visible tool |
| Tools | Down / Up | Focus history / path |
| Tools | Enter | Validate and launch |
| History | Up / Down | Select entry |
| History | Enter | Revalidate and relaunch the selected entry |
| History | Tab | Copy entry's path and tool into the form for editing; no launch |
| Any | Ctrl+P / Ctrl+T / Ctrl+R | Focus path / tools / history |
| Any | Esc | Dismiss suggestions or inline transient UI first; otherwise close the untouched dashboard pane |

No history: show an empty state; history shortcuts are harmless. On a storage read failure, show an error and retain previously loaded rows where available. If an entry's command was removed, show it as unavailable even in narrow layouts and let the user copy its directory into the form, but never silently substitute another command.

Key delivery is exercised in normal and locked modes. Zellij may intercept shortcuts in normal mode; use locked mode for full plugin input. The opt-in `examples/locked.kdl` is a test/example configuration, not a replacement for the user's global bindings. F1 opens scrollable help, including full command labels and configuration errors; F5 rebuilds the HOME index, reloads shared history, and resets the form to original cwd and Shell while retaining command configuration. Ctrl+Q closes only the plugin pane.

## 6. Configured commands

The user's Zellij plugin configuration is the only command-definition source.
No settings editor, preference file, config rewriting, or automatic agent defaults
are required. Layouts and plugin aliases can supply different lists. Launching the
WASM directly without command configuration provides Shell only.

Example settings inside the plugin configuration block (KDL uses spaces, not `=`):

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

Configuration contract:

1. Shell is always first and initially selected. It uses Zellij's configured
   default-shell API, not a configured executable or a hard-coded shell.
2. `commands` is a comma-separated, ordered list of stable command IDs. Missing
   or empty means Shell only. Trim whitespace around IDs and retain only the
   first occurrence of a duplicate. Ignore empty list segments.
3. For each listed ID, `command_<id>` is required and specifies one executable
   name or path, not a command line. `arguments_<id>` is optional and defaults to
   no arguments. `label_<id>` is optional and defaults to the ID. Unlisted command
   definitions do not appear in the selector.
4. The ID is independent of its label and executable. Different IDs may launch
   the same executable with different arguments. Reserve `shell` for the built-in
   entry; it cannot be overridden or duplicated by user configuration.
5. Parse `arguments_<id>` with shell-style quoting and escaping into an argument
   vector, preserving quoted empty arguments. Do not execute a shell or expand
   variables, tilde, globs, pipes, redirections, or command substitutions.
   For example, `arguments_claude "-w --model \"some model\""` in KDL passes
   three literal arguments: `-w`, `--model`, and `some model`.
6. Users needing shell interpretation must explicitly configure a shell executable
   and its arguments, such as `sh` with `-c`. Launchpad never introduces this
   wrapper implicitly. The selected directory is always the structured cwd.
7. Invalid definitions (including missing/empty executables or malformed argument
   quoting) produce a visible configuration error and exclude the affected entry;
   Shell and other valid entries remain usable. Do not probe executables or filter
   entries by installation status. Missing executables follow normal host rejection.
8. Preserve the parsed configuration through HOME remount/reload, F5, and form reset.
   A changed Zellij configuration is applied when opening a newly configured plugin
   instance; live config editing/reload watching is out of scope.

History stores the stable command ID, not executable data. Resolve replay against
that instance's current configured command definition. Label changes keep the
association; executable/argument changes under the same ID apply to future replay.
Removed or invalid command IDs remain visible as unavailable history entries:
replay must not silently substitute another command, but copying the directory
remains possible. History is disposable cache: ignore unsupported old schemas,
with no migration or backward compatibility. Never execute a command definition
read from history.

## 7. Launch lifecycle

States: booting → permission check → ready → validating → launching → handed off; failures return to ready with the form intact.

- Freeze duplicate submission while validating/launching.
- Use structured executable/arguments/cwd. Never synthesize `cd <path> && <command>` or inject typed text into another terminal.
- Launch into the dashboard's own pane identity, even if the user switches tabs during asynchronous work.
- Before replacement, name its originating stable tab ID `basename · configured label`, including history replay. Shell uses `Shell`; HOME uses its basename; root uses `/`. Read back the name before spawning. Never use a positional/current-tab rename. Known spawn rejection restores the previous name only if the current name still matches our write; the host offers no atomic compare-and-swap (see launch notes).
- Known host rejection leaves the dashboard usable with the form intact and requires explicit retry. Successful replacement closes the dashboard permanently; it is not suppressed for later restoration.
- Shell uses Zellij's default-shell lifecycle. Configured command panes close on process exit, including nonzero exits; Launchpad does not return. Preserve the explicit own-pane action and existing asynchronous rejection handling. Do not respawn commands or fall back into another program.
- History records validated launch attempts **before** replacement, because successful
  replacement normally destroys the plugin before acknowledgement. Known spawn
  rejection removes that exact attempt without overwriting concurrent updates;
  it does not restore entries already displaced by the ten-directory cap.
  Pre-validation failures do not create records; unknown outcomes may remain.
- Failure to save history must not kill or roll back a successfully launched tool; display a warning when feasible.

## 8. Recent history

The real plugin stores a version-2 JSON projection at `/cache/history.json`,
with up to ten entries: unique operation ID, UTC Unix timestamp in seconds,
absolute validated cwd, and a stable configured-command ID (or built-in Shell ID).
Only version-2 journal records are supported; IDs remain case-sensitive.
Unsupported old-format cache is ignored, not migrated. See [configured command details](docs/configured-commands.md)
for exact bounds, lexical parsing, UI overflow and pinned-host CLI limitations.
Replay uses the current trusted
launcher mapping, never executable data from history. Replay/copy-to-form uses the same directory policy: HOME validation plus the
exact invoking-cwd exception for that instance. An outside-HOME historical path
is not a general capability to launch it from another cwd; deleted paths fail.

`/cache/history.d/` contains immutable versioned operation records and the newest
clear watermark. These coordinate concurrent panes/sessions without WASI flock,
blocking sleeps, or expiring lock ownership. Records are atomically published;
readers merge the journal instead of trusting a potentially stale JSON projection.
Refresh/reopen repairs the projection. This is convergent history, not a
linearizable live feed. See [the algorithm and limits](docs/history.md).

Bounds: ten distinct directory records plus one clear watermark after quiescent
compaction; transient concurrent records are allowed; scans stop at 128 files,
each record is read through a 32 KiB limit, and paths are limited to 4096 UTF-8
bytes. Missing/corrupt/unknown-version records are ignored. Unsafe paths and
I/O/quota failures show errors when the dashboard remains visible; launches are
still attempted. Clear/delete update the UI only after persistence succeeds.

No prompts, credentials, environment dumps, conversation content, or terminal
output are recorded. Permissions are inherited from Zellij's host-managed cache.
Cache cleanup is allowed to remove history; it is not a durable backup.
On pinned 0.45.1, `/data` is per instance and `/cache` is shared by plugin URL.

## 9. Architecture and integration

Rust WASM plugin sharing application state, editing, matching, command definitions,
and rendering with a native simulation adapter. A persistent worker owns the HOME
index and ordinary directory validation. The main instance validates only the
exact invoking cwd via a remount acknowledgement and directory-open check.
There is no external runtime helper executable.

Pinned Zellij host/SDK: **0.45.1**. Actual launch APIs:

- Shell: `open_terminal_in_place_of_plugin`, using Zellij's configured default shell.
- Configured commands: `run_action(Action::NewInPlacePane)` with an explicit originating plugin pane ID, structured executable/argv/cwd, `close_replaced_pane: true`, and both command hold flags false.
- Correlate asynchronous action rejection with the pending launch. Successful replacement normally destroys the plugin before it receives completion.

The normal plugin requests `ReadSessionEnvironmentVariables`, `FullHdAccess`,
`ChangeApplicationState`, `OpenTerminalsOrPlugins`, `RunActionsAsUser`, and
`ReadApplicationState` (own-pane-to-stable-tab mapping and name readback).
Read only HOME from the session environment. On this host, changing the `/host`
mount requires FullHdAccess; a guarded self-reload makes the worker inherit HOME,
then a worker handshake verifies the mapping. ChangeApplicationState supports
that bootstrap, not tab renaming. These host permissions are broader than the
plugin's HOME-only search policy and own-pane launch actions.

Use Zellij's URL-shared `/cache` for disposable history and per-instance `/data`
for bootstrap state. History's immutable journal avoids reliance on unavailable
WASI advisory locking. No network access, pane-content reads, terminal-input
injection, or rewriting the user's Zellij configuration is required.

Installation is opt-in. `examples/launchpad.kdl` provides Shell only;
`examples/configured.kdl` demonstrates configured commands. Layouts and plugin
aliases carry settings; do not use the host's comma-splitting CLI configuration
argument for multi-command lists. New-tab keybinding/default-layout recipes remain
listed separately below. Do not claim `welcome-screen` is a new-tab hook or change
all of the user's new-pane behaviour.

## 10. Acceptance criteria

- A ready dashboard has editable path focus, initial input at the original invoking cwd, and Shell selected. HOME/permission/bootstrap failures remain visible rather than falling back to another directory.
- Shell appears first and uses Zellij's default shell rather than hard-coded bash. Other entries appear only from `commands`, in configured order with configured labels; absent/empty configuration gives Shell only.
- Tests cover arbitrary command IDs, multiple variants of one executable, optional fields, whitespace/deduplication, malformed definitions, and unavailable history IDs. Invalid entries show errors without disabling Shell or valid entries.
- Actual harmless fixture executables verify exact argument vectors (including quoted spaces and empty arguments), literal metacharacters without expansion, and selected cwd; tests do not run actual coding agents.
- Configured commands survive bootstrap remount/reload, F5, and reset. Dynamic selector keyboard/mouse behaviour and long labels/many entries remain usable at supported sizes.
- Unicode, spaces, apostrophes, semicolons, dollar signs, and shell-looking directory names are passed literally and cannot execute injected commands.
- Invalid/unreadable directories, stale completions, broken symlinks, missing executables, denied permissions, and failed launches preserve a usable form.
- Launch replaces only the dashboard pane, in the same tab, and never creates an accidental split or affects the currently focused unrelated tab.
- Enter-repeat/double-click cannot create duplicate launches.
- History shows up to ten distinct recently opened directories with the last tool, survives plugin/session reopening at the same URL/cache, and merges concurrent writes.
- Stale history entries cannot silently launch a different tool.
- Corrupt or unwritable history does not prevent ordinary launch.
- Tests cover 80×24, 40×12, and very small viewports; long paths and wide Unicode do not corrupt layout.
- Unit tests cover matching, input/focus state, validation, sorting/pruning, and schema handling. Live Zellij integration tests cover permissions, replacement, launch cwd/argv, default shell, cancellation, and restart persistence on the pinned supported version(s).

## 11. Deferred features and verification gaps

These are not implemented and are separate from the current acceptance criteria:

- A configurable starting-directory override (the invoking cwd is already the default).
- Configurable search roots. HOME-only search remains the current deliberate scope.
- General directory-symlink support, including cycle and escape policy. Only the exact invoking cwd is currently excepted; search never follows links.
- A documented new-tab keybinding and default-new-tab layout recipe that preserve existing tab/status bars and user layouts.
- macOS verification. No cross-platform test claim is made from Linux results.
- A reproducible current-build p95 input-to-render benchmark against the earlier aspirational 50 ms target. Existing performance measurements are not a hard latency guarantee.

An intermittent blank permission screen has occurred in live Zellij verification.
Component reruns or recorded viewport redraws have passed, but the startup cause
remains unresolved; they are not evidence of an uninterrupted full-suite pass.

## 12. Explicitly out of scope

Invoking-directory-relative path resolution is not planned: relative-looking input is served by fuzzy search and accepting the desired completion. Existing HOME-relative literal validation need not be removed, but is not a reason to add cwd-relative navigation.

Other exclusions: conversation resume, agent status monitoring, built-in worktree management, git-aware dashboards, remote host selection, automatic trust/permission flags, ad-hoc command entry in the launch form, a settings editor, and project environment auto-activation. Users may explicitly configure command arguments that request a tool's own worktree mode or other behaviour. zoxide ranking and pinned projects are ideas, not acceptance criteria.

## Sources

[1] https://github.com/zellij-org/awesome-zellij
[4] https://github.com/UrosNikolic/zellij-claude
[12] https://zellij.dev/documentation/plugin-api-commands
[13] https://zellij.dev/documentation/plugin-api-permissions.html
[15] https://zellij.dev/documentation/plugin-api-file-system.html
[18] https://github.com/dmtrKovalenko/fff/blob/main/crates/fff-core/Cargo.toml
[19] https://github.com/dmtrKovalenko/frizbee

# Zellij Launchpad — draft product specification

Status: draft target product specification, not all implemented. HOME-only search
and real own-pane launches plus shared persistent history are implemented.
Configurable commands are implemented from Zellij plugin configuration; they replace
the fixed launcher list and earlier installed-tool discovery proposal. No executable
availability checks are required. Native/demo adapters still simulate launches
and do not access persistent history. History's implemented contract supersedes
older event-log proposals below: see [persistence details](docs/history.md).
Current scope and deviations: [home-search notes](docs/home-search.md) and
[real launch behavior](docs/real-launch.md).
Working name, not checked for uniqueness.

## 1. Existing solutions and differentiation

Research found adjacent tools, but no verified exact match for a new-tab dashboard combining path autocomplete, installed-tool selection, and ten recent launches. This is a bounded finding, not proof that none exists.

The community catalogue describes **zellij-newtab-plus** as named-tab creation with zoxide navigation, **ghost** as a floating command-terminal launcher, and **zellij-ai-session** as finding and resuming coding sessions by project. These descriptions were read in the catalogue; their individual implementations were not verified.[1]

**zellij-claude** discovers running Claude sessions, filters them by directory/name/status, and supports switching across Zellij sessions. Its documented focus is managing existing sessions rather than this launch workflow.[4]

**Product distinction:** directory-first, multi-tool, disposable new-tab launcher. Not an agent monitor, conversation browser, or project/session manager.

## 2. Product contract

Opening a dashboard tab gives the user an immediately focused directory input, a configured-command selector, and recent launch history. Selecting a command launches it in the chosen directory and replaces the dashboard pane in the same tab.

Proposed v0.1 defaults:

- Platform: Linux and macOS; local host running Zellij.
- Exactly one launch per activation; no automatic agent resume.
- Initial directory: invoking terminal's working directory, falling back to configured starting directory, then home.
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
- Measure and truncate by terminal cells, not bytes. Escape control characters in filenames; preserve actual path bytes separately from display labels.

## 4. Directory input and autocomplete

- Accept absolute paths, `~`, `~/…`, `.`, `..`, and paths relative to the captured invoking directory.
- Expand only the leading home shortcut; no command substitution, glob expansion, or arbitrary shell expression evaluation. `~otheruser` and environment-variable expansion are out of scope for v0.1.
- Complete directories only, including symlinks resolving to directories.
- Reuse an established embedded fuzzy matcher; do not implement a custom fuzzy matching/ranking algorithm. Selected matcher: Frizbee (`neo_frizbee`), approved by the user, embedded directly in the WASM plugin. Fuzzy-match directory names and relative paths using the library's scoring, with deterministic tie-breaking—not just prefixes of the currently typed path component. A bare query such as `notes` must find `~/Projects/notes/` within the configured search roots without requiring the user to type `~/Projects/`. Abbreviated queries can match non-contiguous characters. Show enough of each matching path to distinguish directories with the same name. Accepting a result fills the actual directory path; never launch using the unresolved query text. Explicit absolute or relative paths remain supported.
- Deliver a single WASM plugin: no external search executable or native companion. Directory enumeration, path expansion, and the input/suggestion UI still need thin Zellij-specific integration; a matcher library does not supply those pieces.
- A standalone probe using `neo_frizbee` 0.13.1 with default features disabled and `safe_read` enabled passed native fixture checks and compiled for `wasm32-wasip1`. Live Zellij execution remains unverified. The full fff engine is not selected: its current core has unconditional native-oriented dependencies including notify, heed/LMDB, memmap2, git2, and rayon.[18][19]
- Hidden directories are excluded unless explicitly requested by a dot-prefixed path component.
- Discover directory candidates beneath configured search roots (including `~/Projects` for this design), not only immediate children of the current directory. Use bounded, incremental enumeration with cached candidates; never scan the entire disk on each keystroke. Root configuration, traversal limits, exclusions, and symlink-cycle handling must be specified before implementation.
- Spaces and quotes are literal filename characters, not shell syntax. Do not automatically add shell quoting to the input.
- List directory entries asynchronously, debounce input, and discard stale responses using request generations.
- Validate immediately before launch: directory exists, is a directory, and can be entered. Broken symlinks and permission failures get actionable inline messages.
- Do not create missing directories automatically.
- Preserve the entered absolute/logical path for display and relaunch. A canonical path may be stored as metadata but must not silently replace the user's symlink spelling.

Proposed performance targets: no filesystem I/O in the render path; p95 input-to-render under 50 ms on a declared local test machine; cap visible completions and bound background work. Slow or unreachable mounts must not block typing.

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

History unavailable: show an empty state; history shortcuts are harmless. If an entry's tool was removed, show it as unavailable and let the user copy its directory into the form, but never silently substitute another tool.

Key delivery must be tested against Zellij normal and locked modes. Installation documents conflicts and a compatible binding set rather than globally stealing terminal input. Global shortcuts remain subject to revision after the compatibility spike.

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
- Keep the dashboard recoverable until launch acceptance is known; prove exact suppression/close ordering in the API spike.
- On accepted launch, optionally rename the tab to `<directory basename> · <tool>`. Do not rename other tabs.
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
launcher mapping, never executable data from history. All replay/copy-to-form
launches use ordinary directory validation; deleted/out-of-HOME paths cannot launch.

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

Proposed implementation: Rust WASM plugin, with pure modules for path editing, matching, focus/state transitions, target descriptors, history schema, and rendering. Keep host access behind an adapter for testability.

Zellij documents `open_terminal_in_place_of_plugin` and `open_command_pane_in_place_of_plugin`, including a flag to close versus suppress the replaced plugin. Command launches support a separate cwd and argument vector.[12]

Use a dedicated dashboard layout and an explicit new-tab keybinding as the initial integration. Offer a default-new-tab layout recipe after verifying layout composition. Do not claim the `welcome-screen` alias is a new-tab hook, and do not change every user's new-pane behaviour. Installation is opt-in and must preserve existing tab/status bars and existing layouts.

Permissions should be mapped to the tested implementation: likely `ReadApplicationState`, `OpenTerminalsOrPlugins`, `RunCommands`, `ChangeApplicationState` for renaming, and `FullHdAccess` for arbitrary directory browsing/state storage. Current documentation also lists `ReadSessionEnvironmentVariables`; use it only if needed and supported by the pinned baseline.[13]

No network access, stdin injection, pane-content reads, or config-rewrite permission is needed for the intended product. A denied permission must produce a clear restricted mode or explanation, not a retry loop.

### Required compatibility spike before implementation

The live documentation is not proof of support in the user's installed release. Pin the Rust SDK and minimum Zellij release only after checking:

1. Capturing invoking cwd and the host PATH/home/XDG environment reliably.
2. Filesystem mapping outside `/host`; safe access to durable state without confusing host paths and WASI paths.
3. Configured executable/argument forwarding in the environment inherited by command panes, without availability probing or implicit shell interpretation.
4. Own-pane replacement, acknowledgement events, error recovery, and history write ordering for both Shell and agents.
5. Cross-instance/cross-session file locking from WASI. A single WASM artifact is a product constraint. If reliable host discovery or durable locking cannot be implemented through Zellij/WASI, report the limitation and revisit the affected requirement; do not introduce an external binary.
6. Key delivery, default-layout integration, focus races, plugin cleanup, and narrow-terminal rendering.

## 10. Acceptance criteria

- A new dashboard opens with editable path focus and the expected invoking cwd/fallback.
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

## 11. Explicitly out of scope

Conversation resume, agent status monitoring, built-in worktree management, git-aware dashboards, remote host selection, automatic trust/permission flags, ad-hoc command entry in the launch form, a settings editor, and project environment auto-activation. Users may explicitly configure command arguments that request a tool's own worktree mode or other behaviour. Optional future additions: zoxide ranking and pinned projects.

## Sources

[1] https://github.com/zellij-org/awesome-zellij
[4] https://github.com/UrosNikolic/zellij-claude
[12] https://zellij.dev/documentation/plugin-api-commands
[13] https://zellij.dev/documentation/plugin-api-permissions.html
[15] https://zellij.dev/documentation/plugin-api-file-system.html
[18] https://github.com/dmtrKovalenko/fff/blob/main/crates/fff-core/Cargo.toml
[19] https://github.com/dmtrKovalenko/frizbee

# Zellij Launchpad — draft product specification

Status: proposed v0.1. HOME-only search is implemented; launches remain simulated.
Current scope and deviations: [home-search notes](docs/home-search.md).
Working name, not checked for uniqueness.

## 1. Existing solutions and differentiation

Research found adjacent tools, but no verified exact match for a new-tab dashboard combining path autocomplete, installed-tool selection, and ten recent launches. This is a bounded finding, not proof that none exists.

The community catalogue describes **zellij-newtab-plus** as named-tab creation with zoxide navigation, **ghost** as a floating command-terminal launcher, and **zellij-ai-session** as finding and resuming coding sessions by project. These descriptions were read in the catalogue; their individual implementations were not verified.[1]

**zellij-claude** discovers running Claude sessions, filters them by directory/name/status, and supports switching across Zellij sessions. Its documented focus is managing existing sessions rather than this launch workflow.[4]

**Product distinction:** directory-first, multi-tool, disposable new-tab launcher. Not an agent monitor, conversation browser, or project/session manager.

## 2. Product contract

Opening a dashboard tab gives the user an immediately focused directory input, an installed-tool selector, and recent launch history. Selecting a tool launches it in the chosen directory and replaces the dashboard pane in the same tab.

Proposed v0.1 defaults:

- Platform: Linux and macOS; local host running Zellij.
- Exactly one launch per activation; no automatic agent resume.
- Initial directory: invoking terminal's working directory, falling back to configured starting directory, then home.
- Initial tool: default shell. Do not silently start a remembered agent.
- Tool order: Shell, Claude, Codex, Copilot, Hermes.
- Recent list: ten most recent launch events, newest first; repeated path/tool combinations remain separate entries.
- History persists across new tabs, Zellij sessions, and host restarts for the same OS user.
- No required fzf, zoxide, Python, or shell framework.

Interpretation of “exist on the path”: executable command on the launch environment's `PATH`, not a file inside the chosen directory. The selected directory is the working directory. A project-local executable counts only if normal PATH resolution includes it.

## 3. Screen

Illustrative only; installed tools and history vary:

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

The input is the primary visual element. Suggestions occupy a capped region immediately below it; tools stay below that region and history below tools. Hide unavailable tools rather than rendering disabled choices. Show “Checking installed tools…” during discovery, not a misleading empty result.

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

## 6. Launch targets and discovery

| Label | Resolution | Launch |
|---|---|---|
| Shell | Zellij's configured default shell | Native terminal-opening API at chosen cwd |
| Claude | `claude` | Resolved executable, no default arguments |
| Codex | `codex` | Resolved executable, no default arguments |
| Copilot | `copilot` | Resolved executable, no default arguments |
| Hermes | `hermes` | Resolved executable, no default arguments |

Bare command names are the proposed launcher defaults. Confirm their interactive entrypoints during implementation; overrides use a command path plus an argument array, not an arbitrary shell string. Do not treat legacy `gh copilot` as equivalent to standalone `copilot`.

Discovery rules:

1. Use the same host environment context as launch; do not assume the WASM environment is the host environment.
2. Resolve each fixed executable name according to PATH order, including execute permission checks; resolve relative/empty PATH components against the selected directory.
3. Record the absolute resolved executable; recheck on submit to catch uninstall/replacement and avoid discovery/launch disagreement.
4. Do not launch tools to detect them: no `--version`, login checks, network calls, or project scripts.
5. Refresh on opening the dashboard and via an explicit refresh action. Invalidate cwd-sensitive results after directory changes.
6. Shell aliases and functions are not executable targets. Shell-initialization changes, direnv, and version-manager activation are not automatically sourced in v0.1. Explain this when discovery differs from an interactive shell; support trusted explicit executable overrides.
7. If a selected tool disappears, reset to Shell with a visible notice, never a silent fallback on submit.

## 7. Launch lifecycle

States: booting → permission check → ready → validating → launching → handed off; failures return to ready with the form intact.

- Freeze duplicate submission while validating/launching.
- Use structured executable/arguments/cwd. Never synthesize `cd <path> && <command>` or inject typed text into another terminal.
- Launch into the dashboard's own pane identity, even if the user switches tabs during asynchronous work.
- Keep the dashboard recoverable until launch acceptance is known; prove exact suppression/close ordering in the API spike.
- On accepted launch, optionally rename the tab to `<directory basename> · <tool>`. Do not rename other tabs.
- Shell exits normally; agent commands retain native Zellij exit/error/re-run behaviour. Do not automatically respawn agents or fall back into another program.
- A launch record means the terminal/command pane was accepted/created, not that an agent authenticated or completed a task. A later nonzero exit remains a launch event. Pre-validation failures and rejected spawn requests do not count.
- Failure to save history must not kill or roll back a successfully launched tool; display a warning when feasible.

## 8. Recent history

Store a versioned document with ten events, each containing:

- unique event ID;
- timestamp in UTC;
- absolute logical cwd;
- stable launcher ID;
- resolved executable and argument array used (Shell can omit executable metadata).

Use the current trusted launcher definition on replay, not arbitrary executable data read from history. Stored executable metadata is informational. Revalidate every replay and visibly flag unavailable tool/directory combinations.

No prompts, credentials, environment dumps, conversation content, or terminal output are recorded.

Proposed location: `${XDG_STATE_HOME:-~/.local/state}/zellij-launchpad/history.json`. Use private user permissions, a schema version, locked read-modify-write, and atomic replacement. Simultaneous launches in different tabs/sessions must not overwrite one another. Timestamp ties use a deterministic secondary ordering. Reject malformed records, preserve corrupt files for diagnosis, and recover to a usable empty history with a warning. Provide delete-selected and clear-history actions, with confirmation for clearing everything.

Zellij documents `/data` as shared plugin storage that is deleted on unload, so it is not the durable history store.[15]

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
3. Discovery matching the environment inherited by command panes.
4. Own-pane replacement, acknowledgement events, error recovery, and history write ordering for both Shell and agents.
5. Cross-instance/cross-session file locking from WASI. A single WASM artifact is a product constraint. If reliable host discovery or durable locking cannot be implemented through Zellij/WASI, report the limitation and revisit the affected requirement; do not introduce an external binary.
6. Key delivery, default-layout integration, focus races, plugin cleanup, and narrow-terminal rendering.

## 10. Acceptance criteria

- A new dashboard opens with editable path focus and the expected invoking cwd/fallback.
- Only executable installed agents appear, in fixed order; Shell uses Zellij's default shell rather than hard-coded bash.
- All five launch targets are covered by tests; deterministic fixtures stand in for tools not installed on a test host, without claiming real-agent verification.
- Unicode, spaces, apostrophes, semicolons, dollar signs, and shell-looking directory names are passed literally and cannot execute injected commands.
- Invalid/unreadable directories, stale completions, broken symlinks, missing executables, denied permissions, and failed launches preserve a usable form.
- Launch replaces only the dashboard pane, in the same tab, and never creates an accidental split or affects the currently focused unrelated tab.
- Enter-repeat/double-click cannot create duplicate launches.
- History shows exactly the latest ten accepted launch events, retaining duplicates, surviving restart, and respecting concurrent writes.
- Stale history entries cannot silently launch a different tool.
- Corrupt or unwritable history does not prevent ordinary launch.
- Tests cover 80×24, 40×12, and very small viewports; long paths and wide Unicode do not corrupt layout.
- Unit tests cover matching, input/focus state, validation, sorting/pruning, and schema handling. Live Zellij integration tests cover permissions, replacement, launch cwd/argv, default shell, cancellation, and restart persistence on the pinned supported version(s).

## 11. Explicitly out of scope

Conversation resume, agent status monitoring, worktree creation, git-aware dashboards, remote host selection, per-project scripts, automatic trust/permission flags, arbitrary command entry, and project environment auto-activation. Optional future additions: zoxide ranking, pinned projects, custom launchers, and an alternative deduplicated recent-target view.

## Sources

[1] https://github.com/zellij-org/awesome-zellij
[4] https://github.com/UrosNikolic/zellij-claude
[12] https://zellij.dev/documentation/plugin-api-commands
[13] https://zellij.dev/documentation/plugin-api-permissions.html
[15] https://zellij.dev/documentation/plugin-api-file-system.html
[18] https://github.com/dmtrKovalenko/fff/blob/main/crates/fff-core/Cargo.toml
[19] https://github.com/dmtrKovalenko/frizbee

# Plugin verification — Zellij 0.45.1

## Result

The fixture-only Launchpad runs as a **single WASI WASM plugin** in the installed
Zellij host. Rendering, editor/state, fuzzy matching, hit maps and fixtures are
shared with the native prototype. No real tool process is launched. Quit uses
`close_self()` and was verified not to close an ordinary companion pane.

- Artifact: `target/wasm32-wasip1/release/launchpad-plugin.wasm`
- Size: **1,840,520 bytes**
- SHA-256: `3a6d9b08f6e1e91c94637d1798bd04028b42a6918599ddd914cbc1606e1240a7`
- Host/SDK: **Zellij 0.45.1 / zellij-tile 0.45.1**
- Rust: **1.98.1**; target **wasm32-wasip1**
- Ratatui **0.30.2**, neo_frizbee **0.13.1**; both pinned in Cargo.lock.
- Permissions requested: **none**.

## Exact build and launch commands

From Bash, outside an existing Zellij session:

```bash
cd /home/piclaw/Projects/zellij-launchpad
rustup target add wasm32-wasip1 --toolchain 1.98.1
cargo build --locked --release -p launchpad-plugin --target wasm32-wasip1
zellij --config examples/locked.kdl \
  --layout-string "$(<examples/launchpad.kdl)"
```

The example's absolute `file:` URL must be updated when moving the artifact.
The plugin itself has no asset/helper dependency. The example Shell tab is a
normal host pane, not a native companion. Short fixed tab titles avoid stock
Zellij title wrapping. No global config was edited.

In an existing locked-mode session, this second loading path was also tested:

```sh
zellij action launch-plugin \
  file:/home/piclaw/Projects/zellij-launchpad/target/wasm32-wasip1/release/launchpad-plugin.wasm
```

In the supplied config, Ctrl+G unlocks, Alt+Left/Right changes tabs, Ctrl+G locks
again, and **Ctrl+G then Ctrl+Q exits the host**. Ctrl+Q while locked closes only
Launchpad. Esc's closed-pane screen remains simulated, matching the prototype.

## Complete rerun

```sh
cd /home/piclaw/Projects/zellij-launchpad
python3 -m venv target/verification-venv
target/verification-venv/bin/pip install pyte==0.8.2 playwright==1.58.0
# /usr/bin/chromium was used here; Chromium is a development-only dependency.
bash tools/verify_plugin.sh
```

The **same complete runner was executed successfully**, exit code 0.
[Full output](complete-run.log). It includes:

| Check | Actual result |
|---|---|
| `cargo fmt --all --check` | Passed |
| `cargo test --workspace --locked` | **37 passed**, 1 PTY-only probe ignored here |
| Native workspace clippy, all targets, `-D warnings` | Passed |
| Plugin WASI clippy, `-D warnings` | Passed |
| Native release build | Passed |
| WASI release build | Passed |
| WASI dependency audit | **No Crossterm** |
| Existing native real-PTY driver | **51 checks passed** |
| Existing native lifecycle driver | **3 checks passed**, including ignored probe in real PTYs |
| Live Zellij driver, first fresh session | **45 checks passed** |
| Live Zellij driver, second fresh session | **45 checks passed** |
| Actual PTY replay screenshots | 6 generated using xterm.js 6.0.0 / Chromium |

The 37 Rust checks comprise the original 28 plus ANSI serialization, SDK event,
render/resize/quarantine and exact-SDK KDL parsing coverage. New behavior was
exercised in RED/GREEN slices: wide continuation serialization; style/space
preservation; SDK shortcuts; shared render/caret/hits; click/hold/release/paste;
wheel position invalidation; host scalar Unicode compatibility; stale mouse
quarantine. These are not only cross-compilation checks.

### Live coverage

[First run](live-1.log) · [second run](live-2.log) · [machine-readable report](live-report.json)

The attached real PTY exercises notes/nts Frizbee matching, completion without
launching, tool selection, simulated launch/back, history copy/replay, repeated
Enter suppression in the placeholder, F1 help, invalid path, reset, availability,
actual SGR suggestion/tool/history/launch/copy/replay/back/quit clicks, a genuine
press–drag–release sequence, bracketed Unicode paste, CJK-continuation cursor
placement, 80×24 and 120×36 alignment, section-local wheels, scrolled-help Back,
40×10 layout, 30×8 launch guard, normal-mode host interception and locked-mode
recovery. Both footer Quit and Ctrl+Q were exercised.

The host was read back after each pane-close operation:

- [Before footer Quit](panes-before.json): exact WASM URL and an ordinary shell.
- [After footer Quit](panes-after.json): Launchpad absent, same ordinary shell ID.
- [After Ctrl+Q in a same-tab split](panes-after-key-quit.json): second Launchpad
  absent, same ordinary shell ID still present.

The shell remained interactive and printed `SURVIVOR_OK`. The session then exited
normally via the documented host keys, with original PTY termios restored.

### Visual evidence

These screenshots come from replaying **unmodified attached-Zellij PTY output**
with its recorded resize sequence, not from drawing the buffer strings or HTML
prototype. Initial, Unicode, wide, narrow, guard and split scenes were visually
inspected in the actual xterm.js emulator.

- [Initial 80×24](01-initial-80x24.png)
- [Unicode and synthetic caret at 80×24](06-unicode-80x24.png)
- [Wide 120×36](07-wide-120x36.png)
- [Narrow 40×10 dashboard](09-narrow-40x10.png)
- [30×8 safety guard](10-guard-30x8.png)
- [Plugin alongside the ordinary shell before Ctrl+Q](13-split-before-key-quit.png)

Full raw ANSI, JSON replay records and host logs are retained in ignored paths:

- `target/zj-3a27701d/` — first final run
- `target/zj-7f91056f/` — second final run and screenshots
- `target/plugin-verification/` — all command logs

Each run creates its own config, HOME, cache, data, runtime, socket and log paths
under `target/zj-*`; cleanup targets only the exact unique session in that private
socket directory. There are no global kill/delete-session commands. Native
historical evidence and the unrelated `spikes/` directory were preserved.

## Caveats and investigations

See [SDK findings and sources](sdk-notes.md). Important limitations are explicit:

1. **Host keys:** locked mode is required for conflicting shortcuts. SDK events
   expose no key repeat kind and no mouse modifiers; native filtering cannot be
   replicated perfectly. The simulated-terminal state prevents duplicate launch
   history after a launch, but held Enter can accept then submit a completion.
2. **Wheel coordinates:** SDK wheel events have none. The adapter uses the last
   delivered pointer position; it cannot invent missing motion events. No known
   position means no scroll. Resize resets it and quarantines queued mice.
3. **Unicode:** Zellij's scalar grid drops combining codepoints. Display-only NFC
   restores composable accents; incompatible multi-scalar emoji use a padded
   replacement instead of corrupting adjacent cells. Logical text is unchanged.
4. **Startup layout:** file-based `--layout`/`-n` startup fell back to the stock
   layout on this installed host despite valid KDL. `--layout-string` and the
   existing-session launch-plugin route were exercised successfully. No host
   workaround binary or global configuration change was introduced.
5. **Scope:** no real directory discovery, persistence, network, agent execution
   or launch-driven pane replacement is implemented. SPEC.md still describes
   the future product; it and the approved design HTML were not changed.

During harness development, pyte's unsupported terminal probes needed filtering
for text assertions, and a Back click initially matched help prose rather than
the footer. The final driver preserves raw bytes, locates footer controls and
asserts the distinctive destination screen. Final passing runs include those
stronger checks; earlier intermediate output is not counted as acceptance.

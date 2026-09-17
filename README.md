# Launchpad — Ratatui prototype and Zellij plugin

A playable Ratatui interpretation of [SPEC.md](SPEC.md) and
[the approved HTML study](design/launchpad.html). The path-first layout, olive/charcoal
palette and pale green focus treatment carry over; browser chrome does not.

**Launches, directories, availability and history are still fixtures.**
There is no host directory enumeration, PATH discovery, account access, command
execution, networking or persistence in either adapter. All paths and history
are embedded under `/home/demo`. The native executable owns its terminal; the
single-file WASM plugin receives Zellij events and renders into its own pane.
Plugin Quit really closes that pane; launching a tool remains a simulation.

## Run the Zellij plugin

Tested in **Zellij 0.45.1**, with **zellij-tile 0.45.1**, Rust **1.98.1**,
Ratatui **0.30.2** and embedded **neo_frizbee 0.13.1**. No native companion,
external matcher, asset bundle or runtime helper is required.

```sh
cd /home/piclaw/Projects/zellij-launchpad
rustup target add wasm32-wasip1 --toolchain 1.98.1
cargo build --locked --release -p launchpad-plugin --target wasm32-wasip1
```

Artifact: **`target/wasm32-wasip1/release/launchpad-plugin.wasm`**.
Copy that one file to distribute the plugin; update the absolute `file:` URL in
[examples/launchpad.kdl](examples/launchpad.kdl) if you move it.

From **Bash**, outside an existing Zellij session:

```bash
cd /home/piclaw/Projects/zellij-launchpad
zellij --config examples/locked.kdl \
  --layout-string "$(<examples/launchpad.kdl)"
```

The example creates a full-size **Launch** tab and an ordinary **Shell** tab.
It uses its own opt-in config, not your normal Zellij configuration. The shell
is a companion for testing pane survival, not a helper used by the plugin.
Tab titles are intentionally short and deterministic. This installed host
silently fell back to its stock layout with `--layout <file>` and `-n <file>` in
isolated tests; `--layout-string` is the **live-verified loading recipe**, using
the same valid KDL. The SDK parses the sample in a regression test.

In an **existing** Zellij session, load only the artifact with:

```sh
zellij action launch-plugin \
  file:/home/piclaw/Projects/zellij-launchpad/target/wasm32-wasip1/release/launchpad-plugin.wasm
```

That CLI route was also live-tested, opening a split beside a surviving shell.
Use your session's locked-mode binding first (normally Ctrl+G) for key delivery.

### Host keys, permissions and differences

- The example starts **locked**: Ctrl+P/T/R and Ctrl+Q reach Launchpad.
  **Ctrl+G** unlocks; **Ctrl+G then Ctrl+Q** exits Zellij itself. In unlocked
  normal mode, **Alt+Right/Left** changes tabs; Ctrl+G locks again. No global
  config is changed, and the plugin does not intercept host input.
- **No permissions are requested.** Key, Mouse, PastedText, subscription and
  `close_self()` require none on 0.45.1. No RunCommands, FullHdAccess,
  ReadApplicationState or ChangeApplicationState permissions are needed.
- **Quit (Ctrl+Q, Ctrl+C or the footer) closes only this plugin pane.** Another
  ordinary pane survives, including in the same tab. Esc's untouched-pane
  closure remains the prototype's **simulated closed screen**; Esc can reopen it.
- Zellij `KeyWithModifier` has no press/repeat/release field. The adapter cannot
  distinguish held Enter from successive presses. Once the simulated terminal
  is open, repeated Enter cannot add launch events, but a held Enter can accept
  a completion and then submit it. Native Crossterm repeat filtering is unchanged.
- The SDK sends LeftClick / Hold / Release separately. Only LeftClick activates;
  Hold, Release and right-click do not. **Mouse modifiers are not exposed**:
  modified-click routing is the host's responsibility (Shift normally selects
  terminal text). An orphan drag report may be turned into a click by the host.
- SDK wheel events contain a line count, **not coordinates**. Section-local
  scrolling uses the last delivered hover/click position; without one it does
  nothing. Terminals must forward motion for reliable hover-local scrolling.
  Resize clears that position and briefly quarantines queued mouse events.
- A nonblinking **styled caret** uses the shared renderer's cursor position;
  the plugin never takes control of the host hardware cursor or terminal modes.
- Zellij 0.45.1 drops zero-width Unicode scalars. Display-only NFC composition
  preserves accents such as `e + ◌́`; logical editor bytes and hit maps stay
  unchanged. Emoji clusters whose host scalar width disagrees with Ratatui are
  displayed as `�` plus padding rather than shifting adjacent cells. Other
  noncomposable combining marks remain subject to the host's rendering limit.

See [SDK/source findings](verification/plugin/sdk-notes.md) and
[live verification](verification/plugin/REPORT.md).

## Run the native prototype

The built executable on this machine is a single **Linux aarch64** binary:

```sh
/home/piclaw/Projects/zellij-launchpad/target/release/zellij-launchpad-prototype
```

It can be launched from any working directory; no assets or helper programs are
needed. It uses the platform's normal C runtime/loader (not a static musl build).
Use a UTF-8 terminal, preferably with true-color support. **80×24** shows all ten
initial events; **120×36** adds spacing. Narrow layouts wrap tools and scroll
history. Below **40×10**, a resize notice blocks interaction except quit/reset.

Build it yourself:

```sh
cd /home/piclaw/Projects/zellij-launchpad
cargo build --release --locked
./target/release/zellij-launchpad-prototype
```

Rustup reads the project-local `rust-toolchain.toml`; this does not change the
global default toolchain. A fresh build needs network access to download the
compiler/crates. Running the compiled binary does not.

### Try the main flow

1. **Ctrl+U**, type `notes` (or the noncontiguous abbreviation `nts`).
2. **Down**, then **Enter**: accepts the highlighted completion, **does not launch**.
3. **Ctrl+T**, then **Left/Right**: choose a tool.
4. **Enter**: opens a clearly labelled simulated terminal and records one event.
5. **Esc**: return. **Ctrl+R** selects recent launches; **Enter** replays,
   **Tab** copies the selected path and tool back into the form without launching.
6. **Ctrl+Q** or **Ctrl+C** quits. Printable `q` remains ordinary input.

## Keys

| Area | Keys | Action |
|---|---|---|
| Anywhere | Ctrl+P / Ctrl+T / Ctrl+R | Path / tools / history focus |
| Path | Tab / Shift+Tab | Accept and cycle original matching results; never launch |
| Path | Up / Down | Highlight suggestions; Down without suggestions focuses tools |
| Path | Enter | Accept highlight only; otherwise validate the literal fixture path and simulate launch |
| Path | Left / Right, Home / End | Move by extended grapheme cluster |
| Path | Ctrl+A / Ctrl+E / Ctrl+U | Home / end / clear the entire input |
| Path | Backspace / Delete | Remove one complete grapheme |
| Tools | Left / Right | Select visible tools in fixed Shell, Claude, Codex, Copilot, Hermes order |
| Tools | Up / Down / Enter | Path / history / simulate launch |
| History | Up / Down, Home / End | Select event; scroll follows selection |
| History | Enter / Tab | Revalidate and replay / copy to form |
| History | Delete / Ctrl+L | Remove selected / clear all with a second Ctrl+L confirmation |
| Anywhere | F1 | Scrollable keyboard help; Up/Down and Home/End navigate |
| Demo | F5 / F6 | Reset all fixtures / toggle Copilot availability |
| Anywhere | Esc | Dismiss help, suggestions or message first; close an untouched dashboard as a simulated pane |
| Terminal / closed pane | Esc | Return to the dashboard; Shell becomes the default |
| Anywhere | Ctrl+Q / Ctrl+C | Quit native TUI / close own plugin pane |

Bracketed paste inserts text literally (control characters are removed). Repeated
Enter in the terminal placeholder cannot record additional events. No command is
ever executed.

## Mouse

- Left-click the path to focus and place its cursor, including horizontally scrolled
  text. Wide characters and combining sequences stay whole.
- Click a suggestion to copy it into the form, a tool to select it, or a recent row
  to select it. **None of these clicks launches.**
- Click **Enter ↵** to simulate a form launch. With history focused, **Tab copy**
  copies the selected path/tool and **Enter replay** explicitly relaunches it.
- Wheel over suggestions, history or help to navigate that section. Lists stop at
  their ends; wheel events elsewhere do not change focus.
- Click footer **F1 help**, **F5 reset**, **Ctrl+Q quit**, or **Esc back**. Narrow
  footers abbreviate the global controls to **F1**, **F5**, **^Q**.

In the **native adapter**, only unmodified left-button presses activate controls; release, drag, motion,
middle/right clicks and horizontal wheels do not. Mouse capture is enabled while
running and disabled on quit, returned errors and unwinding panics. The terminal
or multiplexer must forward mouse events (SGR supported); use its selection
modifier, often Shift, to select terminal text instead. Mouse input briefly waits
for a quiet input queue after resize so queued old coordinates cannot launch.
Uncatchable termination such as SIGKILL cannot run terminal cleanup.

## Fixtures and errors

- Initial logical working directory: `~/Projects/`; initial tool: **Shell**.
- `~/Projects/notes`, `~/Projects/research/notes`, `~/Projects/service/api`:
  bare-name and nested-path matching with enough path shown to disambiguate.
- `~/Projects/team notes`, `~/Projects/café`, `~/Projects/修理`,
  `~/Projects/it's literal; $HOME`: spaces, Unicode and shell-looking text are literal.
- `~/Projects/current`: a simulated directory symlink; its logical spelling is kept.
- `~/Projects/.archive`: hidden unless a dot-prefixed component is requested.
- `~/restricted`, `~/broken-link`, `~/Projects/missing`: actionable fixture errors.
- Absolute paths, `~`, `~/…`, `.`, `..` and relative paths resolve lexically against
  the fixture home/cwd. No shell expansion or shell quoting is performed.
- **F6** hides Copilot. Its history entries are marked `Copilot!` / `unavailable`.
  Replaying or copying an unavailable target never silently substitutes another
  tool. Choose an available tool explicitly with Ctrl+T and the arrow keys.
- History always keeps at most ten accepted events, newest first, including
  duplicates. All initial ages are fabricated labels; new events say `Just now`.
  History resets on restart or F5. There is no persistence.

## Pinned versions

Verified against upstream metadata on **2026-09-17**, not inferred from the
previously installed compiler:

| Component | Version | Verification source |
|---|---|---|
| Rust stable | **1.98.1** (`48a229cea`, channel date 2026-09-03) | [Official stable channel metadata](https://static.rust-lang.org/dist/channel-rust-stable.toml) |
| Ratatui | **0.30.2**, highest published non-yanked, non-prerelease version | [crates.io API](https://crates.io/api/v1/crates/ratatui) · [version documentation](https://docs.rs/ratatui/0.30.2/ratatui/) |
| Embedded Frizbee | **neo_frizbee 0.13.1**, `safe_read`, default features disabled | [crate](https://crates.io/crates/neo_frizbee/0.13.1) |
| Native terminal adapter | **Crossterm 0.29.0** | `Cargo.lock` |
| Zellij host / SDK | **0.45.1 / zellij-tile 0.45.1** | Live host and pinned release source; see plugin report |

`Cargo.lock` also resolves `unicode-segmentation 1.13.3` and `unicode-width 0.2.2`.
The latter matches Ratatui's cell-width dependency. Frizbee scores both basenames
and full paths, with lexical path tie-breaking. `Matcher::new` is used rather than
query-syntax parsing, so quotes and dollar signs do not become operators.

## Code boundaries

- `src/app.rs`: pure in-memory state, focus transitions, validation and launch events.
- `src/editor.rs`: grapheme-aware editing; byte offsets never serve as cell widths.
- `src/fixtures.rs`: all hard-coded directories/history and embedded Frizbee search.
- `src/view.rs`, `src/theme.rs`, `src/cells.rs`: reusable Ratatui rendering, palette,
  terminal-cell clipping and input scrolling. Rendering emits domain-action hit
  regions from the same geometry, not a second layout. No Crossterm or OS access.
- `src/terminal.rs`: native key/mouse decoding, resize handling, raw mode and alternate
  screen lifecycle. RAII plus a panic hook restore terminal state; non-TTY startup
  is rejected before emitting escape sequences.
- `src/help.rs`: shared static help copy; `src/main.rs`: error/exit reporting.

- `src/ansi.rs`: full-buffer plugin serializer, continuation-cell skipping,
  true-color/styles, row positioning, display-only Unicode host compatibility.
- `plugin/src/lib.rs`: SDK-to-domain events, shared Ratatui in-memory rendering,
  synthetic caret and resize mouse quarantine. No duplicate UI/layout code.
- `plugin/src/main.rs`: SDK registration, minimal subscriptions, stdout and
  close-self glue. Its Rust 2021 edition accommodates the SDK's export macro.

The workspace root defaults to the unchanged native binary. The plugin depends
on the root library with **default features disabled**; the WASI dependency tree
contains **no Crossterm**. `Cargo.lock` pins both builds. Async discovery,
persistence and real agent/shell launch or pane replacement from SPEC.md remain
out of scope. SPEC.md and the approved HTML are unchanged.

## Plugin and native verification

```sh
python3 -m venv target/verification-venv
target/verification-venv/bin/pip install pyte==0.8.2 playwright==1.58.0
# Chromium must be installed for the screenshot replay step.
bash tools/verify_plugin.sh
```

The complete runner checks fmt, **37 passing Rust tests** (the original 28 plus
9 new checks), native/WASM clippy with warnings denied, both release builds,
absence of Crossterm from WASI, the original **51 native PTY checks**, all
**3 lifecycle probes**, and **45 live Zellij checks twice**. The ordinary Rust
suite leaves the one PTY-only probe ignored; the lifecycle script exercises it
for returned errors and unwinding panics.

Every Zellij run gets a unique session plus HOME/config/cache/data/socket/log
paths beneath `target/zj-*`. Only that exact session is cleaned up. Reports
include the loaded WASM SHA-256, host pane readbacks, raw ANSI, resize/write replay
records and decoded screens. The runner also replays actual PTY bytes in pinned
xterm.js 6.0.0 with Chromium; the browser is an **evidence viewer**, not the UI.
Python, pyte, Playwright and xterm.js are development-only dependencies.

[Current report and screenshots](verification/plugin/REPORT.md).
The evidence below is **historical native-prototype verification**.

## Historical native verification

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

**27 tests passed**, including Ratatui TestBackend checks at 80×24, 120×36,
40×12, 40×10 and tiny/zero-sized buffers; Unicode cell coordinates and grapheme
joining; completion/launch separation; focus/navigation; literal paths; pruning;
availability errors; help scrolling; compact guards; non-TTY rejection; mouse hit
regions, blank/stale clicks, Unicode/scrolled cursor placement, wrapped tools,
scrolled lists, wheel boundaries and explicit launch/copy/back controls. One
PTY-only cleanup probe is ignored by the ordinary suite and exercised below.

The release binary passed **51 checks in a real Linux PTY**: all **24 existing
keyboard checks** plus **27 mouse checks** using actual SGR sequences. Three
additional isolated PTY checks cover a mouse-driven quit and returned-error/panic
cleanup (fault injection exists only in the test binary). The drivers verify
original `termios` flags, alternate screen, bracketed paste and mouse capture.
It is a development tool, not a runtime dependency:

```sh
python3 -m venv target/verification-venv
target/verification-venv/bin/pip install pyte==0.8.2
target/verification-venv/bin/python tools/verify_pty.py
target/verification-venv/bin/python tools/verify_cleanup.py
```

Evidence: [build/test output](verification/build-checks.txt),
[PTY report](verification/pty-report.json), [version metadata](verification/versions.json),
and decoded terminal snapshots under `verification/`. Full raw ANSI and replay
records from the run are in the ignored `target/pty-evidence/` directory.

Mouse update: [verification report](verification/mouse-report.md),
[build checks](verification/mouse-build-checks.txt),
[PTY checks](verification/mouse-pty-report.json),
[cleanup checks](verification/mouse-cleanup-report.json). The decoded mouse-update
screens were inspected at 80×24, 120×36 and 40×10; the screenshots below predate
the new footer controls.

Screenshots below were visually inspected after replaying the **actual PTY ANSI
output** in xterm.js, not recreated from the HTML design:

- [80×24](verification/80x24.png)
- [120×36](verification/120x36.png)
- [40×12, history scrolled](verification/40x12.png)

Terminal font/emulator Unicode-width differences can still affect unusual emoji;
CJK and accented fixture rendering were checked. These historical screenshots
do not establish live Zellij or real-agent behavior; see the plugin report above. The approved spec, HTML and existing matcher spike were
left untouched.

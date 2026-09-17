# Launchpad — native TUI prototype

A playable Ratatui interpretation of [SPEC.md](SPEC.md) and
[the approved HTML study](design/launchpad.html). The path-first layout, olive/charcoal
palette and pale green focus treatment carry over; browser chrome does not.

**Everything except input, fuzzy matching and rendering is simulated.**
No directory enumeration, PATH discovery, account access, subprocesses, Zellij,
network requests or persistent writes occur at runtime. The binary reads terminal
input and writes terminal output only. All paths, availability and history are
embedded fixtures under `/home/demo`.

## Run

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
| Anywhere | Ctrl+Q / Ctrl+C | Quit and restore the terminal |

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

Only unmodified left-button presses activate controls; release, drag, motion,
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

This is **not** a Zellij integration or WASM artifact. Async discovery, persistence,
permissions and actual pane replacement from the product specification are not
implemented. The UI and pure state/editor modules are the intended carry-forward
parts; the fixture provider and terminal placeholder are disposable.

## Verification

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
CJK and accented fixture rendering were checked. No live Zellij or real-agent
verification is claimed. The approved spec, HTML and existing matcher spike were
left untouched.

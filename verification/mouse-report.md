# Native mouse verification

## Artifact

`/home/piclaw/Projects/zellij-launchpad/target/release/zellij-launchpad-prototype`

Linux aarch64 release binary, rebuilt with the existing pinned toolchain/dependencies.
SHA-256: `050e51c5641daf4ddc0df27549df686996fb3de22060c8cc0e16026478d474d2`.

## Results

- `cargo fmt --check`: passed.
- `cargo test --locked`: **27 passed**, no failures. One PTY-only lifecycle probe is
  deliberately ignored here and was separately exercised in both error and panic modes.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo build --locked --release`: passed.
- Main real-PTY driver: **51 passed** = **24 unchanged keyboard checks + 27 mouse checks**.
- Isolated lifecycle PTYs: **3 passed** (returned error, unwinding panic, release-binary mouse quit).
- All four PTY sessions restored the original termios flags and disabled mouse
  capture, alternate screen and bracketed paste. Failure injection is test-only;
  the release binary has no additional environment-driven modes.

See [full command output](mouse-build-checks.txt), [main PTY report](mouse-pty-report.json)
and [lifecycle report](mouse-cleanup-report.json). Raw ANSI, per-snapshot replay
records and lifecycle streams are retained under `target/pty-evidence/`.

## Checked behavior

- Terminal-cell cursor placement, including CJK continuation cells, combining
  accents, emoji grapheme boundaries, padding, clipped tails and horizontally
  scrolled input. Byte offsets never substitute for terminal columns.
- Suggestions copy into the form only; tool/history clicks only select and focus.
  Scrolled suggestions/history and wrapped tools use their actual rendered positions.
- Explicit form launch and history replay; history copy-to-form; back/reopen,
  help close, reset and quit controls. Unavailable history targets still revalidate.
- Wheel navigation stays inside suggestions/history/help, clamps at list ends,
  and does not steal focus over input or margins.
- Release, right/middle click, drag, motion, modifiers, blank/out-of-bounds clicks,
  stale-size maps, queued resize clicks and below-minimum layouts do not launch.
- UI-layer hit regions are emitted during rendering in domain terms. Crossterm
  decoding remains in the native adapter; there is no duplicate layout algorithm.

The feature slices were observed failing before implementation (path clicks,
completion acceptance, tools, history actions, launch/back controls, wheel input,
mouse decoding, and the native PTY capture check), then rerun to green. The final
suite also includes negative/edge regressions. PTY expectations were corrected
for the existing third `notes` match and pyte's NFC normalization, not by changing
application results. Input-origin calculation was made linear rather than repeatedly
measuring every prefix; the long-Unicode render regression remains green.

## Visual inspection

Inspected actual decoded PTY output, not a recreated layout:

- [80×24 dashboard](mouse-dashboard-80x24.txt): all ten events still fit; global
  footer controls are complete, with no clipped path hint.
- [80×24 history](mouse-history-80x24.txt): separate `Tab copy` / `Enter replay`
  controls, selected row marker and original columns retained.
- [40×10 history](mouse-history-40x10.txt): wrapped tools, selected last entry,
  compact copy/replay and F1/F5/^Q controls remain visible.
- [120×36 tools](mouse-roomy-120x36.txt): original roomy history rhythm retained.
- [80×24 help](mouse-help-80x24.txt): wheel scrolling and a visible back control.

## Scope and caveats

Fixture-only runtime remains unchanged: no real launches, filesystem discovery,
network access or persistence. `SPEC.md`, `design/launchpad.html`, pinned versions
and the unrelated matcher spike were not edited. No commit or push was performed.

Mouse forwarding depends on the terminal/multiplexer. Native text selection may
require Shift. SGR has no resize-generation tag: queued coordinates are conservatively
ignored until a freshly drawn layout has a quiet input queue (100 ms poll interval).
A click during that brief interval may need repeating. Terminal fonts/emulators can
disagree about unusual emoji widths; pyte normalizes combining accents and does not
fully model ZWJ emoji, so grapheme assertions also use Ratatui's actual cell model.
SIGKILL cannot run cleanup. No live Zellij session was tested.

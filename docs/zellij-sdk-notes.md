# Zellij 0.45.1 integration findings

Reference notes behind the adapter's design decisions, kept with the other
implementation docs. Statements about run evidence describe the research pass
that produced them, not the current build; see
[the current verification record](../verification/plugin-only/REPORT.md).

Research used the published **zellij-tile 0.45.1 / zellij-utils 0.45.1** crates
and release tag **v0.45.1**, commit
`efd8fd5a89a20c07a111d248ad7fce53848d2c18`.
The installed executable returned `zellij 0.45.1`. Source checkout and downloaded
official documentation are retained under ignored `target/sdk-research/`.

## Official documentation consulted

- [Plugin events](https://zellij.dev/documentation/plugin-api-events): Key,
  Mouse and PastedText subscription/delivery; PastedText needs no permission.
- [Plugin commands](https://zellij.dev/documentation/plugin-api-commands):
  subscribe and close_self; the latter closes the plugin pane entirely.
- [Plugin permissions](https://zellij.dev/documentation/plugin-api-permissions.html).

Live docs can advance independently. Implementation decisions were checked
against the exact release source below, then exercised in the installed host.

## Release-source contracts

- [`zellij-tile/src/lib.rs`](https://github.com/zellij-org/zellij/blob/v0.45.1/zellij-tile/src/lib.rs):
  `ZellijPlugin::load/update/render`, `register_plugin!`. Returning true from
  update requests rendering; render also runs on startup/resize and receives
  content dimensions, excluding pane frames. The registration macro contains
  pre-2024 `#[no_mangle]` exports, so the thin plugin package uses edition 2021.
- [`zellij-utils/src/data.rs`](https://github.com/zellij-org/zellij/blob/v0.45.1/zellij-utils/src/data.rs):
  `KeyWithModifier` is BareKey plus a modifier set, without key event kind.
  Shift+Tab is Tab plus Shift, not a BackTab variant. PastedText is a dedicated
  Event, not a synthetic sequence of Enter/Tab actions.
- The same file's Mouse enum uses `(line: isize, column: usize)` for LeftClick,
  RightClick, Hold, Release and Hover. ScrollUp/Down carry only a line count.
  There are no pointer modifiers or wheel coordinates. Negative/out-of-range
  coordinates are rejected before the shared hit map sees them.
- [`plugin_pane.rs`](https://github.com/zellij-org/zellij/blob/v0.45.1/zellij-server/src/panes/plugin_pane.rs),
  `start_selection`, `update_selection`, `end_selection`, `mouse_event` and
  scroll methods: press => LeftClick, held selection => Hold, release => Release;
  unheld motion => Hover. Only LeftClick activates the application. A genuine
  drag was verified with a preceding press: an orphan button-held motion can
  instead be interpreted as the first press by the host.
- The same file's `handle_plugin_bytes` clears the viewport/scrollback and resets
  the cursor for **every render**. Therefore Ratatui incremental diffs alone
  would be wrong. We clear and reuse one Ratatui cell buffer and serialize every
  visible cell, skipping coordinates covered by wide glyphs. CUP starts each
  row explicitly; there is no newline at row ends or after the bottom-right
  cell. Styles reset initially, on transitions and at the end. Real styled
  spaces are retained. The plugin draws widgets directly into that buffer,
  bypassing terminal diffing and backend copies; printable ASCII cells bypass
  Unicode normalization and temporary string allocation. No raw mode, alternate
  screen or Crossterm in WASI.
- [`grid.rs`](https://github.com/zellij-org/zellij/blob/v0.45.1/zellij-server/src/panes/grid.rs),
  around lines 2457–2463: Zellij deliberately drops zero-width scalars and notes
  this breaks grapheme segmentation (upstream issue #1538). Live PTY output
  confirmed decomposed accents were lost. The serializer now composes NFC for
  display, and substitutes a padded replacement glyph when the scalar width
  would disagree with the shared Ratatui cell width. Editor bytes are unchanged.
- [`zellij_exports.rs`](https://github.com/zellij-org/zellij/blob/v0.45.1/zellij-server/src/plugins/zellij_exports.rs),
  `close_self`: sends ClosePane with `PaneId::Plugin(env.plugin_id)`, not the
  current pane/session. `check_command_permission` allows CloseSelf and Subscribe
  through its default no-permission branch. No permission request is made.
- [`wasm_bridge.rs`](https://github.com/zellij-org/zellij/blob/v0.45.1/zellij-server/src/plugins/wasm_bridge.rs),
  `check_event_permission`: Key, Mouse and PastedText need no permission.
  We deliberately do not subscribe to ModeUpdate, PaneUpdate or global input.

## Host constraints, not claimed parity

- No SDK key-repeat flag: repeated Enter is blocked by the shared terminal
  placeholder state, not by a fabricated key-release detector. Holding Enter
  across completion acceptance may then submit the completed path.
- No SDK mouse modifier data: modified mouse input cannot be filtered exactly
  like native Crossterm. Host routing (including Shift selection) applies.
- No SDK wheel position: the adapter uses the last delivered pointer position,
  not the focused section. With no position, it ignores the wheel. Hosts/terminals
  that do not forward motion cannot provide exact hover-local wheel behavior.
- No mouse layout generation: resize clears the cached pointer and imposes a
  100 ms quiet interval, extended by queued mouse events. This is a best-effort
  quarantine, not a claim that arbitrary network-delayed mouse reports can be
  identified as stale.
- The sample starts in locked mode; normal-mode host Ctrl+T interception and
  recovery after re-lock were live-tested. Ctrl+G then Ctrl+Q remains a host exit.
- The installed host silently loaded a stock tab for both file-based startup
  layout forms in isolated sessions. SDK parsing of the KDL succeeds. Using
  `--layout-string` with the file's exact contents loaded both named tabs and the
  WASM reliably; that is the documented startup command. The file-loader issue
  is not fixed or attributed to a specific upstream defect here.

## Verification lessons

- pyte 0.8.2 cannot parse Zellij's DCS/APC terminal probes and its private DSR
  handler has a narrower signature. The text-assertion adapter discards those
  control strings only; unmodified PTY bytes are retained and replayed in xterm.js.
- Help can mention clickable labels in prose. The harness targets the **last**
  occurrence (the footer), and explicitly asserts the dashboard after Back.
  An earlier loose `Launchpad` assertion matched the help title as well; it was
  tightened before the final two passing runs and final screenshot inspection.
- Native output and Zellij output are different evidence streams. The images in
  this directory replay the attached Zellij client's real PTY, not Ratatui buffer
  strings, the approved HTML, or a recreated web interface.

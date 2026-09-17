# Verification notes — 2026-09-17

- Native artifact: `target/release/zellij-launchpad-prototype`, Linux aarch64,
  922456 bytes, dynamically linked to the normal platform runtime.
- SHA-256: `cfee8dfee49bfef8194abd46d724d1c5814e0e15a1dddf35bd4bb2882316a1d3`.
- Final build gates: formatting, 16 Rust tests, warning-free Clippy, release build.
- Final real-PTY run: 24 checks; exit code 0; exact original termios restored.
- Screenshots replay captured ANSI through xterm.js 6.0.0. They are not HTML-design
  screenshots or manually fabricated terminal output. Browser-only blank surround
  was cropped from the 40×12 image. Raw session/replay data remains in
  `target/pty-evidence/`; the optional verification server was stopped afterward.
- Visually inspected 80×24, 120×36 and 40×12: path field dominates, source palette
  is retained, tool order is stable, history columns align including CJK fixtures,
  small history scrolls, and the expanded view uses spare rows for line spacing.
- Design self-audit: 0/10 generic-design flags. Command/Inspect composition;
  deliberate terminal monospace; no gradients, cards, decorative statistics,
  browser chrome, fake Zellij bars or taglines.
- Regression fixes during verification: deletion can join Unicode graphemes and
  must re-snap the cursor; minimum 40×10 layouts must reserve a visible history row.
  Both have tests that failed before the fixes.
- PTY harness detail: standalone Escape is sent separately from subsequent keys,
  otherwise terminal parsers can legitimately interpret the bytes as an Alt chord.
- No production Zellij/agent, filesystem, persistence or cross-platform testing
  is claimed. Runtime has no fixture files/assets or verification-tool dependency.
- Approved `SPEC.md`, `design/launchpad.html`, `.gitignore` and the existing matcher
  spike are untouched. No commit or push was performed. Global default Rust remains
  1.94.0; 1.98.1 is pinned only for this project.

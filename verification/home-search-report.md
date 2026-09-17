# HOME search verification

Verified on Zellij 0.45.1, Linux aarch64. Working tree based on `478e308`;
no commit or push. Existing demo/default separation and local spikes preserved.

## Reproduce

```sh
bash tools/verify_plugin.sh
```

This passed formatting, workspace tests, native and WASM clippy with `-D warnings`,
both locked release builds, WASM dependency check (no Crossterm), and all live
PTY suites below. Workspace: **53 passed, 1 ignored**; the ignored cleanup probe
was separately exercised by the real PTY cleanup harness.

| Live suite | Checks passed | Evidence |
|---|---:|---|
| search-plugin | 22 | `target/zj-rch-c381cd82` |
| search-native | 21 | `target/zj-rch-b5198736` |
| search-denied | 6 | `target/zj-rch-0796d60d` |
| search-real-home | 7 | `target/zj-rch-ac7e2452` |
| search-native-real-home | 6 | `target/zj-rch-01b3427e` |
| live-1 | 45 | `target/zj-322ddd1f` |
| live-2 | 45 | `target/zj-ac0d32b5` |
| native-pty | 51 | `target/plugin-verification/native-pty.log` |
| native-cleanup | 3 | `target/plugin-verification/native-cleanup.log` |

Full run: `target/home-search-final.log`. Machine-readable counts/locations:
`target/home-search-summary.json`. Per-command logs: `target/plugin-verification/`.
Controlled-HOME search screenshots: `target/zj-rch-c381cd82/01-search.png`,
`02-error.png`, `03-final.png`. Index-status contrast, path borders and result
alignment were also visually inspected from an actual xterm.js replay. Real-HOME
runs retain assertions only, not user directory screenshots/listings.

## Artifacts

- WASM: `target/wasm32-wasip1/release/launchpad-plugin.wasm`
  SHA-256 `9b2185845f72e8228d77426145c1dff6a5dd1a97b8f0bbca71248f54164af04d`
- Native: `target/release/zellij-launchpad-prototype`
  SHA-256 `9fe31bd862d11ffa9f337ea17f2eb8d209793d87a80d0bfbd980013f0d9ca546`

## Findings

- `ReadSessionEnvironmentVariables` resolves actual session HOME, which differs
  from the invoking CWD in the controlled-host tests. `/host` is remounted only
  after permission and traversed only after its matching acknowledgement.
- Live 0.45.1 rejects `ChangeHostFolder` unless granted **FullHdAccess**. This
  contradicts the current website's ChangeApplicationState wording. Confirmed in
  pinned `zellij_exports.rs:5602–5603`, as well as the first failed live probe's
  host log under `target/zj-rch-698c89ea/tmp/zellij-1000/zellij-log/zellij.log`.
- Frizbee finds nested real directories from bare abbreviations. Unicode, spaces,
  real mouse completion, dot-hidden filtering, deleted candidates, inaccessible
  directories, files and outside-HOME/symlink rejection were exercised live.
- Existing 160-column centering, compact guard, keyboard/mouse behavior, pane-only
  quit and cleanup remain verified. Legacy regression suites explicitly use demo
  mode; real startup does not fabricate history.
- TDD slices observed red before implementation, including control-name skipping,
  real indexing/validation APIs, restricted narrow-state visibility, history-copy
  dismissal during indexing, status contrast, and resize quarantine scheduling.

Policy/limitations: see [implementation notes](../docs/home-search.md). Indexing
is bounded and incremental, not a hard syscall deadline. All symlinks and names
containing invalid UTF-8/control characters are skipped. Launches/availability
remain simulated; no persistent history or configuration feature was added.

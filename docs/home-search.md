# HOME directory search

The native adapter gives `HomeIndex` the real HOME path as both displayed and
filesystem root. A background thread owns traversal, fuzzy matching and directory
validation. Filesystem work does not run on the terminal event loop.

## Traversal

The serial `ignore` walker uses its standard ignore handling, including parent
`.gitignore` and `.ignore` files, nested rules and negation, global Git ignores,
and `.git/info/exclude`. Git-related rules use the crate's default repository
detection. `node_modules` is indexed unless excluded by ignore rules.
Hidden directories are pruned. Symlinks are not followed.

Dot-prefixed entries are hidden on every platform. On Windows, the native walker
also checks `FILE_ATTRIBUTE_HIDDEN`; hidden entries and their descendants are
pruned without consuming the index's entry or retained-path budgets.
Ignore-file negations cannot reveal them. The Hidden attribute alone does not
prevent HOME itself from being scanned. F5 rebuilds the index and rechecks attributes;
explicit hidden paths remain valid launch targets.

The configuration's top-level `ignore` array adds absolute displayed host paths
whose directory trees are pruned before entry and retained-path budgets. Matching
uses complete path components: excluding `cache` does not exclude `cache-old`.
Ignore-file negations cannot override these exclusions. Ignoring HOME or an
ancestor skips traversal entirely, including for hidden HOME directories.
F5 keeps the loaded exclusions; reopen to load configuration edits. Literal
validation, invoking-directory exceptions and history replay are unaffected.

Traversal cooperatively yields after at most 128 entries or its existing 5 ms
budget. Limits are 200,000 retained directories, 2,000,000 visited entries,
depth 64 and 60 MiB of estimated retained index memory, excluding the walker's
internal allocations and ignore matchers. The UI reports limits and errors.
There is no external search process. While rebuilding, the previous snapshot
and the new index each have their own retained-data budget.

## Persistent index

The worker loads a versioned `index.json` before it begins traversal. Linux uses
`$XDG_CACHE_HOME/zellij-launchpad` (fallback `~/.cache/zellij-launchpad`), macOS
uses `~/Library/Caches/zellij-launchpad`, and Windows uses
`%LOCALAPPDATA%/zellij-launchpad` (fallback `~/AppData/Local/zellij-launchpad`).
Environment overrides must be absolute. Configuration and history locations are
unchanged.

A loaded index remains searchable during startup rebuilding and F5 refreshes.
The worker builds its replacement separately, and switches the searchable
snapshot when scanning finishes. Repeated F5 restarts only the pending scan.
Search revisions advance on content publication independently of directory count,
so replacements with fewer directories or the same count refresh the UI too.
Without a usable saved index, the first scan exposes results progressively.

The cache records HOME, normalized configuration exclusions, indexing limits,
and whether the scan reached a limit. Incompatible, malformed, or oversized
snapshots are ignored. Paths are checked lexically without accessing each saved
directory; selected directories still undergo live validation. Ignore-file and
filesystem changes are picked up by the background scan.

Completed scans, including scans ending at configured limits, are serialized to
a unique temporary file beside the destination, synced, closed, and renamed over
it. The previous file is never deleted before replacement. Concurrent instances
publish independently; the last successful publication wins. A crash before
publication leaves the previous index usable; abandoned temporary files are
ignored and can safely be removed when no instances are running.

Fatal scan errors retain the old index. Inaccessible subdirectories retain the
walker's existing skip behavior. Cache IO errors appear in search status without
disabling search or launch: a successful rebuild remains usable in memory even
if saving fails. Deleting `index.json` safely forces the next launch to scan from
scratch. There is no expiry timer or filesystem watcher.

## Matching and input

Frizbee ranks candidates using the existing normalization and matching behavior.
Each directory scores the better of its full path and its own name. Equal scores
are common because Frizbee ignores text after the best alignment, so ties are
broken in order by:

1. the directory's own name containing the query's last typed `/` segment
   (substring; skipped for a trailing `/`, `~`, `.` and `..`), so `repo/branch`
   prefers the checkout over folders inside it;
2. the most recent launch from history;
3. fewer path components;
4. path order.

Tie-breaks never outrank a higher score or change which directories match. The UI receives at most 100 paths / 64 KiB, rather than a copy of the catalogue.
Queries over 100 Unicode scalar values return no suggestions, including after
HOME expansion. This bounds matcher scratch allocation; short queries can still
match long directory names.

The editor caps typed/pasted input at 100 Unicode scalars. Completed, history and
invoking paths retain their full identity. Grapheme-aware cursor/deletion behavior
and Unicode display widths are preserved. Filesystem paths remain limited to
4096 UTF-8 bytes and may not contain controls.

## Validation

Relative paths start at HOME. `~` and `~/…` are supported; other tilde expansion
is not. Literal hidden/ignored paths may be validated even though not indexed.
Ordinary validation checks each component before reducing `..`, rejecting files,
symlinks and attempts to escape HOME. Opening the directory verifies accessibility.

Only the exact original invoking cwd, or its HOME-short spelling, bypasses the
ordinary HOME/symlink policy. It is re-opened on every launch attempt, without
expanding the index. Logical `$PWD` is used only when it resolves to the actual
process cwd. Missing or deleted cwd fails visibly with no fallback. F5 restores
that captured identity. Non-UTF-8 names are rejected without lossy conversion;
legitimate U+FFFD names work.

Validation is separate from OS process creation; concurrent filesystem changes
can still invalidate a checked path. This is not a filesystem sandbox or an
atomic validation/spawn security boundary.

## Verification

Core tests exercise ignore rules, limits, suggestion selection, input bounds and asynchronous
result fencing. Native tests exercise direct filesystem validation, invoking-cwd
exceptions and worker epochs. The live suite uses disposable HOME trees and real
terminal input; it never scans the user's HOME.

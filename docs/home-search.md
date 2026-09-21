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
Ignore-file negations cannot reveal them. HOME itself is always scanned even if
it has the Hidden attribute. F5 rebuilds the index and rechecks attributes;
explicit hidden paths remain valid launch targets.

Traversal cooperatively yields after at most 128 entries or its existing 5 ms
budget. Limits are 200,000 retained directories, 2,000,000 visited entries,
depth 64 and 60 MiB of estimated retained index memory, excluding the walker's
internal allocations and ignore matchers. The UI reports limits and errors.
There is no persistent index cache or external search process.

## Matching and input

Frizbee ranks candidates using the existing normalization and matching behavior.
The UI receives at most 100 paths / 64 KiB, rather than a copy of the catalogue.
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

Core tests exercise ignore rules, limits, completion, input bounds and asynchronous
result fencing. Native tests exercise direct filesystem validation, invoking-cwd
exceptions and worker epochs. The live suite uses disposable HOME trees and real
terminal input; it never scans the user's HOME.

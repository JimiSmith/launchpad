# Shared recent launches (native)

Launchpad remembers **ten unique directories**, newest first, using exact
validated absolute paths. Launching any tool in the same directory refreshes
that directory's recency. History loads at startup and F5. Selecting a recent
row fills Directory and preserves the current Tool; Enter and Launch use that
tool. Configured shortcuts launch their tool in the selected directory.
Ages are derived from stored UTC Unix seconds on load/refresh.

`history.json` is a version-3 JSON projection with `entries` containing
`id`, `path`, and `opened_at`. No tool ID or executable is stored.
`history.d/` is its small **authoritative recovery journal**. Both are
inside `$XDG_STATE_HOME/launchpad` (fallback `~/.local/state/launchpad`).
The native executable starts fresh and never imports or modifies plugin caches.
Removing only `history.json` does not clear history: refresh repairs it from the
journal. Use the UI's clear action. The core's test-only simulation never writes
this store.

## State schema

New journal records use version 3. Version-2 records remain readable: their
old tool fields are ignored, and entries for the same directory collapse to
the newest attempt. Version-2 clear markers still apply. Refresh writes a
version-3 projection and prunes superseded records. Old instances must be
closed before using the new version; mixed-version writers are not supported.
Other versions are ignored and retained for diagnosis, counting toward the
scan cap. A projection without a supported journal starts empty; refresh
replaces that disposable projection from the journal.

## What a record means

A record is a **validated launch attempt**, saved immediately before the host
replacement call. Successful replacement normally destroys Launchpad before
the CLI reply, so this is not a claim that the command started, authenticated,
or completed successfully. Pre-validation failures create no record. A known
Shell rejection or correlated agent rejection removes only that attempt ID; it
never restores an old entire snapshot over concurrent updates. Removal does not
restore entries already evicted by deduplication/the ten-directory cap. Unknown
outcomes can remain. Nonzero command exits do not erase history.

History failures never prevent the host launch call. A save failure is logged
without logging environment or history contents; immediate pane replacement may
prevent showing a warning. Clear and Delete retain the displayed rows and report
failure unless the store operation succeeds. A failure after journal publication
can leave a durable change despite a reported error; refresh reconciles it.
Clear retains the existing two-key confirmation and Escape cancellation.

## Coordination without a persistent lock

The native adapter retains the existing journal protocol. Atomic replacement of
a single JSON file loses concurrent updates. Expiring filesystem locks cannot safely fence a paused old writer.
Instead:

1. Each mutation gets a fixed-width wall-clock nanosecond prefix, native process
   ID and local counter. Ties have deterministic lexical ordering.
2. Write its versioned immutable journal record to a unique `create_new` temp,
   flush with `sync_all`, then publish with a same-filesystem rename. Clear is
   a record with no entry. There is no shared lock, lease, sleep, or owner cleanup.
3. Read valid immutable records, sort newest first, stop at the latest clear,
   deduplicate by exact validated path, and retain at most ten directories.
4. Atomically replace `history.json` with this projection. Prune only observed
   records superseded by a newer record for the same directory, ten newer distinct directories, or
   a newer clear. Never delete files absent from the collected snapshot.
5. Delete/rejection unlinks only the selected operation ID, then rematerializes.
   Refresh/reopen merges the journal and repairs a stale/missing/corrupt projection.

Concurrent launches have independent records, so replacing a stale projection
cannot lose them. Readers are **convergent, not linearizable**: during mutation or
compaction a read may temporarily miss a concurrently published/replaced record;
`history.json` can temporarily lag. Reopen/F5 after activity settles reconciles it.
Already-open panes are not live-subscribed to other panes' changes. Clear orders
against concurrent launch by the operation timestamp/ID, not callback arrival.
Wall-clock jumps can change recency ordering. State deletion during active writes
is not a transaction and is not a backup/restore mechanism.

## Bounds, faults, and filesystem policy

- After quiescent compaction: at most ten directory records plus one clear marker.
  Concurrent in-flight operations temporarily add records.
- Each journal scan and temporary-file scan stops at 128 directory entries; each
  journal file is read with a 32 KiB cap before parsing. Generated projection size
  is bounded by ten paths of at most 4096 UTF-8 bytes and fixed-schema metadata.
- Operation IDs are at most 100 ASCII alphanumeric/hyphen bytes. Paths must be absolute
  without controls or parent traversal; schema must be version 2 or 3,
  and a record's filename, ID and entry ID must agree. Unknown/corrupt records are
  ignored, not interpreted as executable data, and retained for diagnosis.
- Unreadable journal records are I/O failures, not corrupt content. A failed initial
  journal scan leaves the good projection untouched; refresh errors retain already
  displayed rows. Restoring access permits recovery. Only a record disappearing
  before open (concurrent compaction) is ignored. Pruning likewise ignores an
  already-removed record but reports other unlink failures; the projection may
  already have been published before such a failure. Launches remain allowed.
- State root, journal, records, projection and owned temporary paths reject
  symlinks/non-regular files where applicable. `create_new` rejects existing temp
  files without deleting them. These checks are not a TOCTOU security boundary
  against hostile concurrent filesystem replacement; state directory permissions apply.
- Abandoned regular `history-*.tmp` files older than 60 seconds are reclaimed on
  writes/refresh. A paused writer whose temp is reclaimed can only fail its own
  rename, never publish through another writer's lock. Fresh temps are untouched.
- No blocking retry occurs in the store. I/O is bounded in count/bytes,
  not wall-clock latency: a slow filesystem syscall can still stall an event.
- Oversized/corrupt state can hit the scan cap; the UI reports this and still
  allows launches. Move the affected state directory aside to recover; the old
  files are not automatically deleted.
- `sync_all` plus rename is used, but no power-loss durability
  or filesystem-directory-fsync guarantee is claimed.

## Verification

The native history module retains tests for concurrency, corruption, age,
deduplication, clear/delete and filesystem bounds. `tools/verify_native.py` checks
persistence and rejected-launch rollback in disposable real Zellij sessions.

## Saved directories

Saved is an explicit directory-only list, independent of launch history. Ctrl+S
validates the entered path (or selected list row) using the launch directory
policy, then saves its absolute path without launching. Entries are unique by
exact validated path, sorted lexically by full path, and never evicted by launches.
Selecting Saved preserves the current tool. Delete removes just that saved entry;
Ctrl+L only clears Recent. Missing directories remain saved until removed, and
launch validation reports their failure.

The shared Saved / Recent section opens on Saved when nonempty, otherwise Recent.
Left/Right switches lists when focused. F5 reloads both lists while preserving the
chosen list. Saving the first directory does not switch lists during the session.

`saved.json` in the same state directory contains
`{"version":1,"directories":["/absolute/path"]}`. `saved.lock` is a persistent file
used for an OS-managed exclusive lock; the lock itself is released when its file
handle closes, including process exit. Readers and mutations acquire it without
waiting. Contention reports a retryable error. Writers read the latest snapshot
under the lock, apply only the requested addition/removal, flush `saved.tmp`, and
atomically replace `saved.json`. This avoids overwriting another pane's unrelated
changes. A leftover regular temp from a crashed writer is replaced under the lock.
Do not remove the lock file while instances are running.

Snapshots are limited to 4 MiB, paths to 4096 UTF-8 bytes, and paths must be
absolute without controls or parent traversal. Unknown versions, corrupt files,
and symlinks/non-regular state files are rejected visibly without overwriting the
snapshot. Failed persistence preserves the displayed list. As with history,
filesystem calls may block on slow storage and no power-loss durability guarantee
is claimed. Already-open panes pick up other panes' changes with F5.

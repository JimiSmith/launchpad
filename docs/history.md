# Shared recent directories (Zellij 0.45.1)

The normal plugin remembers **ten unique absolute directories**, newest first,
and the last selected tool for each. It loads history after the worker's HOME
handshake (also after the guarded startup reload and F5). Initial tool remains
Shell; replay or copying a history row explicitly selects its remembered tool.
Ages are derived from stored UTC Unix seconds on load/refresh, not reset to now.
The existing keyboard/mouse layout and directory validation are unchanged.

`/cache/history.json` is a version-2 JSON projection with `entries` containing
`id`, `path`, `tool` (stable configured-command ID, or `shell`), and `opened_at`.
Replay uses this instance's current configuration, never an executable stored in
history. Label/executable changes do not change the stable association. Removed
or invalid IDs stay unavailable; Tab copies the path without substituting Shell.
`/cache/history.d/` is its small **authoritative recovery journal**. Both are
inside Zellij's plugin-URL-shared cache, not HOME, `/host`, or per-instance `/data`.
Changing the plugin URL separates history even if the WASM bytes are identical.
Deleting the URL cache clears history. Removing only `history.json` does not:
reopen/refresh repairs it from the journal. Do not manually edit the projection.
Native, `demo=true`, and `simulate_launch=true` never access this store.

## Cache schema

Only version-2 journal records are supported. IDs remain case-sensitive (a
configured `Claude` is distinct from `claude`). Old and unknown-version records,
including clear records, are ignored and never interpreted as commands. There
is no import, migration, staging directory, or compatibility lock. A projection
without a supported journal starts empty; refresh replaces the disposable
projection from the current journal. Unsupported journal files remain for
diagnosis and count toward the scan cap. Close old instances when upgrading;
mixed-version writers are not supported.

## What a record means

A record is a **validated launch attempt**, saved immediately before the host
replacement call. Successful replacement normally destroys the plugin before
`ActionComplete`, so this is not a claim that the command started, authenticated,
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

WASI does not provide the advisory file locks needed for a conventional locked
read-modify-write. Atomic replacement of a single JSON file loses concurrent
updates. Expiring filesystem locks cannot safely fence a paused old writer.
Instead:

1. Each mutation gets a fixed-width wall-clock nanosecond prefix, Zellij server
   PID, plugin ID, and local counter. Ties have deterministic lexical ordering.
2. Write its versioned immutable journal record to a unique `create_new` temp,
   flush with `sync_all`, then publish with a same-filesystem rename. Clear is
   a record with no entry. There is no shared lock, lease, sleep, or owner cleanup.
3. Read valid immutable records, sort newest first, stop at the latest clear,
   deduplicate by exact validated path, and retain at most ten directory rows.
4. Atomically replace `history.json` with this projection. Prune only observed
   records superseded by a newer same-path record, ten newer distinct paths, or
   a newer clear. Never delete files absent from the collected snapshot.
5. Delete/rejection unlinks only the selected operation ID, then rematerializes.
   Refresh/reopen merges the journal and repairs a stale/missing/corrupt projection.

Concurrent launches have independent records, so replacing a stale projection
cannot lose them. Readers are **convergent, not linearizable**: during mutation or
compaction a read may temporarily miss a concurrently published/replaced record;
`history.json` can temporarily lag. Reopen/F5 after activity settles reconciles it.
Already-open panes are not live-subscribed to other panes' changes. Clear orders
against concurrent launch by the operation timestamp/ID, not callback arrival.
Wall-clock jumps can change recency ordering. Cache deletion during active writes
is not a transaction and is not a backup/restore mechanism.

## Bounds, faults, and filesystem policy

- After quiescent compaction: at most ten directory records plus one clear marker.
  Concurrent in-flight operations temporarily add records.
- Each journal scan and cache-temp scan stops at 128 directory entries; each
  journal file is read with a 32 KiB cap before parsing. Generated projection size
  is bounded by ten paths of at most 4096 UTF-8 bytes and fixed-schema metadata.
- Operation IDs are at most 100 ASCII alphanumeric/hyphen bytes. Command IDs use
  the [configuration ID bounds](configured-commands.md). Paths must be absolute
  without controls or parent traversal; schema must be version 2,
  and a record's filename, ID and entry ID must agree. Unknown/corrupt records are
  ignored, not interpreted as executable data, and retained for diagnosis.
- Unreadable journal records are I/O failures, not corrupt content. A failed initial
  journal scan leaves the good projection untouched; refresh errors retain already
  displayed rows. Restoring access permits recovery. Only a record disappearing
  before open (concurrent compaction) is ignored. Pruning likewise ignores an
  already-removed record but reports other unlink failures; the projection may
  already have been published before such a failure. Launches remain allowed.
- Cache root, journal, records, projection and owned temporary paths reject
  symlinks/non-regular files where applicable. `create_new` rejects existing temp
  files without deleting them. These checks are not a TOCTOU security boundary
  against hostile concurrent filesystem replacement; host cache permissions apply.
- Abandoned regular `history-*.tmp` files older than 60 seconds are reclaimed on
  writes/refresh. A paused writer whose temp is reclaimed can only fail its own
  rename, never publish through another writer's lock. Fresh temps are untouched.
- No blocking retry or sleep occurs in the plugin. I/O is bounded in count/bytes,
  not wall-clock latency: a slow filesystem syscall can still stall an event.
- Oversized/corrupt caches can hit the scan cap; the UI reports this and still
  allows launches. Remove the affected URL cache to recover. No external helper
  executable, process probing, shell script, or new dependency is required.
- `sync_all` plus rename was exercised in real WASI, but no power-loss durability
  or filesystem-directory-fsync guarantee is claimed.

## Verification

Run `bash tools/verify_plugin.sh`, or the focused
`target/verification-venv/bin/python tools/verify_history.py` after building WASM.
The focused harness uses disposable `target/history-persistence/live-*` HOME,
PATH, config/cache/data, two actual PTYs/Zellij sessions and harmless executable
fixtures. Socket roots live under `target/hp-sock/` to fit Unix socket path limits.
It checks replacement/reopen, remount reload, cap/dedup/tool/order, stored age,
clear/cancel/delete, rejected spawn rollback, deleted directories, corrupt/missing
cache, symlink failures without blocking execution, simulation isolation, and
concurrent sessions sharing a URL. Native tests stress concurrent store operations
and parser/filesystem bounds. No test mode scans the actual user's HOME.

RED/GREEN logs, PTY records, exact execution logs, host logs, screenshots and the
final artifact hash are retained under `target/history-persistence/REPORT.md`.

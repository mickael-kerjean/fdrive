# Windows test brief

You are testing the fdrive Windows client after a rework of the deletion path.
Everything below is self-contained; you don't need prior session context.

## Background: what changed and why

Deletion semantics are now **"delete means delete"**: a local delete journals one
row (`'r'` for a file, `'d'` for a folder) and replays as one blind `sdk.rm`
against the server — recursive for folders, `NotFound` counts as success, and if
the rm errors but the target turns out to be already gone (stat/ls check), that
also counts as success. There are **no per-file leases, no emptiness guard, no
delete-vs-edit conflicts** anymore. A folder delete gesture also *subsumes* any
pending delete/save rows beneath it at journal time, so a big delete is one plan,
not hundreds.

The safety net against **bugs** (not against real user deletes) is the deletion
circuit breaker, active on Windows because deletes here are *inferred* from
watcher events, which a bug can fabricate. It counts delete gestures (a folder
counts as 1); at 50 in one session it trips: all pending server-side deletions
are held, and — new — the tripped state **persists across restarts** (row
`breaker_tripped` in the ledger's `meta` table). The new tray dialog is the only
release path.

Also new: a stall watchdog — if pending plans stop retiring for 2 minutes while
the scheduler is busy, the log gets `sync stalled: N pending plans ...` every 2
minutes.

## Environment

- Sync root: `%USERPROFILE%\Filestash`
- Data dir: `%LOCALAPPDATA%\Filestash` — `fdrive.log`, `fdrive.db` (sqlite),
  `fdrive.toml`
- Journal inspection: `SELECT seq, op, path FROM journal;` — op `'r'` = file
  delete, `'d'` = folder delete, `'s'` = save, `'w'` = dirty mark.
  Breaker state: `SELECT * FROM meta;`
- Useful log greps: `journaled rmdir`, `removed `, `deletions this session`,
  `sync stalled`, `breaker was tripped before restart`
- **Do not run an older build against a db this build has written**: new `'r'`
  rows carry no lease columns and an old loader silently drops them (pending
  deletes lost). Fresh db per binary, or forward-only.

## Test 1 — folder delete is one recursive rm

1. Sync a folder with a few hundred files, let it settle.
2. Delete the folder in Explorer.
3. Expect: log shows one `journaled rmdir <dir>/`, then one `removed <dir>/`;
   the journal briefly holds a single `'d'` row (plus at most a handful of `'r'`
   rows from per-file events that raced ahead — they must all retire); the server
   subtree is gone; tray returns to Ok; no rm-per-file storm in the log.

- [ ] pass
- [ ] fail → capture `fdrive.log` and the journal table

## Test 2 — breaker trip + tray dialog

1. Put 60 small synced files at the top level (individual files, not one
   folder), let them settle.
2. Select all 60 in Explorer and delete them.
3. Expect, within seconds: log line `60 deletions this session; further
   server-side deletions are held`; tray icon flips to the error icon with
   tooltip "Filestash — deletions held"; a Yes/No/Cancel warning dialog appears,
   default button **No**; **no file has been deleted on the server yet**.
4. Choose **Cancel** (decide later). Expect: dialog closes, deletions stay held,
   tray menu now has a "Held deletions..." entry at the top which reopens the
   same dialog. Server files still present.
5. Reopen via the menu, choose **Yes** (release). Expect: all 60 files disappear
   from the server; journal drains; tray returns to Ok; menu entry disappears.

- [ ] pass
- [ ] fail → note which step diverged, capture log

## Test 3 — cancel restores instead of deleting

1. Repeat test 2 steps 1-3 (fresh 60 files, trip the breaker).
2. In the dialog choose **No** (keep server files).
3. Expect: log `cancelled N held deletions; the server copies remain`; server
   files untouched; the local placeholders reappear after the next
   refresh/browse of the folder (server is source of truth).

- [ ] pass
- [ ] fail

## Test 4 — tripped breaker survives a restart

1. Trip the breaker (as in test 2), choose **Cancel** in the dialog.
2. Quit the app from the tray. Restart it and log in.
3. Expect: log `deletion breaker was tripped before restart; server-side
   deletions stay held`; within ~30s the dialog re-prompts (sweep tick); server
   files still present; "Held deletions..." menu entry present; choosing Yes
   drains the wave.

- [ ] pass
- [ ] fail

## Test 5 — suppression race: late watcher events after vacuum

The known-open race this file originally existed for. Vacuum deletes local
copies of clean files that still exist remotely (`vacuum`, adapter.rs, shielded
by `suppress(&root, ...)`). The suppression flag drops the instant `vacuum_dir`
returns, but ReadDirectoryChangesW delivers events asynchronously — any delete
event processed after the flag is down passes `is_suppressed` (`on_delete`),
gets journaled as a user delete, and replays as `sdk.rm`. With leases gone this
is strictly destructive now; the breaker is the only backstop, and only above 50
gestures. Same race in miniature on the placeholder-rebuild path (delete +
recreate of a file that is live remotely).

1. Sync a folder with thousands of small files, settle, confirm clean.
2. Trigger a vacuum that evicts a large number of cached files (logout does a
   vacuum; or force it via the maintenance path).
3. Watch the journal for `'r'` rows appearing for files never touched by the
   user, and the server for files disappearing. If the wave is big enough the
   breaker should trip and hold it — that is the backstop working, note it.
4. If nothing fires, tighten the race: add an artificial delay in the watcher's
   event-processing loop (or generate heavy parallel event load) and retry.

Verdict:
- [ ] does not reproduce → document why delivery is in-window (accept)
- [ ] reproduces → fix: suppression tombstones with a grace period (~30s TTL)
      instead of dropping the entry when the closure returns; adapter-only.

## Known gap — do not file as a new bug

A single *phantom folder* event (watcher hallucinates "folder X deleted") counts
as **1** breaker gesture but now nukes the whole subtree recursively — the
breaker cannot catch one-event/large-subtree bugs. Weighting folder gestures by
observation count is designed but not implemented. If test 5 reproduces via a
folder event, that is this gap amplifying it — record it, don't chase it.

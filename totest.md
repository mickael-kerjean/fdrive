# Windows test brief

You are testing the fdrive Windows client after a rework of the deletion path.
Everything below is self-contained; you don't need prior session context.

> **Status: run 2026-07-29, re-run 2026-07-30 against 192.168.68.105:8334,
> DESKTOP-RLMTOD9.** Results are recorded per test below. Read "Findings from the
> run" first — two of the five tests are blocked or invalidated by bugs found
> while running them, and the brief's own description of the breaker turned out
> to be wrong.
>
> **Rogue-deletion verdict (the question run 2 was commissioned to answer):
> no rogue remote deletion was observed in any vector tested.** The full server
> tree was captured byte-for-byte before the run (1748 files, 100,038,764 bytes)
> and matched **exactly** afterwards. Details in "Rogue-deletion assessment".
> This is evidence of absence under the loads applied, *not* proof — the
> `suppress()` window described in Test 5 still exists in the code.

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

### Tooling notes (learned the hard way, saves an hour)

- **No `sqlite3.exe` and no Python on this box.** `C:\Windows\System32\winsqlite3.dll`
  exists; drive it from PowerShell via `Add-Type` P/Invoke
  (`sqlite3_open_v2`/`prepare_v2`/`step`/`column_text`). Open read-only (flags=1)
  so you can query while the client holds the db.
  Watch out: PowerShell is case-insensitive, so a local `$db` handle silently
  clobbers a `$Db` parameter.
- **Driving the breaker dialog headlessly works.** Find the window by class
  `#32770` owned by the fdrive-windows pid, then
  `SendMessageW(hwnd, WM_COMMAND=0x0111, IDYES=6 | IDNO=7, 0)`.
  `EnumChildWindows` returns *nothing* for this dialog (same blind spot the
  tray menu has under UIA) — do not waste time trying to locate the buttons.
- **Driving the tray menu headlessly works.** Find the window of class
  `fdrive_tray` (note: `fdrive_`, not `fsync_`), then
  `PostMessageW(hwnd, 0x8001, 1, 0x0205)` to open the menu, `FindWindowW("#32768", null)`
  to get the popup, then `WM_KEYDOWN`(0x100) with `0x28` per arrow-down and `0x0D`
  for Enter. **2 downs + Enter = Refresh** when nothing is held.
- The API wants `Authorization: Bearer <token>` **and**
  `X-Requested-With: SDKHttpRequest`. A cookie will not do; token is in
  `fdrive.toml`.
- **The real data is on the `sftp` connection, not `virtualfs`.** `GET /api/config`
  lists both. `virtualfs` is the throwaway in-memory one (its root is `a`/`b`/`c`);
  the sync-root content lives on `sftp` (you can tell from the `Longname` field in
  an `ls` — real Unix perms). Note `login.json` claims `storage: virtualfs`, which
  is **stale and misleading**.
- **Logging in with credentials works** — the old note that it is "rejected (307,
  no cookie)" is stale. `POST /api/session/auth/?label=sftp` with
  `user=test&password=test` returns **303 + `Set-Cookie: auth=<token>`**; that
  cookie value *is* the bearer token.
- **`Logout` wipes `token` from `fdrive.toml`**, and `fdrive.toml.bak` holds an
  expired one (`list : permission denied`). If you drive Logout you must re-auth
  via the endpoint above to get back in — budget for it before you click.
- **Do not name a PowerShell helper function `Ls`.** Aliases outrank functions in
  PowerShell's resolution order, so `Ls` silently becomes `Get-ChildItem` and you
  will spend a while wondering why your "server listing" is enumerating the repo.
- Do not shell out to `powershell -NoProfile` from a Bash tool for these scripts:
  `curl.exe` does not resolve there, every listing comes back empty, and the
  script exits **0** having done nothing.

---

## Rogue-deletion assessment (run 2, 2026-07-30)

The question: **can this program go rogue and delete remote files the user never
deleted?** Every vector below was exercised deliberately. None fired.

**Method that makes this safe to test — use it again.** A spurious delete becomes
a **journal row before it becomes an `sdk.rm`**. So you can provoke the race, then
read `SELECT seq, op, path FROM journal` *before* anything replays, and catch the
bug red-handed without a single remote file being touched. Combine that with a
byte-exact full-tree snapshot (`walk.ps1`) taken before and after, and the whole
hunt is reversible.

| Vector | Result |
|---|---|
| Folder delete, 120 files (F1 path) | Breaker **held** it — no server-side delete |
| Server-side delete of a 150-file hydrated dir, open Explorer view, 400-file parallel write load | **0** spurious journal rows; one clean `dropped (gone remotely)` |
| **Vacuum via Logout** — the documented destructive path | Vacuum ran and evicted clean copies; journal **empty** afterwards. **0** rogue rows queued |
| Full-tree integrity, before vs after everything | **1748 files / 100,038,764 bytes → identical** |

Notes that matter for interpreting this:

- **The vacuum test is the important one** and it genuinely ran — proof is that the
  local root dropped from 7 entries to 4, evicting exactly the clean files
  (`4913`, the mp3, `jpg_original`) while keeping pinned `counting.txt` and the
  dirty jpg. That is correct vacuum semantics, and it journaled **nothing**.
- `vacuum()` wraps the **entire recursive walk** in one `suppress(&root, ...)`
  (`adapter.rs:839`), and `is_suppressed` matches descendants — so for the whole
  walk everything under the root is shielded. The exposure is only the tail of
  event delivery after the walk returns. That window is real in code but was not
  hit in practice.
- **The breaker's false-trip (F1) fails safe.** It holds deletions rather than
  issuing them. Annoying, not dangerous — do not "fix" it by loosening the budget
  in a way that lets a real wave through.
- Counting the mid-run fixtures reconciled exactly: the tree read 2348 files at
  peak = 1748 baseline + 600 fixture files (400 `noise` + 200 `vac`), and returned
  to 1748 after cleanup. No unexplained delta at any point.

### F6 — leftover local files are re-uploaded after logout+vacuum (new, not a delete risk)

Files that vacuum keeps (dirty or pinned) are **re-adopted as local edits on the
next login and uploaded**:

```
INFO  adapter  "adopting local edit counting.txt"
INFO  adapter  "re-adopted test/rogue/vac/f0.txt"
INFO  upload   "uploaded test/rogue/noise/n100.txt (11 bytes)"
```

Observed resurrecting a whole scratch tree that had been deleted server-side: the
local copies survived vacuum, then re-uploaded on login and recreated it remotely.
The direction is **upload, never delete**, so this is not a rogue-delete vector —
but it does mean local leftovers can silently undo a remote deletion, and a stale
local copy can be pushed over a newer server one. Worth a look on its own merits.

## Findings from the run

### F1 — the breaker counts watcher events, not gestures (severe, new)

**A single ordinary folder delete trips the breaker.** Deleting one folder
containing 200 files trips it instantly:

```
ERROR ... "50 deletions this session; further server-side deletions are held"
INFO  ... "journaled rmdir test/totest/bulk/"
```

This directly contradicts the brief's "It counts delete gestures (a folder counts
as 1)". The journal proves the engine's *subsumption* is working perfectly — after
deleting 200 files the journal held exactly:

```
seq | op | path
1   | d  | test/totest/bulk      <- one row, zero 'r' rows
```

So subsumption is not the problem; the **counter runs before it**.
`Engine::delete()` (`engine/facade.rs:61`) calls `breaker_note()` on *every*
invocation, and the adapter's `on_delete` (`fdrive-windows/src/adapter.rs:338`,
the only Windows caller) invokes it once per `ReadDirectoryChangesW` event.
Windows deletes a tree file-by-file, so a 200-file folder = 200 `breaker_note()`
calls, while the journal correctly collapses to 1 row.

Consequence: **any user deleting a folder of >50 files hits the "this may be a
bug" dialog every single time** — the exact false positive the breaker was
designed not to produce. Fix is to count what the journal counts (post-subsumption
gestures), not raw watcher events.

### F2 — an emptied directory caches as empty forever (new)

After its children are deleted, a directory is cached as listed-and-empty and
**never re-lists on its own**. Reproduced deterministically:

1. `test/totest/bulk` deleted (dir now empty locally).
2. `many60/` created on the server underneath `test/totest/`.
3. Local `test/totest` stays empty — polled for **120s**: never appears.
4. **Client restart does not fix it** (no re-list at startup).
5. Log shows **zero** list attempts for the path — it is serving a cached listing,
   not failing a fetch.
6. Tray **Refresh** recovers it instantly (`manual refresh: re-listing populated
   tree` → `many60` appears).

This is what blocks the e2e suite (see F5), and it is why the harness aborts with
`e2e dir never appeared locally: remote-to-local chain is broken`.

**Untested boundary, worth checking before you chase this:** all of the above was
measured with *no Explorer window open* on the directory (PowerShell
`Get-ChildItem` only). The e2e suite normally passes its `remote-create-file`
test, and it keeps an Explorer view open via `Open-View`. So the refresh may be
driven by an open view rather than by enumeration. Determine whether "no open
view" is a supported case before deciding this is a regression.

### F3 — held plans are reported as stalled (noise, new)

While the breaker holds a plan, it counts as a pending plan that never retires,
so the client logs indefinitely:

```
ERROR ... sync stalled: 1 pending plans, none retired: [rmdir test/e2e]   (every 2 min)
INFO  ... recovered 1 pending plans                                        (every 30s)
```

Held is not stalled. This is exactly the noise that would bury a genuine stall.

### F4 — the dialog is now two buttons, not three

The Yes/No/Cancel prompt was reduced to **Yes/No** (`MB_YESNO`, default No) and
the text shortened, at the maintainer's request:

```
Filestash blocked 60 deletions from reaching the server.
This many at once is unusual and may be a bug.

Delete them on the server too?

Yes — delete on the server
No — keep everything, restore here
```

All MessageBox dialogs moved out of `gui.rs`/`gui/tray.rs` into **`gui/alert.rs`**
(`alert`, `info`, `message_box`, `confirm_deletions`). `confirm_deletions` returns
`bool`; `tray.rs` keeps the `CTX`/event wiring. Anything that is not an explicit
Yes now routes to `DeletionsCancel`, so a failed MessageBox call falls to the safe
side.

**Consequence to be aware of:** "decide later" is gone, and `MB_YESNO` has no X
button and ignores Esc — the prompt is now forced-choice. That makes the
**"Held deletions..." tray entry effectively unreachable**, since you can no
longer leave the dialog up unanswered. It survives only in the narrow window
between a restart and the sweep tick re-prompting. Either restore a third option
or delete that menu entry — right now it is close to dead code.

### F5 — e2e `conflict-keeps-both` was stale; fixed but NOT yet verified

The suite reported `conflict-keeps-both FAIL`. Cause: conflict naming gained a
device segment (`engine/upload.rs:39`):

```rust
format!("{stem} (conflicted copy from {device}){ext}")   // device() = hostname, port.rs:18
```

The script still expected `c1 (conflicted copy).txt`, so both the `-match` and the
`Srv-Cat` path missed. **The code was right; the test was stale.**
`scripts/windows-e2e.ps1:488` now discovers the name from the server listing
instead of hardcoding it, so it survives running on another machine.

**This fix is still unverified end-to-end** — every attempt to re-run the suite
died in its own setup because of F2. Verify it once F2 is understood.

---

## Test 1 — folder delete is one recursive rm

1. Sync a folder with a few hundred files, let it settle.
2. Delete the folder in Explorer.
3. Expect: log shows one `journaled rmdir <dir>/`, then one `removed <dir>/`;
   the journal briefly holds a single `'d'` row (plus at most a handful of `'r'`
   rows from per-file events that raced ahead — they must all retire); the server
   subtree is gone; tray returns to Ok; no rm-per-file storm in the log.

**Run 2 reproduced this identically** with a 120-file folder: 1
`journaled rmdir test/rogue/bulk/`, journal held exactly one `'d'` row, breaker
tripped at 50. Deterministic, not a fluke of the 200-file case.

- [x] **pass — deletion semantics**, with 200 files:
      exactly 1 `journaled rmdir test/totest/bulk/`, exactly 1
      `removed test/totest/bulk/`, **0** per-file rm operations, server subtree
      gone. The journal held **one** `'d'` row and **zero** `'r'` rows — cleaner
      than the brief's "at most a handful".
- [x] **fail — breaker false-trips.** The same single gesture tripped the
      breaker (see **F1**), so the rm did not retire until released by hand.
      Both halves of this test are real: the delete path is correct, the counter
      is not.

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

- [x] **pass (steps 1-3).** Run in `test/totest/many60/` rather than the sync
      root top level — 60 individual file gestures either way. Trips at exactly
      50 (`50 deletions this session`, not 60 — it fires on crossing the budget,
      the message reports the count at trip time). Journal held **60 `'r'` rows**,
      `meta.breaker_tripped = 1`, dialog up.
      **All 60 files still on the server** — nothing leaked out ahead of the
      prompt, which is the assertion that matters most here.
- [ ] steps 4-5 **obsolete**: there is no Cancel button any more (**F4**).
      The release path itself is covered by Test 4 below.
- Tooltip text was **not** verified at runtime (reading a tray tooltip headlessly
  is not worth the effort); `gui.rs:118` maps `Status::Held` to
  `"Filestash — deletions held"` by inspection only.

## Test 3 — cancel restores instead of deleting

1. Repeat test 2 steps 1-3 (fresh 60 files, trip the breaker).
2. In the dialog choose **No** (keep server files).
3. Expect: log `cancelled N held deletions; the server copies remain`; server
   files untouched; the local placeholders reappear after the next
   refresh/browse of the folder (server is source of truth).

- [x] **pass.** `cancelled 60 held deletions; the server copies remain`; all 60
      still on the server; journal drained to 0 rows; `breaker_tripped` row gone.
      Local placeholders were **still absent immediately after** the cancel and
      came back only after a tray Refresh — all 60 restored. That matches the
      brief's "after the next refresh/browse", but note it does **not** happen on
      its own; see **F2**.

## Test 4 — tripped breaker survives a restart

1. Trip the breaker (as in test 2), choose **Cancel** in the dialog.
2. Quit the app from the tray. Restart it and log in.
3. Expect: log `deletion breaker was tripped before restart; server-side
   deletions stay held`; within ~30s the dialog re-prompts (sweep tick); server
   files still present; "Held deletions..." menu entry present; choosing Yes
   drains the wave.

- [x] **pass.** Step 1 adapted: with no Cancel button (**F4**), the process was
      killed with the dialog still unanswered, which is the same state.
      On restart: `deletion breaker was tripped before restart; server-side
      deletions stay held`, then `recovered 60 pending plans`; the dialog
      re-prompted on its own; all 60 files still on the server.
      Choosing **Yes** drained the wave — 60 × `removed test/totest/t4/fN.txt`,
      server count 0, journal 0 rows, `breaker_tripped` cleared.
- Independently corroborated earlier in the session: two unrelated restarts while
  held both logged the same line, so persistence is solid.

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

- [x] **does not reproduce** under a clean isolated test. Deleting a 60-file
      directory **server-side only** and letting the client mirror it produced:
      0 `journaled rmdir`, 0 breaker trips, 0 spurious journal rows, and a single
      clean `dropped test/totest/many60 (gone remotely)`. Suppression held for the
      whole subtree.

**But there is an unexplained counter-observation — do not close this out yet.**
Earlier in the session, during an e2e **teardown**, which is server-side only
(`Close-Views; Srv-Rm "$SrvE2E/"` — no local delete anywhere), the client logged:

```
23:36:45 ERROR "50 deletions this session; further server-side deletions are held"
23:36:45 INFO  "journaled rmdir test/e2e/many/"
23:36:45 INFO  "journaled rmdir test/e2e/"
```

`journaled rmdir` can only come from `Engine::delete()`, whose only Windows caller
is the watcher handler `on_delete` — so watcher events for a tree *the client
removed itself* were journaled as user gestures. Note the isolated repro logged
`dropped ... (gone remotely)` and this did not, so a **different code path**
handled the removal. The difference is probably load and open Explorer views (the
e2e run had views open and heavy parallel event traffic); the isolated test had
neither.

Next step is the brief's own suggestion: reproduce under heavy parallel event load
and with views open, and instrument which path removes the local copy. Note also
that `suppress()` (`adapter.rs:54-70`) still drops its entry the instant the
closure returns, so the theoretical window is unchanged — it just was not hit by a
quiet single-threaded delete.

### Run 2 (2026-07-30) — vacuum path exercised properly, still does not reproduce

Run 1 only managed the isolated server-side delete. Run 2 did the step this test
actually asks for — **trigger a vacuum via Logout** — plus a loaded variant:

1. **Vacuum via Logout**, with 200 freshly hydrated files under `test/rogue/vac/`
   so vacuum had plenty of clean copies to evict. Vacuum ran (local root 7 → 4
   entries, clean files evicted, pinned/dirty kept). Journal afterwards:
   **0 rows**. No rogue delete was even *queued*, let alone sent.
2. **Loaded race**: 150-file hydrated dir, an Explorer view open on it (via
   `explorer.exe`, as the harness does), and a background job writing 400 files
   into a sibling dir to saturate `ReadDirectoryChangesW` — then delete the dir
   **server-side**. Result: **0** spurious rows, 0 breaker trips, one clean
   `dropped test/rogue/race (gone remotely)`. The noise was deliberately
   write-only so that *any* delete row would be unambiguous evidence.

Full-tree byte-exact comparison across the entire run: **no change** (1748 files,
100,038,764 bytes).

Verdict:
- [x] does not reproduce — **under a clean isolated delete (run 1), under vacuum
      via Logout (run 2), and under heavy parallel event load with an open view
      (run 2)**. Suppression held every time.
- [ ] reproduces → fix: suppression tombstones with a grace period (~30s TTL)
      instead of dropping the entry when the closure returns; adapter-only.

**Still do not close this out.** Three reasons:
1. The `suppress()` window is **unchanged in code** (`adapter.rs:54-70` still drops
   the entry when the closure returns). Nothing was fixed; the race was merely not
   hit. A faster disk, more files, or a slower event pump could change that.
2. The run-1 teardown anomaly below is **still unexplained** — and note run 2's
   clean mirrors all logged `dropped ... (gone remotely)`, which the anomaly did
   *not*. Whatever path ran there is still unidentified.
3. The phantom-folder gap (see "Known gap") is untested and unaffected by any of
   this — one fabricated folder event still means a recursive subtree rm that the
   breaker counts as 1.
   If you want certainty rather than "did not reproduce", implement the tombstone
   TTL anyway; it is cheap and it closes the window by construction.

## Known gap — do not file as a new bug

A single *phantom folder* event (watcher hallucinates "folder X deleted") counts
as **1** breaker gesture but now nukes the whole subtree recursively — the
breaker cannot catch one-event/large-subtree bugs. Weighting folder gestures by
observation count is designed but not implemented. If test 5 reproduces via a
folder event, that is this gap amplifying it — record it, don't chase it.

> Note how this interacts with **F1**: the breaker undercounts the one case that
> is genuinely dangerous (a phantom folder event = 1) and overcounts the one case
> that is genuinely benign (a real 200-file folder delete = 200). Both point at
> the same fix — count gestures after subsumption, and weight a folder by what it
> actually removes.

## Suggested order of work

1. **F1** — breaker counts events, not gestures. Cheapest fix, removes a
   guaranteed false positive on routine use, and unblocks Test 1.
2. **F2** — stale empty-directory cache. Blocks the whole e2e suite; settle the
   "is an open Explorer view required?" question first.
3. **F5** — re-run the e2e suite to verify the conflict-naming fix, once F2 is
   resolved.
4. **F4** — decide whether "Held deletions..." keeps a reachable path.
5. **F3** — stop counting held plans as stalled.
6. **Test 5** — chase the teardown anomaly under load.

## State left behind (end of run 2)

- Server tree verified **byte-exact** against the pre-run snapshot: 1748 files,
  141 dirs, 100,038,764 bytes. All scratch data (`/test/totest/**`,
  `/test/rogue/**`) removed; `/test/e2e` remains from the harness.
- Client running against `http://192.168.68.105:8334`, sync root shows all 7
  entries, journal **empty**, breaker **not** tripped.
- **`fdrive.toml` holds a freshly minted `sftp` token** (the Logout in run 2 wiped
  the original). Previous config saved as `fdrive.toml.pre-restore`. The token has
  `Max-Age=604800`, so it expires around **2026-08-06**; re-auth per the tooling
  note when it does.
- A full byte-exact backup of the server tree was taken before the destructive
  work and is **still on disk** at
  `%LOCALAPPDATA%\Temp\claude\C--Users-micka-Documents-fsync\3cf2803d-bb14-47db-90c4-ec861e2c7778\scratchpad\backup`
  (1748 files, 95.4 MB, includes `filestash-sync/.git`). It was never needed.
  Delete it when you no longer want a restore point — it is in a temp dir and will
  not survive indefinitely.
- Uncommitted: the `gui/alert.rs` extraction + dialog rewrite, the
  `scripts/windows-e2e.ps1` conflict-name fix, and this file.

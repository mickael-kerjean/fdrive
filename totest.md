# Windows test brief — round 3

You are testing the fdrive Windows client. Self-contained; no prior session
context needed. Rounds 1-2 (2026-07-29/30) established that **no rogue remote
deletion occurs under any vector tested** (byte-exact full-tree verification,
1748 files) and filed findings F1-F6; the fixes for F1/F3/F4 have landed since
and are what this round verifies, plus the still-unexplained run-1 anomaly.

> **Run 3 in progress (2026-07-30).** Tests 1 and 2 **pass** — see their sections
> for measurements. Test 3 was mid-observation when this was written; tests 4-6
> not yet run. Commits added during run 3: `9f84f08` (slow-folder-delete
> coalescing + regression test), `37243ad` (F3 stall scoped to unheld work),
> `98c544e` / this file (brief upkeep). Core suite 107 + 10 green, fmt and clippy
> clean.
>
> **Headline measurement:** a 2000-file folder delete takes ~18s of event
> streaming and still resolves to **one** recursive rm with **zero** breaker
> gestures. The same run showed the `WINDOW_DELETE_MAX` cliff sits near 3300
> files — read the Test 1 notes before touching the window constants.
>
> Baseline server tree re-verified before destructive work: 1748 files / 141 dirs
> / 100,038,764 bytes, unchanged from run 2.

## What changed since round 2

- **F1 fixed — breaker counts journal gestures, not watcher events.** Counting
  moved from `Engine::delete()` into the journal layer: only plans that survive
  coalescing/subsumption count ('r' plan = 1, 'd' plan = 1). A folder delete of
  200 files is now **1** gesture. Budget still 50 per session.
- **F3 fixed — held is not stalled.** No `sync stalled` lines while the breaker
  holds; `recovered N pending plans` logs once at startup, not every 30s.
- **F4 done — "Held deletions..." menu entry removed.** The forced-choice
  Yes/No dialog (default No) is the only surface; after a restart while held,
  the sweep re-prompts within ~30s.
- **Follow-up to F1 (commit `9f84f08`) — a slow folder delete stays one gesture.**
  F1 alone only held for folders whose events fitted inside `WINDOW_MAX` (2s):
  past that the window force-flushed mid-burst, the children became counted `'r'`
  plans *and* went out as individual `rm` calls, and the folder gesture arrived
  too late to fold them. The cap is now taken from the oldest **non-delete** op,
  with `WINDOW_DELETE_MAX` (30s) as a backstop, so a pure-delete burst waits for
  the 250ms quiet gap — which is where the directory event lands.
- Unchanged and deliberate: no suppression tombstone was added. The `suppress()`
  drop-on-return window (adapter.rs:54-70) still exists; the breaker is the
  intended net until the adapter restructure removes suppression entirely.
- **F3 hardened further (commit `37243ad`).** The original fix reset the stall
  timer whenever *any* deletion was held, which disabled the watchdog wholesale —
  a genuinely stuck upload could never report while the breaker held. The stall
  check is now driven by `pending_unheld()` (pending plans minus held removes), so
  it stays quiet when everything outstanding is held but still fires for unheld
  work. `stall_report` samples unheld plans only, so the message names what is
  actually stuck. `State::pending()` was dead after this and is gone.
- **Known, accepted, not a bug to file:** breaker accounting sits downstream of a
  timing heuristic, so a window miss escalates from "wasteful" to "false-positive
  dialog". Past `WINDOW_DELETE_MAX` the old fragmentation returns (no data loss —
  plans not yet inflight are still subsumed, the server still converges). The
  durable fix is to make the breaker count timing-independent (distinct delete
  roots, not plans made per window); deliberately deferred to keep this simple.
  **Run 3 put a number on the cliff: ~9 ms per file measured, so 30s ≈ 3300
  files.** That is an ordinary folder, not an edge case, which strengthens the
  case for the timing-independent count over tuning the constant. `WINDOW_MAX`
  itself (2s) is exceeded by as few as ~150 files.

## Environment

- Sync root: `%USERPROFILE%\Filestash`; data dir: `%LOCALAPPDATA%\Filestash`
  (`fdrive.log`, `fdrive.db`, `fdrive.toml`)
- Journal: `SELECT seq, op, path FROM journal;` — 'r' file delete, 'd' folder
  delete, 's' save, 'w' dirty mark. Breaker: `SELECT * FROM meta;`
- Log greps: `journaled rmdir`, `removed `, `deletions this session`,
  `sync stalled`, `breaker was tripped before restart`, `recovered `
- Server previously at `http://192.168.68.105:8334`; token in `fdrive.toml`
  (minted 2026-07-30, `Max-Age=604800` → expires ~2026-08-06; re-auth per
  tooling notes if needed).
- **Build first from the current tree** — the fixes exist only in source.
- **Do not run an older binary against a db this build wrote** ('r' rows carry
  no lease columns; old loaders silently drop them).

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
  `EnumChildWindows` returns *nothing* for this dialog — do not waste time
  trying to locate the buttons.
- **Driving the tray menu headlessly works.** Window class `fdrive_tray`
  (note: `fdrive_`, not `fsync_`), then `PostMessageW(hwnd, 0x8001, 1, 0x0205)`
  to open the menu, `FindWindowW("#32768", null)` for the popup, then
  `WM_KEYDOWN`(0x100) with `0x28` per arrow-down and `0x0D` for Enter.
  **Menu layout changed this round** (the held entry is gone): when logged in
  the order is Browse, Refresh, Logs, Autostart, ---, Logout, Restart, Quit —
  so **2 downs + Enter = Refresh** in all states now. Re-verify before relying
  on it.
- The API wants `Authorization: Bearer <token>` **and**
  `X-Requested-With: SDKHttpRequest`. A cookie will not do.
- **The real data is on the `sftp` connection, not `virtualfs`** (`GET
  /api/config` lists both; `virtualfs` is the in-memory throwaway with root
  `a`/`b`/`c`). `login.json` claiming `storage: virtualfs` is stale.
- **Login**: `POST /api/session/auth/?label=sftp` with `user=test&password=test`
  returns 303 + `Set-Cookie: auth=<token>`; that cookie value is the bearer
  token.
- **`Logout` wipes `token` from `fdrive.toml`** — budget a re-auth before you
  drive it.
- **Do not name a PowerShell helper function `Ls`** (alias outranks it) and do
  not shell out to `powershell -NoProfile` from a Bash tool (`curl.exe` does not
  resolve; scripts exit 0 having done nothing).
- Take a full-tree byte-exact snapshot before destructive work (`walk.ps1`
  pattern from run 2) and diff it at the end. A spurious delete becomes a
  journal row before it becomes an `sdk.rm` — read the journal before anything
  replays and you catch bugs without losing a file.

## Test 1 — F1: a folder delete no longer trips the breaker

1. Sync a folder with 200 files, settle, then delete the folder in Explorer.
2. Expect: NO `deletions this session` line, NO dialog; one
   `journaled rmdir`, one `removed <dir>/`, subtree gone from the server,
   journal drains to 0.
3. Repeat with a 60-file folder for good measure — same expectation.
4. **Then a folder big enough that its delete events outlast `WINDOW_MAX` (2s) —
   2000+ files.** 200 files delete well inside the window, so steps 1-3 pass with
   or without `9f84f08` and prove nothing about it. This is the case that does.
   Same expectation, plus: count the `rm` calls — there must be **one**, not one
   per file. Fragmentation into N calls means the burst outran
   `WINDOW_DELETE_MAX` or the quiet gap was missed; capture the timing.

- [x] **pass (200)** — [x] **pass (60)** — [x] **pass (2000)** (run 3, 2026-07-30)

Every case: `deletions this session` **0**, `journaled rmdir` **1**, `removed <dir>/`
**1**, per-file `removed` **0**, journal drained to 0, `breaker_tripped` absent,
server subtree gone. Measured wall-clock for the Explorer delete:

| files | delete took | vs `WINDOW_MAX` (2s) |
|---|---|---|
| 200 | 2511 ms | already past it |
| 60 | 3053 ms | already past it |
| 2000 | **17912 ms** | 9× past it |

Two things fell out of this that matter more than the pass:

1. **Step 4's premise was wrong — 200 files already outlast the window.** Even the
   small cases took >2s, so they were *already* hitting the fragmentation path
   before `9f84f08`. The fix matters at far smaller folder sizes than assumed.
   Do not treat the 200-file case as a control; there isn't one below ~150 files.
2. **The `WINDOW_DELETE_MAX` cliff is reachable in normal use.** 2000 files in
   17.9s is ~**9 ms/file**, so the 30s ceiling lands at roughly **3300 files** —
   an ordinary photo folder, `node_modules`, or build tree. Past it you get
   fragmentation into thousands of individual `rm` calls *plus* a false-positive
   dialog. See "Known open items" — this is the concrete argument for making
   breaker accounting timing-independent rather than tuning the constant. Cheap
   stopgap if that waits: raising `WINDOW_DELETE_MAX` to 5 min buys ~33k files at
   no practical cost.

## Test 2 — regression: 60 individual deletes still trip

1. 60 individual synced files at one level; select all, delete in Explorer.
2. Expect: trips at 50, dialog (Yes/No, default No), nothing on the server
   deleted before you answer. Answer **No**: `cancelled N held deletions`,
   server intact, placeholders return after a tray Refresh.

- [x] **pass** (run 3) — tripped, journal held **60 `'r'` rows**, and **all 60
      files were still on the server** before answering, which is the assertion
      that matters. Dialog up (class `#32770`).
- The trip line read **`63 deletions this session`**, not 60. That is correct and
  is useful corroboration: 3 = the three Test 1 folder deletes contributing
  exactly **one gesture each**, + 60 individual files, all counted in one journal
  swap. If you ever see this number scale with folder *contents* rather than
  folder *count*, F1 has regressed.
- Note the budget is cumulative per session, so Test 1 spends 3 of the 50 before
  Test 2 starts. Restart the client between tests if you want an exact threshold
  measurement.

## Test 3 — F3: held is quiet

While the breaker is held (re-trip via test 2, leave the dialog unanswered):

1. Wait ≥5 minutes. Expect **zero** `sync stalled` lines in that window.
2. Expect `recovered N pending plans` to appear **only** after a process start,
   never on the 30s cadence.

- [ ] pass
- [ ] fail → paste the offending lines with timestamps

## Test 4 — F4 + persistence: dialog is the only surface and survives restart

1. Trip the breaker, kill the process with the dialog unanswered.
2. Restart + login. Expect: `breaker was tripped before restart...` in the log,
   dialog re-prompts within ~30s, server files still present.
3. Open the tray menu: **no "Held deletions..." entry** in any state.
4. Answer **Yes**: wave drains, `breaker_tripped` row gone, tray back to Ok.

- [ ] pass
- [ ] fail

## Test 5 — F5: full e2e suite run

`scripts/windows-e2e.ps1` — the `conflict-keeps-both` fix (hostname-tagged
conflict names) is still unverified end-to-end. The harness keeps Explorer
views open, which sidesteps the F2 stale-listing behavior; don't remove that.

- [ ] suite passes (incl. conflict-keeps-both)
- [ ] failures → list per test; if setup dies on a directory never appearing,
      that is F2 (known), note it and move on

## Test 6 — the run-1 teardown anomaly (the real hunt)

During a run-1 e2e **teardown** — server-side `rm` only, no local deletes — the
client logged `journaled rmdir test/e2e/many/` + a breaker trip: watcher events
for a removal *the client itself performed* were journaled as user gestures.
The clean isolated repro logs `dropped ... (gone remotely)` instead; whatever
path ran there is unidentified. Suspected ingredients: open Explorer views +
heavy parallel event load during the remote-to-local mirror.

1. Recreate run-1 conditions: e2e-style tree (~100+ files), Explorer views open
   on it, parallel write load in a sibling dir, then delete the tree
   **server-side** and let the client mirror it.
2. Watch the journal live. Any 'r'/'d' row for that tree is the anomaly firing.
   The breaker should hold a big wave (that is how it was caught last time) —
   **do not release it**; read the journal, capture the surrounding log
   (which lines removed the local copies — `dropped`? refresh? something else),
   then answer **No** to discard.
3. Use an expendable tree only: a wave under 50 rows will NOT be held and will
   replay against the server.
4. Repeat a few times; the race is load-dependent.

- [ ] does not reproduce (n attempts: __) → note conditions tried
- [ ] reproduces → capture journal rows + the log window around them; identify
      the code path that deleted the local files; do NOT chase a fix, report

## Known open items — do not file as new bugs

- **F2**: an emptied directory serves a cached empty listing until an Explorer
  view drives a refresh; restart does not re-list. Unresolved design question.
- **F6**: files surviving vacuum (dirty/pinned) are re-adopted and re-uploaded
  on next login, which can resurrect a server-side deletion. Upload-direction;
  ruling pending.
- **Phantom folder gap**: a single fabricated folder event = 1 gesture but a
  recursive subtree rm; weighting is designed, not implemented.
- **`suppress()` window**: unchanged by decision, breaker is the net. If test 6
  reproduces, that window is likely the mechanism — evidence wanted, not a fix.

## Server state (as left by run 2)

Baseline tree: 1748 files / 141 dirs / 100,038,764 bytes, verified byte-exact.
`/test/e2e` remains from the harness. A full backup from run 2 may still be at
`%LOCALAPPDATA%\Temp\claude\...\scratchpad\backup` (95.4 MB) — reuse or refresh
it before destructive work.

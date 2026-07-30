# Windows test brief — round 4

You are testing the fdrive Windows client. Self-contained; no prior session
context needed. Read "What changed" first — **the deletion circuit breaker no
longer exists**, which invalidates half of the round-3 brief you may find in
git history and changes how carefully you must handle destructive tests.

## What changed since round 3: the breaker was removed entirely

Round 3's own measurements sealed it: the breaker's counter could not
distinguish a real folder delete from a phantom event no matter where it
counted (raw events → per-file dialog on every folder; journal plans →
hostage to window timing, false dialog again past ~3300 files), and each fix
leaked complexity into unrelated core code. Decision: delete the mechanism,
then make rogue deletion impossible architecturally (adapter restructure,
separate effort — not your concern this round).

Gone, entirely: gesture counting, budget, `tripped` state and its `meta`
persistence, the hold on pending deletes, `deletions_held/release/cancel`, the
Yes/No dialog, the `Held` tray status. `Engine::start` no longer takes a
`Deletions` mode. **You will never see a breaker dialog, a
`deletions this session` log line, or a `breaker_tripped` row. If you do, you
are running a stale binary — rebuild.**

Kept from round 3's work (still worth verifying):
- The **delete-aware window** (`WINDOW_DELETE_MAX` 30s): a long pure-delete
  burst waits for its folder event so the wave collapses to one recursive rm.
- The **stall watchdog**, back in its simple form: any pending plan not
  retiring for 2 min logs `sync stalled: ...` every 2 min. There is no "held"
  carve-out anymore because nothing holds.
- The `meta` table (schema version anchor) — `version = 1` only.

**Consequences for you:**
1. Deletes replay to the server **immediately**. There is no safety net that
   pauses a wave, and no dialog to answer. Every destructive test is live.
2. Take the byte-exact full-tree snapshot BEFORE anything destructive
   (`walk.ps1` pattern), and diff after. This is now your only rollback.
3. The anomaly hunt (test 3 below) previously relied on the breaker to catch
   and hold a phantom wave. Now a phantom wave **executes**. Run it only
   against expendable trees, and watch the journal live — a spurious delete is
   a journal row before it is an `sdk.rm`, so fast eyes still catch it
   pre-damage if replay is slow, but do not count on that.

## Environment

- Sync root: `%USERPROFILE%\Filestash`; data dir: `%LOCALAPPDATA%\Filestash`
  (`fdrive.log`, `fdrive.db`, `fdrive.toml`)
- Journal: `SELECT seq, op, path FROM journal;` — 'r' file delete, 'd' folder
  delete, 's' save, 'w' dirty mark. `SELECT * FROM meta;` should show only
  `version|1`.
- Log greps: `journaled rmdir`, `removed `, `sync stalled`, `dropped `,
  `recovered `
- Server: `http://192.168.68.105:8334`; token in `fdrive.toml` (minted
  2026-07-30, expires ~2026-08-06; re-auth per tooling notes).
- **Build from the current tree first.**
- Do not run an older binary against a db this build wrote.

### Tooling notes (learned rounds 1-3, saves an hour)

- **No `sqlite3.exe` and no Python on this box.** `C:\Windows\System32\winsqlite3.dll`
  exists; drive it from PowerShell via `Add-Type` P/Invoke
  (`sqlite3_open_v2`/`prepare_v2`/`step`/`column_text`). Open read-only (flags=1)
  so you can query while the client holds the db. PowerShell is
  case-insensitive: a local `$db` clobbers a `$Db` parameter.
- **Driving the tray menu headlessly works.** Window class `fdrive_tray`, then
  `PostMessageW(hwnd, 0x8001, 1, 0x0205)` to open the menu,
  `FindWindowW("#32768", null)` for the popup, `WM_KEYDOWN`(0x100) with `0x28`
  per arrow-down and `0x0D` for Enter. Menu when logged in: Browse, Refresh,
  Logs, Autostart, ---, Logout, Restart, Quit → 2 downs + Enter = Refresh.
  (The dialog-driving recipe from round 3 is obsolete — no dialog exists.)
- The API wants `Authorization: Bearer <token>` **and**
  `X-Requested-With: SDKHttpRequest`.
- **The real data is on the `sftp` connection, not `virtualfs`** (`GET
  /api/config`; `virtualfs` is the in-memory throwaway). `login.json` claiming
  `storage: virtualfs` is stale.
- **Login**: `POST /api/session/auth/?label=sftp` with `user=test&password=test`
  → 303 + `Set-Cookie: auth=<token>`; the cookie value is the bearer token.
- **`Logout` wipes `token` from `fdrive.toml`** — budget a re-auth.
- Don't name a PowerShell helper `Ls`; don't run these scripts via
  `powershell -NoProfile` from a Bash tool (`curl.exe` missing, silent exit 0).

## Test 1 — delete semantics without the breaker

1. Folder with 200 files: delete in Explorer. Expect one `journaled rmdir`,
   one `removed <dir>/`, one rm call, **no dialog, no held state**, server
   subtree gone, journal drains to 0. It now proceeds immediately.
2. Folder with 2000+ files (outlasts the 2s general window; round 3 measured
   ~9ms/file): same expectation — the delete-aware window must still collapse
   it to **one** rm. Count the rm calls.
3. 60 individual files, select-all delete: expect 60 rm calls, all proceed,
   journal drains, no dialog. (Round 3 held this wave; now it just executes.)

- [ ] pass (200) — [ ] pass (2000) — [ ] pass (60 individual)
- [ ] fail → log + journal + rm-call count

## Test 2 — watchdog sanity

1. Block the server (firewall the port or stop it) with a pending upload or
   delete queued. After ~2 min expect `sync stalled: N pending plans ...`
   every 2 min, naming the stuck plans.
2. Unblock; the queue drains; the stall lines stop.

- [ ] pass
- [ ] fail

## Test 3 — the run-1 teardown anomaly (unchanged, now unprotected)

Background: during a run-1 e2e teardown (server-side `rm` only), the client
journaled rmdirs for a tree it had removed itself — watcher events for a
self-inflicted local removal were treated as user gestures. The clean repro
logs `dropped ... (gone remotely)` instead; the misbehaving path is
unidentified. The `suppress()` drop-on-return window (adapter.rs:54-70) is
still the suspect. **There is no breaker to catch this anymore: if it fires,
the phantom deletes will replay to the server.**

1. Snapshot the tree first. Use an expendable subtree only.
2. Recreate: 100+ file tree, Explorer views open on it, parallel write load in
   a sibling dir, then delete the tree server-side and let the client mirror.
3. Watch the journal live for 'r'/'d' rows naming that tree. Also grep the log
   after: any `journaled rmdir` for a tree you deleted server-side = anomaly.
   (Damage is self-limiting here — the tree is already gone server-side, so
   replayed rms hit missing paths — but confirm nothing OUTSIDE the tree
   appears in the journal.)
4. Repeat several times; it is load-dependent.

- [ ] does not reproduce (attempts: __)
- [ ] reproduces → journal rows + surrounding log; identify which path removed
      the local files; report, do not fix

## Test 4 — full e2e suite

`scripts/windows-e2e.ps1`. The `conflict-keeps-both` fix (hostname-tagged
conflict copy names) is still unverified end-to-end. Keep the Explorer views
the harness opens (F2: without a view, an emptied dir serves a stale listing).

- [ ] suite passes
- [ ] failures per test; setup dying on "dir never appeared" is F2 (known)

## Known open items — do not file as new bugs

- **F2**: emptied dir caches an empty listing until an Explorer view drives a
  refresh; restart doesn't re-list. Design question, pending.
- **F6**: vacuum survivors (dirty/pinned) re-adopt + re-upload on next login,
  which can resurrect a server-side deletion. Ruling pending.
- **`suppress()` race window**: still open by decision; the architectural
  rework (not this round) is the fix. Test 3 is about evidence, not fixing.

## Server state (as left by run 3)

Baseline: 1748 files / 141 dirs / 100,038,764 bytes (verify against your own
fresh snapshot — run 3 was mid-flight when pulled back). `/test/e2e` remains
from the harness. Clean up any `/test/totest/**` or `/test/rogue/**` leftovers
you find before measuring.

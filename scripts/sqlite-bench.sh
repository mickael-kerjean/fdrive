#!/bin/sh
# Runs the same sqlite workload on the fdrive mount and on the local fs,
# and prints the per-phase penalty. Usage: sqlite-bench.sh [REMOTE_DIR] [LOCAL_DIR]
set -e
REMOTE="${1:-/tmp/mnt/SystemData}"
LOCAL="${2:-/tmp}"
exec python3 - "$REMOTE" "$LOCAL" << 'EOF'
import os
import random
import sqlite3
import sys
import time

PAYLOAD = random.Random(1).randbytes(1024)

def phase(results, name, work):
    t0 = time.perf_counter()
    out = work()
    results[name] = time.perf_counter() - t0
    return out

def bench(dir):
    path = os.path.join(dir, "fdrive-bench.db")
    for suffix in ("", "-journal", "-wal", "-shm"):
        try:
            os.remove(path + suffix)
        except FileNotFoundError:
            pass
    results = {}
    rnd = random.Random(42)

    db = phase(results, "open + create table", lambda: sqlite3.connect(path))
    db.execute("CREATE TABLE t(id INTEGER PRIMARY KEY, blob BLOB, n INTEGER)")
    db.commit()

    def bulk():
        with db:
            db.executemany(
                "INSERT INTO t(blob, n) VALUES (?, ?)",
                [(PAYLOAD, i) for i in range(500)],
            )
    phase(results, "bulk insert (500 rows, 1 txn)", bulk)

    def tiny_txns():
        for i in range(50):
            with db:
                db.execute("INSERT INTO t(blob, n) VALUES (?, ?)", (PAYLOAD, i))
    phase(results, "50 tiny txns (journal dance)", tiny_txns)

    def reads():
        db.execute("SELECT count(*), sum(n) FROM t").fetchone()
        for _ in range(200):
            db.execute(
                "SELECT n FROM t WHERE id = ?", (rnd.randint(1, 500),)
            ).fetchone()
    phase(results, "scan + 200 point reads", reads)

    def updates():
        for _ in range(20):
            with db:
                for _ in range(10):
                    db.execute(
                        "UPDATE t SET n = n + 1 WHERE id = ?",
                        (rnd.randint(1, 500),),
                    )
    phase(results, "200 updates (20 txns)", updates)

    phase(results, "vacuum", lambda: db.execute("VACUUM"))

    def wal():
        try:
            mode = db.execute("PRAGMA journal_mode=WAL").fetchone()[0]
            if mode != "wal":
                return f"refused ({mode})"
            with db:
                db.execute("INSERT INTO t(blob, n) VALUES (?, ?)", (PAYLOAD, 0))
            db.execute("PRAGMA journal_mode=DELETE")
            return "ok"
        except Exception as err:
            return f"failed ({err})"
    results["wal"] = phase(results, "wal round-trip", wal)

    ok = db.execute("PRAGMA integrity_check").fetchone()[0]
    size = os.path.getsize(path)
    db.close()
    for suffix in ("", "-journal", "-wal", "-shm"):
        try:
            os.remove(path + suffix)
        except FileNotFoundError:
            pass
    return results, ok, size

remote_dir, local_dir = sys.argv[1], sys.argv[2]
print(f"local:  {local_dir}")
local, local_ok, size = bench(local_dir)
print(f"remote: {remote_dir}")
remote, remote_ok, _ = bench(remote_dir)

print(f"\ndb size: {size / 1024:.0f} KiB")
print(f"integrity: local={local_ok} remote={remote_ok}")
print(f"wal mode:  local={local['wal']} remote={remote['wal']}\n")
print(f"{'phase':<32} {'local':>10} {'remote':>10} {'penalty':>9}")
for name in local:
    if name == "wal":
        continue
    l, r = local[name], remote[name]
    print(f"{name:<32} {l * 1000:>8.1f}ms {r * 1000:>8.1f}ms {r / l:>8.1f}x")
EOF

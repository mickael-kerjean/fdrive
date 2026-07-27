// Runs the same sqlite workload on an fdrive mount and on the local fs, and
// prints the per-phase penalty. Defaults to a FakeServer-backed rig mount;
// override with: cargo bench --bench sqlite -- [REMOTE_DIR] [LOCAL_DIR]
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../tests/e2e/rig.rs"]
mod rig;

#[cfg(target_os = "linux")]
mod bench {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use rusqlite::{params, Connection};

    use crate::rig::Rig;

    struct Prng(u64);

    impl Prng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn id(&mut self) -> i64 {
            (self.next() % 500 + 1) as i64
        }
    }

    struct Run {
        phases: Vec<(&'static str, Duration)>,
        wal: String,
        integrity: String,
        size: u64,
    }

    fn phase<T>(
        phases: &mut Vec<(&'static str, Duration)>,
        name: &'static str,
        work: impl FnOnce() -> T,
    ) -> T {
        let t0 = Instant::now();
        let out = work();
        phases.push((name, t0.elapsed()));
        out
    }

    fn clean(path: &Path) {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    fn bench(dir: &Path) -> Run {
        let path = dir.join("fdrive-bench.db");
        clean(&path);
        let payload: Vec<u8> = {
            let mut rnd = Prng(1);
            (0..1024).map(|_| rnd.next() as u8).collect()
        };
        let mut rnd = Prng(42);
        let mut phases = Vec::new();

        let mut db = phase(&mut phases, "open + create table", || {
            Connection::open(&path).unwrap()
        });
        db.execute("CREATE TABLE t(id INTEGER PRIMARY KEY, blob BLOB, n INTEGER)", [])
            .unwrap();

        phase(&mut phases, "bulk insert (500 rows, 1 txn)", || {
            let tx = db.transaction().unwrap();
            {
                let mut stmt = tx
                    .prepare("INSERT INTO t(blob, n) VALUES (?1, ?2)")
                    .unwrap();
                for i in 0..500 {
                    stmt.execute(params![payload, i]).unwrap();
                }
            }
            tx.commit().unwrap();
        });

        phase(&mut phases, "50 tiny txns (journal dance)", || {
            for i in 0..50 {
                let tx = db.transaction().unwrap();
                tx.execute("INSERT INTO t(blob, n) VALUES (?1, ?2)", params![payload, i])
                    .unwrap();
                tx.commit().unwrap();
            }
        });

        phase(&mut phases, "scan + 200 point reads", || {
            db.query_row("SELECT count(*), sum(n) FROM t", [], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .unwrap();
            for _ in 0..200 {
                db.query_row("SELECT n FROM t WHERE id = ?1", [rnd.id()], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap();
            }
        });

        phase(&mut phases, "200 updates (20 txns)", || {
            for _ in 0..20 {
                let tx = db.transaction().unwrap();
                for _ in 0..10 {
                    tx.execute("UPDATE t SET n = n + 1 WHERE id = ?1", [rnd.id()])
                        .unwrap();
                }
                tx.commit().unwrap();
            }
        });

        phase(&mut phases, "vacuum", || db.execute("VACUUM", []).unwrap());

        let wal = phase(&mut phases, "wal round-trip", || {
            match db.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)) {
                Ok(mode) if mode == "wal" => {
                    let out = db
                        .execute("INSERT INTO t(blob, n) VALUES (?1, 0)", params![payload])
                        .and_then(|_| {
                            db.query_row("PRAGMA journal_mode=DELETE", [], |r| {
                                r.get::<_, String>(0)
                            })
                        });
                    match out {
                        Ok(_) => "ok".to_string(),
                        Err(err) => format!("failed ({err})"),
                    }
                }
                Ok(mode) => format!("refused ({mode})"),
                Err(err) => format!("failed ({err})"),
            }
        });

        let integrity: String = db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        drop(db);
        clean(&path);
        Run {
            phases,
            wal,
            integrity,
            size,
        }
    }

    pub fn main() {
        let args: Vec<String> = std::env::args()
            .skip(1)
            .filter(|a| !a.starts_with('-'))
            .collect();
        let local = args.get(1).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let (remote, _rig) = match args.first() {
            Some(dir) => (PathBuf::from(dir), None),
            None => {
                let Some(rig) = Rig::start() else { return };
                (rig.mnt.clone(), Some(rig))
            }
        };

        println!("local:  {}", local.display());
        let l = bench(&local);
        println!("remote: {}", remote.display());
        let r = bench(&remote);

        println!("\ndb size: {} KiB", l.size / 1024);
        println!("integrity: local={} remote={}", l.integrity, r.integrity);
        println!("wal mode:  local={} remote={}\n", l.wal, r.wal);
        println!("{:<32} {:>10} {:>10} {:>9}", "phase", "local", "remote", "penalty");
        for ((name, lt), (_, rt)) in l.phases.iter().zip(&r.phases) {
            let (lt, rt) = (lt.as_secs_f64(), rt.as_secs_f64());
            println!(
                "{:<32} {:>8.1}ms {:>8.1}ms {:>8.1}x",
                name,
                lt * 1000.0,
                rt * 1000.0,
                rt / lt
            );
        }
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    bench::main();
}

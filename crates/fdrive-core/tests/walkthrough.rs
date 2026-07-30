use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fdrive_core::engine::{Engine, Resolution, UploadStatus};
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::Sdk;
use fdrive_core::testkit::FakeServer;

struct Platform {
    root: PathBuf,
    own: bool,
}

impl Platform {
    fn fresh() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "fdrive-walkthrough-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root, own: true }
    }

    fn reopen(&self) -> Self {
        Self {
            root: self.root.clone(),
            own: false,
        }
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        if self.own {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

impl LocalStore for Platform {
    fn backing(&self, path: &RelPath) -> PathBuf {
        self.root.join(path.as_str())
    }

    fn relocate(&self, from: &RelPath, to: &RelPath) -> std::io::Result<()> {
        fs::rename(self.backing(from), self.backing(to))
    }

    fn settled(&self, _target: &RelPath, _mtime: Option<SystemTime>) {}

    fn device(&self) -> String {
        "testkit".into()
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("fdrive.db")
    }
}

fn connect(server: &FakeServer, platform: Platform) -> Arc<Engine<Platform>> {
    let mut sdk = Sdk::new(server.url()).unwrap();
    sdk.set_token("TOKEN".into());
    Engine::start(Arc::new(sdk), tokio::runtime::Handle::current(), platform)
}

fn create(engine: &Engine<Platform>, path: &str, bytes: &[u8]) -> RelPath {
    let path = RelPath::new(path);
    fs::write(engine.local().backing(&path), bytes).unwrap();
    engine.created(&path);
    engine.modified(&path);
    path
}

fn edit(engine: &Engine<Platform>, path: &RelPath, bytes: &[u8]) {
    fs::write(engine.local().backing(path), bytes).unwrap();
    engine.modified(path);
}

async fn settle(engine: &Engine<Platform>) {
    engine.flush(Duration::from_secs(10)).await;
}

fn bytes(len: usize) -> Vec<u8> {
    let mut x: u32 = 42;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8
        })
        .collect()
}

#[tokio::test]
async fn a_file_created_locally_reaches_the_server() {
    let server = FakeServer::start();
    let engine = connect(&server, Platform::fresh());

    create(&engine, "hello.txt", b"hello world");
    settle(&engine).await;

    assert_eq!(server.get("/hello.txt").unwrap(), b"hello world");
    assert_eq!(*engine.upload_status().borrow(), UploadStatus::Idle);
}

#[tokio::test]
async fn an_edit_ships_as_a_delta_not_a_reupload() {
    let server = FakeServer::start();
    let engine = connect(&server, Platform::fresh());
    let path = create(&engine, "big.bin", &bytes(128 * 1024));
    settle(&engine).await;

    let mut edited = bytes(128 * 1024);
    for b in &mut edited[64 * 1024..68 * 1024] {
        *b ^= 0xAA;
    }
    edit(&engine, &path, &edited);
    settle(&engine).await;

    assert_eq!(server.get("/big.bin").unwrap(), edited);
    assert!(
        server.log().iter().any(|l| l == "delta /big.bin"),
        "the second upload went over the wire as an rdiff delta: {:?}",
        server.log()
    );
}

#[tokio::test]
async fn a_rename_travels_as_a_verb_not_as_bytes() {
    let server = FakeServer::start();
    let engine = connect(&server, Platform::fresh());
    let from = create(&engine, "draft.txt", b"content");
    settle(&engine).await;

    let to = RelPath::new("final.txt");
    engine.local().relocate(&from, &to).unwrap();
    engine.rename(&from, &to, false).await.unwrap();
    settle(&engine).await;

    assert_eq!(server.names("/"), ["final.txt"]);
    assert!(server
        .log()
        .contains(&"mv /draft.txt /final.txt".to_string()));
    let saves = server
        .log()
        .iter()
        .filter(|l| l.starts_with("save "))
        .count();
    assert_eq!(saves, 1, "the bytes were uploaded once, then only moved");
}

#[tokio::test]
async fn a_delete_reaches_the_server() {
    let server = FakeServer::start();
    let engine = connect(&server, Platform::fresh());
    let path = create(&engine, "old.txt", b"bye");
    settle(&engine).await;

    fs::remove_file(engine.local().backing(&path)).unwrap();
    engine.delete(&path, false).await.unwrap();
    settle(&engine).await;

    assert_eq!(server.get("/old.txt"), None);
}

#[tokio::test]
async fn offline_edits_land_when_the_server_returns() {
    let server = FakeServer::start();
    let engine = connect(&server, Platform::fresh());
    let path = create(&engine, "notes.txt", b"v1");
    settle(&engine).await;

    server.offline(true);
    edit(&engine, &path, b"v2");
    engine.flush(Duration::from_secs(1)).await;
    assert_eq!(
        server.get("/notes.txt").unwrap(),
        b"v1",
        "nothing lands while offline"
    );

    server.offline(false);
    settle(&engine).await;
    assert_eq!(server.get("/notes.txt").unwrap(), b"v2");
    assert_eq!(*engine.upload_status().borrow(), UploadStatus::Idle);
}

#[tokio::test]
async fn a_restart_replays_what_was_never_acknowledged() {
    let server = FakeServer::start();
    server.offline(true);
    let platform = Platform::fresh();
    let engine = connect(&server, platform.reopen());
    create(&engine, "draft.txt", b"unsent");
    engine.flush(Duration::from_secs(1)).await;
    drop(engine);

    server.offline(false);
    let engine = connect(&server, platform.reopen());
    engine.recover();
    settle(&engine).await;

    assert_eq!(server.get("/draft.txt").unwrap(), b"unsent");
}

#[tokio::test]
async fn a_restart_finishes_a_dir_delete() {
    let server = FakeServer::start();
    let platform = Platform::fresh();
    let engine = connect(&server, platform.reopen());
    fs::create_dir_all(engine.local().backing(&RelPath::new("d"))).unwrap();
    create(&engine, "d/f", b"x");
    settle(&engine).await;
    assert!(server.get("/d/f").is_some());

    server.offline(true);
    engine.delete(&RelPath::new("d"), true).await.unwrap();
    engine.flush(Duration::from_secs(1)).await;
    drop(engine);

    server.offline(false);
    let engine = connect(&server, platform.reopen());
    engine.recover();
    settle(&engine).await;

    assert_eq!(server.get("/d/f"), None);
    assert!(server.names("/").is_empty());
}

#[tokio::test]
async fn simultaneous_edits_conflict_and_resolve() {
    let server = FakeServer::start();
    let engine = connect(&server, Platform::fresh());
    let path = create(&engine, "report.txt", b"v1");
    settle(&engine).await;

    server.put_at(
        "/report.txt",
        b"theirs",
        SystemTime::now() + Duration::from_secs(2),
    );
    edit(&engine, &path, b"ours");
    settle(&engine).await;

    assert_eq!(server.get("/report.txt").unwrap(), b"theirs");
    assert_eq!(
        server.names("/"),
        ["report (conflicted copy from testkit).txt", "report.txt"]
    );
    let conflict = &engine.conflicts()[0];
    assert_eq!(
        conflict.ours.as_ref().unwrap().as_str(),
        "report (conflicted copy from testkit).txt"
    );

    engine.resolve(conflict.seq, Resolution::Ours).unwrap();
    settle(&engine).await;

    assert_eq!(server.get("/report.txt").unwrap(), b"ours");
    assert_eq!(server.names("/"), ["report.txt"]);
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn a_server_file_hydrates_locally_on_demand() {
    let server = FakeServer::start();
    server.put("/photo.jpg", &bytes(32 * 1024));
    let engine = connect(&server, Platform::fresh());

    let path = RelPath::new("photo.jpg");
    engine.hydrate(&path, None).await.unwrap();

    assert_eq!(
        fs::read(engine.local().backing(&path)).unwrap(),
        bytes(32 * 1024)
    );
}

// fixture: librsync.Signature(bytes(1<<20), out, 4096, 16, MD4_SIG_MAGIC) on the real server
#[test]
fn the_real_servers_signature_is_consumable_by_this_client() {
    let sig = fast_rsync::Signature::deserialize(
        include_bytes!("fixtures/server_signature.bin").to_vec(),
    )
    .expect("the server signature must parse");

    let server = bytes(1 << 20);
    let mut local = server.clone();
    for b in &mut local[300 * 1024..304 * 1024] {
        *b ^= 0xAA;
    }

    let mut delta = Vec::new();
    fast_rsync::diff(&sig.index(), &local, &mut delta).expect("diff against the server signature");
    assert!(
        delta.len() < 32 * 1024,
        "4KiB edit produced a {} bytes delta, cross-implementation block matching is broken",
        delta.len()
    );

    let mut restored = Vec::new();
    fast_rsync::apply(&server, &delta, &mut restored).expect("apply");
    assert_eq!(restored, local);
}

fn xorshift(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn server_files(server: &FakeServer) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        for name in server.names(&format!("/{dir}")) {
            let full = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            match server.get(&format!("/{full}")) {
                Some(bytes) => {
                    out.insert(full, bytes);
                }
                None => dirs.push(full),
            }
        }
    }
    out
}

struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        eprintln!("[{}] {}", record.level(), record.args());
    }
    fn flush(&self) {}
}

#[tokio::test]
async fn random_workloads_converge() {
    let _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Info));
    const PATHS: [&str; 6] = ["a.txt", "b.txt", "e.txt", "d/x.txt", "d/y.txt", "d/z.txt"];
    for round in 0..24u64 {
        let mut seed = 0x853c49e6748fea9b ^ (round + 1);
        let server = FakeServer::start();
        let platform = Platform::fresh();
        fs::create_dir_all(platform.root.join("d")).unwrap();
        let mut engine = connect(&server, platform.reopen());
        let mut expected: std::collections::BTreeMap<String, Vec<u8>> = Default::default();
        let mut trace: Vec<String> = Vec::new();

        for phase in 0..4 {
            let offline = phase > 0 && xorshift(&mut seed) % 2 == 0;
            if offline {
                trace.push(format!("-- phase {phase} offline"));
                server.offline(true);
            } else {
                trace.push(format!("-- phase {phase} online"));
            }
            for turn in 0..10 {
                let at = (xorshift(&mut seed) % 6) as usize;
                let path = PATHS[at];
                let rel = RelPath::new(path);
                let on_disk = engine.local().backing(&rel).is_file();
                let gesture = if offline {
                    xorshift(&mut seed) % 2
                } else {
                    xorshift(&mut seed) % 6
                };
                match gesture {
                    0 | 1 => {
                        let content = format!("{round}-{phase}-{turn}-{seed:x}").into_bytes();
                        if on_disk {
                            edit(&engine, &rel, &content);
                        } else {
                            create(&engine, path, &content);
                        }
                        trace.push(format!(
                            "write {path} = {}",
                            String::from_utf8_lossy(&content)
                        ));
                        expected.insert(path.to_string(), content);
                    }
                    2 if on_disk => {
                        fs::remove_file(engine.local().backing(&rel)).unwrap();
                        engine.delete(&rel, false).await.unwrap();
                        trace.push(format!("rm {path}"));
                        expected.remove(path);
                    }
                    3 if on_disk => {
                        let to = PATHS[(at + 1 + (xorshift(&mut seed) % 5) as usize) % 6];
                        let to_rel = RelPath::new(to);
                        fs::rename(
                            engine.local().backing(&rel),
                            engine.local().backing(&to_rel),
                        )
                        .unwrap();
                        engine.rename(&rel, &to_rel, false).await.unwrap();
                        trace.push(format!("mv {path} -> {to}"));
                        let content = expected.remove(path);
                        match content {
                            Some(content) => {
                                expected.insert(to.to_string(), content);
                            }
                            None => {
                                expected.remove(to);
                            }
                        }
                    }
                    4 => {
                        let dir = RelPath::new("d");
                        let _ = fs::remove_dir_all(engine.local().backing(&dir));
                        fs::create_dir_all(engine.local().backing(&dir)).unwrap();
                        engine.delete(&dir, true).await.unwrap();
                        trace.push("rmdir d".to_string());
                        expected.retain(|k, _| !k.starts_with("d/"));
                    }
                    _ => {}
                }
            }
            if offline {
                engine.flush(Duration::from_millis(100)).await;
                drop(engine);
                server.offline(false);
                engine = connect(&server, platform.reopen());
                engine.recover();
            }
            settle(&engine).await;
        }

        let actual = server_files(&server);
        assert_eq!(
            actual,
            expected,
            "server diverged from the model (round {round})\ntrace:\n{}\nserver log:\n{}",
            trace.join("\n"),
            server.log().join("\n")
        );
    }
}

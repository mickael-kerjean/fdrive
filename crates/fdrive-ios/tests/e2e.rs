use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fdrive_ios::Adapter;
use fdrive_testkit::FakeServer;

struct Rig {
    server: FakeServer,
    adapter: Arc<Adapter>,
    data: PathBuf,
}

fn rig() -> Rig {
    rig_with(FakeServer::start())
}

fn rig_with(server: FakeServer) -> Rig {
    static N: AtomicU32 = AtomicU32::new(0);
    let data = std::env::temp_dir().join(format!(
        "fdrive-ios-e2e-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&data).unwrap();
    let adapter = Adapter::new(
        server.url().to_string(),
        false,
        "TOKEN".to_string(),
        data.to_string_lossy().into_owned(),
    )
    .unwrap();
    Rig {
        server,
        adapter,
        data,
    }
}

impl Rig {
    fn settle(&self) {
        std::thread::sleep(Duration::from_millis(300));
        self.adapter.flush(10_000);
    }

    fn restart(&mut self) {
        let server = self.server.clone();
        let adapter = Adapter::new(
            server.url().to_string(),
            false,
            "TOKEN".to_string(),
            self.data.to_string_lossy().into_owned(),
        )
        .unwrap();
        self.adapter = adapter;
    }

    fn save(&self, path: &str, content: &[u8]) {
        let local = self.adapter.create(path.to_string()).unwrap();
        fs::write(local, content).unwrap();
        self.adapter.saved(path.to_string());
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.data);
    }
}

#[test]
#[ignore = "e2e"]
fn a_created_file_lands_on_the_server() {
    let rig = rig();
    rig.save("/doc.txt", b"hello from ios");
    rig.settle();

    assert_eq!(rig.server.get("/doc.txt").unwrap(), b"hello from ios");
    assert_eq!(rig.server.log(), vec!["save /doc.txt"]);
}

#[test]
#[ignore = "e2e"]
fn a_remote_file_opens_with_its_content() {
    let rig = rig();
    rig.server.put("/report.pdf", b"quarterly numbers");

    let local = rig.adapter.open("/report.pdf".to_string()).unwrap();
    assert_eq!(fs::read(local).unwrap(), b"quarterly numbers");
    assert!(rig.server.log().is_empty(), "opening mutates nothing");
}

#[test]
#[ignore = "e2e"]
fn a_remote_change_reaches_a_cached_file() {
    let rig = rig();
    rig.server.put_at(
        "/shared.txt",
        b"version one",
        SystemTime::now() - Duration::from_secs(60),
    );
    let local = rig.adapter.open("/shared.txt".to_string()).unwrap();
    assert_eq!(fs::read(&local).unwrap(), b"version one");

    rig.server.put("/shared.txt", b"version two - longer");
    std::thread::sleep(Duration::from_millis(2500));
    let local = rig.adapter.open("/shared.txt".to_string()).unwrap();
    assert_eq!(fs::read(&local).unwrap(), b"version two - longer");
}

#[test]
#[ignore = "e2e"]
fn a_second_edit_travels_as_a_delta() {
    let rig = rig();
    let mut content = vec![b'a'; 16 * 1024];
    rig.save("/big.txt", &content);
    rig.settle();

    content[0] = b'b';
    let local = rig.adapter.open("/big.txt".to_string()).unwrap();
    fs::write(local, &content).unwrap();
    rig.adapter.saved("/big.txt".to_string());
    rig.settle();

    assert_eq!(rig.server.get("/big.txt").unwrap(), content);
    assert_eq!(rig.server.log(), vec!["save /big.txt", "delta /big.txt"]);
}

#[test]
#[ignore = "e2e"]
fn offline_edits_land_when_the_server_recovers() {
    let rig = rig();
    rig.server.offline(true);
    rig.save("/notes.txt", b"written on the plane");
    std::thread::sleep(Duration::from_millis(300));
    rig.adapter.flush(1_000);
    assert!(rig.server.log().is_empty());

    rig.server.offline(false);
    rig.settle();
    assert_eq!(
        rig.server.get("/notes.txt").unwrap(),
        b"written on the plane"
    );
}

#[test]
#[ignore = "e2e"]
fn a_restart_replays_pending_work_exactly_once() {
    let mut rig = rig();
    rig.server.offline(true);
    rig.save("/pending.txt", b"queued before the crash");
    std::thread::sleep(Duration::from_millis(300));
    rig.adapter.flush(1_000);

    rig.restart();
    rig.server.offline(false);
    rig.settle();

    assert_eq!(
        rig.server.get("/pending.txt").unwrap(),
        b"queued before the crash"
    );
    assert_eq!(rig.server.log(), vec!["save /pending.txt"]);
}

#[test]
#[ignore = "e2e"]
fn a_delete_of_a_listed_file_propagates() {
    let rig = rig();
    rig.server.put("/doomed.txt", b"bytes");
    rig.adapter.ls("/".to_string()).unwrap();

    rig.adapter.delete("/doomed.txt".to_string()).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/doomed.txt"), None);
    assert_eq!(rig.server.log(), vec!["rm /doomed.txt"]);
}

#[test]
#[ignore = "e2e"]
fn a_known_dir_still_lists_offline() {
    let rig = rig();
    rig.server.put("/keep/manual.pdf", b"the whole manual");
    let local = rig.adapter.open("/keep/manual.pdf".to_string()).unwrap();
    assert_eq!(fs::read(&local).unwrap(), b"the whole manual");

    rig.server.offline(true);
    std::thread::sleep(Duration::from_millis(2500));
    let listing = rig.adapter.ls("/keep".to_string()).unwrap();
    assert_eq!(listing.len(), 1, "the ledger answers when the server cannot");
    assert_eq!(listing[0].name, "manual.pdf");
    let local = rig.adapter.open("/keep/manual.pdf".to_string()).unwrap();
    assert_eq!(fs::read(&local).unwrap(), b"the whole manual");
}

#[test]
#[ignore = "e2e"]
fn a_wiped_client_relearns_and_never_mutates() {
    let server = FakeServer::start();
    server.put("/keep/precious.txt", b"the only copy");
    let mut rig = rig_with(server);
    let local = rig.adapter.open("/keep/precious.txt".to_string()).unwrap();
    assert_eq!(fs::read(local).unwrap(), b"the only copy");

    fs::remove_dir_all(&rig.data).unwrap();
    fs::create_dir_all(&rig.data).unwrap();
    rig.restart();
    let local = rig.adapter.open("/keep/precious.txt".to_string()).unwrap();
    assert_eq!(fs::read(local).unwrap(), b"the only copy");
    rig.settle();
    assert!(
        rig.server.log().is_empty(),
        "a local wipe never amplifies into remote mutations: {:?}",
        rig.server.log()
    );
}

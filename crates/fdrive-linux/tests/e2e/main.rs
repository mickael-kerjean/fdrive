#![cfg(target_os = "linux")]

mod rig;

use std::fs;
use std::os::unix::fs::FileExt;
use std::time::{Duration, SystemTime};

use fdrive_testkit::FakeServer;
use rig::Rig;

#[test]
#[ignore = "e2e"]
fn a_vim_dance_lands_as_a_single_save() {
    let Some(rig) = Rig::start() else { return };
    fs::write(rig.path("doc.txt"), b"v1").unwrap();
    rig.settle();
    assert_eq!(rig.server.get("/doc.txt").unwrap(), b"v1");

    fs::write(rig.path(".doc.txt.swp"), b"v2 from the editor").unwrap();
    fs::rename(rig.path(".doc.txt.swp"), rig.path("doc.txt")).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/doc.txt").unwrap(), b"v2 from the editor");
    let log = rig.server.log();
    assert_eq!(
        log,
        vec!["save /doc.txt", "save /doc.txt"],
        "two plain saves, the dance never reached the server"
    );
}

#[test]
#[ignore = "e2e"]
fn offline_edits_land_when_the_server_recovers() {
    let Some(rig) = Rig::start() else { return };
    rig.server.offline(true);
    fs::write(rig.path("notes.txt"), b"written on the plane").unwrap();
    rig.settle_for(Duration::from_secs(2));
    assert!(rig.server.log().is_empty(), "nothing reached the server");

    rig.server.offline(false);
    rig.settle();
    assert_eq!(
        rig.server.get("/notes.txt").unwrap(),
        b"written on the plane"
    );
    assert_eq!(rig.server.log(), vec!["save /notes.txt"]);
}

#[test]
#[ignore = "e2e"]
fn a_stale_lease_becomes_a_conflicted_copy_never_an_overwrite() {
    let Some(rig) = Rig::start() else { return };
    rig.server.put_at(
        "/report.txt",
        b"base",
        SystemTime::now() - Duration::from_secs(60),
    );
    assert_eq!(fs::read(rig.path("report.txt")).unwrap(), b"base");

    rig.server.put("/report.txt", b"theirs");
    fs::write(rig.path("report.txt"), b"ours").unwrap();
    rig.settle();

    assert_eq!(
        rig.server.get("/report.txt").unwrap(),
        b"theirs",
        "their version keeps the name"
    );
    let names = rig.server.names("/");
    assert!(
        names.iter().any(|n| n.contains("conflicted copy")),
        "ours landed as a copy: {names:?}"
    );
    assert!(
        !rig.server.log().iter().any(|l| l.starts_with("rm")),
        "a conflict never deletes anything"
    );
}

#[test]
#[ignore = "e2e"]
fn a_remote_change_reaches_a_cached_file() {
    let Some(rig) = Rig::start() else { return };
    rig.server.put_at(
        "/shared.txt",
        b"version one",
        SystemTime::now() - Duration::from_secs(60),
    );
    assert_eq!(fs::read(rig.path("shared.txt")).unwrap(), b"version one");

    rig.server.put("/shared.txt", b"version two - longer");
    std::thread::sleep(Duration::from_millis(6500));
    assert_eq!(
        fs::read(rig.path("shared.txt")).unwrap(),
        b"version two - longer",
        "no cache layer may serve stale bytes or a stale length"
    );
}

#[test]
#[ignore = "e2e"]
fn a_write_during_hydration_uploads_the_whole_file() {
    let Some(rig) = Rig::start() else { return };
    let original = vec![b'x'; 2 * 1024 * 1024];
    rig.server.put_at(
        "/big.bin",
        &original,
        SystemTime::now() - Duration::from_secs(60),
    );
    fs::read_dir(&rig.mnt).unwrap().count();
    rig.server.throttle(Some(Duration::from_millis(5)));

    let file = fs::OpenOptions::new()
        .write(true)
        .open(rig.path("big.bin"))
        .unwrap();
    file.write_all_at(b"PATCH", (original.len() - 5) as u64)
        .unwrap();
    drop(file);
    rig.server.throttle(None);
    rig.settle();

    let mut expected = original;
    let at = expected.len() - 5;
    expected[at..].copy_from_slice(b"PATCH");
    let got = rig.server.get("/big.bin").unwrap();
    assert_eq!(
        got.len(),
        expected.len(),
        "size survived; log: {:?}",
        rig.server.log()
    );
    assert_eq!(
        &got[at..],
        b"PATCH",
        "the upload holds the patch; tail was {:?}, log: {:?}",
        &got[at..],
        rig.server.log()
    );
    assert_eq!(got, expected, "the rest is the original");
}

#[test]
#[ignore = "e2e"]
fn a_restart_replays_pending_work_exactly_once() {
    let Some(mut rig) = Rig::start() else { return };
    rig.server.offline(true);
    fs::write(rig.path("pending.txt"), b"queued before the crash").unwrap();
    rig.settle_for(Duration::from_secs(2));

    rig.restart();
    rig.server.offline(false);
    rig.settle();

    assert_eq!(
        rig.server.get("/pending.txt").unwrap(),
        b"queued before the crash"
    );
    assert_eq!(
        rig.server.log(),
        vec!["save /pending.txt"],
        "the journal survived the restart and replayed exactly once"
    );
}

#[test]
#[ignore = "e2e"]
fn rm_rf_of_a_listed_tree_cleans_the_server() {
    let Some(rig) = Rig::start() else { return };
    rig.server.put("/project/readme.md", b"one");
    rig.server.put("/project/src/main.rs", b"two");
    fs::read_dir(rig.path("project")).unwrap().count();
    fs::read_dir(rig.path("project/src")).unwrap().count();

    fs::remove_dir_all(rig.path("project")).unwrap();
    rig.settle();

    assert_eq!(rig.server.names("/"), Vec::<String>::new());
    assert!(
        rig.server.log().iter().all(|l| l.starts_with("rm")),
        "a delete storm is only ever deletes: {:?}",
        rig.server.log()
    );
}

#[test]
#[ignore = "e2e"]
fn an_offline_dir_rename_fails_fast_and_leaves_no_debt() {
    let Some(rig) = Rig::start() else { return };
    fs::create_dir(rig.path("folder")).unwrap();
    rig.settle();
    assert_eq!(rig.server.log(), vec!["mkdir /folder"]);

    rig.server.offline(true);
    let refused = fs::rename(rig.path("folder"), rig.path("renamed"));
    assert!(
        refused.is_err(),
        "dir ops are synchronous, offline fails loudly"
    );

    rig.server.offline(false);
    rig.settle();
    assert_eq!(
        rig.server.log(),
        vec!["mkdir /folder"],
        "no mv was queued for later"
    );
}

#[test]
#[ignore = "e2e"]
fn a_wiped_client_relearns_and_never_mutates() {
    let server = FakeServer::start();
    server.put("/keep/precious.txt", b"the only copy");
    let Some(mut rig) = Rig::with(server) else {
        return;
    };
    assert_eq!(
        fs::read(rig.path("keep/precious.txt")).unwrap(),
        b"the only copy"
    );

    rig.wipe_client_state();
    assert_eq!(
        fs::read(rig.path("keep/precious.txt")).unwrap(),
        b"the only copy",
        "the client relearns everything from the server"
    );
    rig.settle();
    assert!(
        rig.server.log().is_empty(),
        "a local wipe never amplifies into remote mutations: {:?}",
        rig.server.log()
    );
}

#[test]
#[ignore = "e2e"]
fn an_old_ledger_schema_drops_pending_work_but_never_invents_any() {
    let server = FakeServer::start();
    server.put("/synced.txt", b"server copy");
    let Some(rig) = Rig::with_state(server, |data| {
        let db = rusqlite::Connection::open(data.join("fdrive.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE observations(path TEXT PRIMARY KEY, size INTEGER NOT NULL, time INTEGER NOT NULL);
             CREATE TABLE journal(seq INTEGER PRIMARY KEY, op TEXT NOT NULL, path TEXT NOT NULL, dest TEXT);
             INSERT INTO observations VALUES ('synced.txt', 11, 1000000000);
             INSERT INTO journal(op, path) VALUES ('w', 'edited-on-the-old-version.txt');
             INSERT INTO journal(op, path, dest) VALUES ('m', 'synced.txt', 'moved.txt');",
        )
        .unwrap();
        fs::create_dir_all(data.join("cache")).unwrap();
        fs::write(
            data.join("cache/edited-on-the-old-version.txt"),
            b"an edit the old version never sent",
        )
        .unwrap();
    }) else {
        return;
    };
    rig.settle();

    assert!(
        rig.server.log().is_empty(),
        "plans from a foreign schema do less, never more: {:?}",
        rig.server.log()
    );
    assert_eq!(
        fs::read(rig.path("synced.txt")).unwrap(),
        b"server copy",
        "the client still works"
    );
    fs::write(rig.path("fresh.txt"), b"life goes on").unwrap();
    rig.settle();
    assert_eq!(rig.server.get("/fresh.txt").unwrap(), b"life goes on");
}

#[test]
#[ignore = "e2e"]
fn a_future_journal_op_is_skipped_not_misread() {
    let server = FakeServer::start();
    server.put("/synced.txt", b"server copy");
    let Some(rig) = Rig::with_state(server, |data| {
        let db = rusqlite::Connection::open(data.join("fdrive.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE observations(path TEXT PRIMARY KEY, size INTEGER NOT NULL, time INTEGER NOT NULL);
             CREATE TABLE journal(seq INTEGER PRIMARY KEY, op TEXT NOT NULL, path TEXT NOT NULL, dest TEXT, base TEXT, size INTEGER, time INTEGER);
             CREATE TABLE conflicts(seq INTEGER PRIMARY KEY, op TEXT NOT NULL, path TEXT NOT NULL, dest TEXT, expected_size INTEGER, expected_time INTEGER, found_size INTEGER, found_time INTEGER, ours TEXT, at INTEGER NOT NULL);
             CREATE TABLE pins(path TEXT PRIMARY KEY);
             CREATE TABLE signatures(path TEXT PRIMARY KEY, sig BLOB NOT NULL);
             INSERT INTO journal(op, path, size, time) VALUES ('z', 'a-verb-from-the-future', 9, 9);
             INSERT INTO journal(op, path, size, time) VALUES ('r', 'never-existed.txt', 5, 5);",
        )
        .unwrap();
    }) else {
        return;
    };
    rig.settle();

    assert!(
        rig.server.log().is_empty(),
        "unknown verbs are retired, and a remove of nothing stats 404 and never calls rm: {:?}",
        rig.server.log()
    );
    assert_eq!(fs::read(rig.path("synced.txt")).unwrap(), b"server copy");
}

#[test]
#[ignore = "e2e"]
fn a_garbage_ledger_quarantines_the_cache_and_relearns() {
    let server = FakeServer::start();
    server.put("/synced.txt", b"server copy");
    let Some(rig) = Rig::with_state(server, |data| {
        fs::write(data.join("fdrive.db"), b"this is not a sqlite database").unwrap();
        fs::create_dir_all(data.join("cache")).unwrap();
        fs::write(
            data.join("cache/maybe-unsent.txt"),
            b"the only copy of an edit",
        )
        .unwrap();
    }) else {
        return;
    };
    rig.settle();

    assert!(
        rig.server.log().is_empty(),
        "an unreadable ledger disarms everything: {:?}",
        rig.server.log()
    );
    let aside = fs::read_dir(rig.data())
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("cache.unreadable-")
        })
        .expect("the cache was set aside, not deleted and not trusted");
    assert_eq!(
        fs::read(aside.path().join("maybe-unsent.txt")).unwrap(),
        b"the only copy of an edit",
        "possibly-unsent bytes survive for the user to recover"
    );
    assert_eq!(fs::read(rig.path("synced.txt")).unwrap(), b"server copy");
}

#[test]
#[ignore = "e2e"]
fn a_rename_travels_as_a_move_not_a_reupload() {
    let Some(rig) = Rig::start() else { return };
    fs::write(rig.path("a.txt"), b"payload").unwrap();
    rig.settle();

    fs::rename(rig.path("a.txt"), rig.path("b.txt")).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/b.txt").unwrap(), b"payload");
    assert_eq!(rig.server.get("/a.txt"), None);
    assert_eq!(
        rig.server.log(),
        vec!["save /a.txt", "mv /a.txt /b.txt"],
        "one save, one move, zero re-uploads"
    );
}

#[test]
#[ignore = "e2e"]
fn a_dir_rename_moves_the_whole_tree_in_one_call() {
    let Some(rig) = Rig::start() else { return };
    fs::create_dir(rig.path("photos")).unwrap();
    fs::write(rig.path("photos/cat.jpg"), b"meow").unwrap();
    rig.settle();

    fs::rename(rig.path("photos"), rig.path("archive")).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/archive/cat.jpg").unwrap(), b"meow");
    assert_eq!(
        rig.server.log(),
        vec![
            "mkdir /photos",
            "save /photos/cat.jpg",
            "mv /photos /archive"
        ],
        "the subtree moved server-side, nothing was re-sent"
    );
}

#[test]
#[ignore = "e2e"]
fn an_exiftool_dance_moves_the_original_and_saves_the_new() {
    let Some(rig) = Rig::start() else { return };
    fs::write(rig.path("photo.jpg"), b"original pixels").unwrap();
    rig.settle();

    fs::rename(rig.path("photo.jpg"), rig.path("photo.jpg_original")).unwrap();
    fs::write(rig.path("photo.jpg"), b"stripped pixels").unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/photo.jpg").unwrap(), b"stripped pixels");
    assert_eq!(
        rig.server.get("/photo.jpg_original").unwrap(),
        b"original pixels"
    );
    assert_eq!(
        rig.server.log()[1..],
        vec!["mv /photo.jpg /photo.jpg_original", "save /photo.jpg"],
        "the original's bytes travel as a move, never a re-upload, never an rm"
    );
}

#[test]
#[ignore = "e2e"]
fn an_offline_delete_lands_on_recovery() {
    let Some(rig) = Rig::start() else { return };
    fs::write(rig.path("doomed.txt"), b"bytes").unwrap();
    rig.settle();

    rig.server.offline(true);
    fs::remove_file(rig.path("doomed.txt")).unwrap();
    rig.settle_for(Duration::from_secs(2));
    rig.server.offline(false);
    rig.settle();

    assert_eq!(rig.server.get("/doomed.txt"), None);
    assert_eq!(rig.server.log(), vec!["save /doomed.txt", "rm /doomed.txt"]);
}

#[test]
#[ignore = "e2e"]
fn a_temp_file_dies_unseen() {
    let Some(rig) = Rig::start() else { return };
    fs::write(rig.path(".mutt-host-1234-5"), b"draft").unwrap();
    fs::remove_file(rig.path(".mutt-host-1234-5")).unwrap();
    rig.settle();

    assert!(
        rig.server.log().is_empty(),
        "created-then-deleted nets to nothing: {:?}",
        rig.server.log()
    );
}

#[test]
#[ignore = "e2e"]
fn unicode_names_roundtrip() {
    let Some(rig) = Rig::start() else { return };
    fs::create_dir(rig.path("réunion")).unwrap();
    fs::write(rig.path("réunion/budget €.txt"), b"12,34").unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/réunion/budget €.txt").unwrap(), b"12,34");
    rig.server
        .put("/réunion/budget €.txt", "56,78 - approuvé".as_bytes());
    std::thread::sleep(Duration::from_millis(6500));
    assert_eq!(
        fs::read(rig.path("réunion/budget €.txt")).unwrap(),
        "56,78 - approuvé".as_bytes()
    );
}

#[test]
#[ignore = "e2e"]
fn a_second_edit_travels_as_a_delta() {
    let Some(rig) = Rig::start() else { return };
    let mut content = vec![b'a'; 16 * 1024];
    fs::write(rig.path("big.txt"), &content).unwrap();
    rig.settle();
    assert_eq!(rig.server.log(), vec!["save /big.txt"]);

    content[0] = b'b';
    fs::write(rig.path("big.txt"), &content).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/big.txt").unwrap(), content);
    assert_eq!(
        rig.server.log(),
        vec!["save /big.txt", "delta /big.txt"],
        "the second edit went over the wire as a delta, not a full body"
    );
}

#[test]
#[ignore = "e2e"]
fn a_changed_file_rehydrates_as_ranges() {
    let Some(rig) = Rig::start() else { return };
    let lcg = |size: usize, seed: u32| -> Vec<u8> {
        let mut x = seed;
        (0..size)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect()
    };
    let v1 = lcg(2 << 20, 1);
    fs::write(rig.path("big.bin"), &v1).unwrap();
    rig.settle();
    assert_eq!(rig.server.get("/big.bin").unwrap(), v1);

    let mut v2 = v1.clone();
    v2[1048576..1052672].copy_from_slice(&lcg(4096, 9));
    rig.server
        .put_at("/big.bin", &v2, SystemTime::now() + Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(6500));

    let got = fs::read(rig.path("big.bin")).unwrap();
    let log = rig.server.log();
    assert!(
        log.contains(&"sig /big.bin".to_string()),
        "the rehydrate asked for the signature: {log:?}"
    );
    assert!(
        log.iter().any(|l| l.starts_with("range /big.bin ")),
        "the rehydrate fetched ranges, not the full body: {log:?}"
    );
    assert_eq!(got, v2);
}

#[test]
#[ignore = "e2e"]
fn a_pin_hydrates_without_anyone_opening() {
    let Some(rig) = Rig::start() else { return };
    rig.server.put("/keep/pinned.txt", b"keep me local");
    fs::read_dir(&rig.mnt).unwrap().count();

    pin_always(&rig.path("keep"));
    assert!(
        wait_for(&rig.data().join("cache/keep/pinned.txt"), b"keep me local"),
        "the content arrived in the cache with no open"
    );

    let dir = std::ffi::CString::new(rig.path("keep").to_str().unwrap()).unwrap();
    let name = std::ffi::CString::new("user.fdrive.pin").unwrap();
    let mut buf = [0u8; 16];
    let n = unsafe {
        libc::getxattr(
            dir.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    assert_eq!(&buf[..n as usize], b"always");
}

#[test]
#[ignore = "e2e"]
fn real_vim_saves_land_with_their_exact_bytes() {
    let Some(rig) = Rig::start() else { return };
    let have = |bin: &str| {
        std::process::Command::new("which")
            .arg(bin)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if !have("vim") || !have("script") {
        eprintln!("skipped: vim or script(1) missing");
        return;
    }
    fs::write(rig.path("essay.txt"), b"first draft").unwrap();
    rig.settle();

    let vim = format!(
        "vim -u NONE -c 'normal Goedited by real vim' -c 'wq' '{}'",
        rig.path("essay.txt").display()
    );
    let out = std::process::Command::new("script")
        .args(["-qec", &vim, "/dev/null"])
        .output()
        .unwrap();
    assert!(out.status.success(), "vim exited badly");
    rig.settle();

    assert_eq!(
        rig.server.get("/essay.txt").unwrap(),
        b"first draft\nedited by real vim\n",
        "the swap-and-rename dance of a real editor lands the exact bytes"
    );
    assert!(
        rig.server.log().iter().all(|l| l.contains("/essay.txt")),
        "no vim artifact ever reached the server: {:?}",
        rig.server.log()
    );
}

fn pin_always(path: &std::path::Path) {
    let target = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let name = std::ffi::CString::new("user.fdrive.pin").unwrap();
    let rc = unsafe {
        libc::setxattr(
            target.as_ptr(),
            name.as_ptr(),
            b"always".as_ptr().cast(),
            6,
            0,
        )
    };
    assert_eq!(
        rc,
        0,
        "setxattr failed: {}",
        std::io::Error::last_os_error()
    );
}

fn wait_for(backing: &std::path::Path, content: &[u8]) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if fs::read(backing).is_ok_and(|got| got == content) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
#[ignore = "e2e"]
fn a_pinned_file_reads_offline() {
    let Some(rig) = Rig::start() else { return };
    rig.server.put("/keep/manual.pdf", b"the whole manual");
    fs::read_dir(&rig.mnt).unwrap().count();
    pin_always(&rig.path("keep"));
    assert!(wait_for(
        &rig.data().join("cache/keep/manual.pdf"),
        b"the whole manual"
    ));

    rig.server.offline(true);
    assert_eq!(
        fs::read(rig.path("keep/manual.pdf")).unwrap(),
        b"the whole manual",
        "pinned content is the offline promise"
    );
}

#[test]
#[ignore = "e2e"]
fn a_local_edit_to_a_pinned_file_pushes() {
    let Some(rig) = Rig::start() else { return };
    rig.server.put("/keep/notes.txt", b"v1");
    fs::read_dir(&rig.mnt).unwrap().count();
    pin_always(&rig.path("keep"));
    assert!(wait_for(&rig.data().join("cache/keep/notes.txt"), b"v1"));

    fs::write(rig.path("keep/notes.txt"), b"v2 edited under the pin").unwrap();
    rig.settle();

    assert_eq!(
        rig.server.get("/keep/notes.txt").unwrap(),
        b"v2 edited under the pin"
    );
    assert_eq!(
        fs::read(rig.data().join("cache/keep/notes.txt")).unwrap(),
        b"v2 edited under the pin",
        "pinned content stays local after the push"
    );
}

#[test]
#[ignore = "e2e"]
fn a_remote_change_to_a_pinned_file_arrives_on_restart_unopened() {
    let Some(mut rig) = Rig::start() else { return };
    rig.server.put_at(
        "/keep/report.txt",
        b"monday",
        SystemTime::now() - Duration::from_secs(60),
    );
    fs::read_dir(&rig.mnt).unwrap().count();
    pin_always(&rig.path("keep"));
    assert!(wait_for(
        &rig.data().join("cache/keep/report.txt"),
        b"monday"
    ));

    rig.server.put("/keep/report.txt", b"tuesday, longer");
    rig.restart();

    assert!(
        wait_for(
            &rig.data().join("cache/keep/report.txt"),
            b"tuesday, longer"
        ),
        "the startup pin sweep pulls the new version with no open"
    );
}

#[test]
#[ignore = "e2e"]
fn delta_upload_survives_a_restart() {
    let Some(mut rig) = Rig::start() else { return };
    let mut content = vec![b'a'; 16 * 1024];
    fs::write(rig.path("big.txt"), &content).unwrap();
    rig.settle();

    rig.restart();
    content[42] = b'b';
    fs::write(rig.path("big.txt"), &content).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/big.txt").unwrap(), content);
    assert_eq!(
        rig.server.log(),
        vec!["save /big.txt", "delta /big.txt"],
        "the delta base outlives the restart and its cache prune"
    );
}

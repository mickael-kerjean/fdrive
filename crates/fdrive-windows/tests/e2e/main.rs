#![cfg(windows)]

mod rig;

use std::fs;
use std::time::{Duration, SystemTime};

use fdrive_testkit::FakeServer;
use rig::Rig;

#[test]
#[ignore = "e2e"]
fn a_local_file_lands_on_the_server() {
    let rig = Rig::start();
    fs::write(rig.path("doc.txt"), b"hello from windows").unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/doc.txt").unwrap(), b"hello from windows");
    assert_eq!(rig.server.log(), vec!["save /doc.txt"]);
}

#[test]
#[ignore = "e2e"]
fn a_remote_file_becomes_a_readable_placeholder() {
    let rig = Rig::start();
    rig.server.put("/report.pdf", b"quarterly numbers");
    rig.sweep();

    assert_eq!(
        fs::read(rig.path("report.pdf")).unwrap(),
        b"quarterly numbers",
        "the placeholder hydrates on first read"
    );
    assert!(rig.server.log().is_empty(), "reading mutates nothing");
}

#[test]
#[ignore = "e2e"]
fn a_remote_change_updates_a_hydrated_placeholder() {
    let rig = Rig::start();
    rig.server.put_at(
        "/shared.txt",
        b"version one",
        SystemTime::now() - Duration::from_secs(60),
    );
    rig.sweep();
    assert_eq!(fs::read(rig.path("shared.txt")).unwrap(), b"version one");

    rig.server.put("/shared.txt", b"version two - longer");
    rig.sweep();
    assert_eq!(
        fs::read(rig.path("shared.txt")).unwrap(),
        b"version two - longer",
        "the placeholder rebuilt instead of serving stale bytes"
    );
}

#[test]
#[ignore = "e2e"]
fn an_editor_dance_lands_the_exact_bytes() {
    let rig = Rig::start();
    fs::write(rig.path("doc.txt"), b"v1").unwrap();
    rig.settle();

    fs::write(rig.path("doc.txt.tmp"), b"v2 from the editor").unwrap();
    fs::rename(rig.path("doc.txt.tmp"), rig.path("doc.txt")).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/doc.txt").unwrap(), b"v2 from the editor");
    let log = rig.server.log();
    assert!(
        log.iter()
            .all(|l| l.ends_with("/doc.txt") && !l.starts_with("rm")),
        "the temp name never reached the server and nothing was deleted: {log:?}"
    );
}

#[test]
#[ignore = "e2e"]
fn offline_edits_land_when_the_server_recovers() {
    let rig = Rig::start();
    rig.server.offline(true);
    fs::write(rig.path("notes.txt"), b"written on the plane").unwrap();
    rig.settle_for(Duration::from_secs(2));
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
    let mut rig = Rig::start();
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
    assert_eq!(rig.server.log(), vec!["save /pending.txt"]);
}

#[test]
#[ignore = "e2e"]
fn a_second_edit_travels_as_a_delta() {
    let rig = Rig::start();
    let mut content = vec![b'a'; 16 * 1024];
    fs::write(rig.path("big.txt"), &content).unwrap();
    rig.settle();

    content[0] = b'b';
    fs::write(rig.path("big.txt"), &content).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/big.txt").unwrap(), content);
    assert_eq!(
        rig.server.log(),
        vec!["save /big.txt", "delta /big.txt"],
        "the second edit went over the wire as a delta"
    );
}

#[test]
#[ignore = "e2e"]
fn rm_rf_of_a_listed_tree_cleans_the_server() {
    let rig = Rig::start();
    rig.server.put("/project/readme.md", b"one");
    rig.server.put("/project/src/main.rs", b"two");
    rig.sweep();
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
fn a_wiped_client_relearns_and_never_mutates() {
    let server = FakeServer::start();
    server.put("/keep/precious.txt", b"the only copy");
    let mut rig = Rig::with(server);
    rig.sweep();
    assert_eq!(
        fs::read(rig.path("keep/precious.txt")).unwrap(),
        b"the only copy"
    );

    fs::remove_dir_all(rig.data()).unwrap();
    fs::create_dir_all(rig.data()).unwrap();
    rig.restart();
    rig.sweep();
    assert_eq!(
        fs::read(rig.path("keep/precious.txt")).unwrap(),
        b"the only copy"
    );
    rig.settle();
    assert!(
        rig.server.log().is_empty(),
        "a local wipe never amplifies into remote mutations: {:?}",
        rig.server.log()
    );
}

use std::path::Path;

use fdrive_windows::wire;

fn attrs(path: &Path) -> u32 {
    use std::os::windows::fs::MetadataExt;
    fs::metadata(path).map(|m| m.file_attributes()).unwrap_or(0)
}

fn is_placeholder(path: &Path) -> bool {
    attrs(path) & 0x400 != 0
}

fn is_dehydrated(path: &Path) -> bool {
    attrs(path) & 0x40_0000 != 0
}

fn is_pinned(path: &Path) -> bool {
    attrs(path) & 0x8_0000 != 0
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(25) {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("timed out waiting for {what}");
}

fn unpin(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::CloudFilters::{
        CfCloseHandle, CfOpenFileWithOplock, CfSetPinState, CF_OPEN_FILE_FLAG_WRITE_ACCESS,
        CF_PIN_STATE_UNPINNED, CF_SET_PIN_FLAG_NONE,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let handle =
        unsafe { CfOpenFileWithOplock(PCWSTR(wide.as_ptr()), CF_OPEN_FILE_FLAG_WRITE_ACCESS) }
            .unwrap();
    unsafe { CfSetPinState(handle, CF_PIN_STATE_UNPINNED, CF_SET_PIN_FLAG_NONE, None) }.unwrap();
    unsafe { CfCloseHandle(handle) };
}

fn recycle(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let rc = unsafe { SHFileOperationW(&mut op) };
    assert_eq!(rc, 0, "SHFileOperationW returned {rc}");
}

#[test]
#[ignore = "e2e"]
fn a_dehydrated_placeholder_tracks_a_remote_change() {
    let rig = Rig::start();
    rig.server.put_at(
        "/rd.txt",
        b"v1",
        SystemTime::now() - Duration::from_secs(60),
    );
    rig.sweep();
    wait_until("the placeholder", || rig.path("rd.txt").exists());
    assert!(is_dehydrated(&rig.path("rd.txt")), "never read, no bytes");

    rig.server.put("/rd.txt", b"v2 much longer content");
    rig.sweep();
    wait_until("the new size", || {
        fs::metadata(rig.path("rd.txt")).map(|m| m.len()).ok() == Some(22)
    });
    assert_eq!(fs::read(rig.path("rd.txt")).unwrap(), b"v2 much longer content");
}

#[test]
#[ignore = "e2e"]
fn a_remote_delete_removes_the_placeholder() {
    let rig = Rig::start();
    rig.server.put("/gone.txt", b"soon gone");
    rig.sweep();
    assert_eq!(fs::read(rig.path("gone.txt")).unwrap(), b"soon gone");

    rig.server.rm("/gone.txt");
    rig.sweep();
    wait_until("the placeholder to vanish", || !rig.path("gone.txt").exists());
    rig.settle();
    assert!(rig.server.log().is_empty(), "{:?}", rig.server.log());
}

#[test]
#[ignore = "e2e"]
fn a_remote_tree_populates_on_first_enumeration() {
    let rig = Rig::start();
    rig.server.put("/rtree/a.txt", b"aaa");
    rig.server.put("/rtree/deep/b.txt", b"bbb");
    rig.sweep();

    let names: Vec<String> = fs::read_dir(rig.path("rtree"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.contains(&"a.txt".to_string()) && names.contains(&"deep".to_string()),
        "first enumeration gave: {names:?}"
    );
    assert_eq!(fs::read(rig.path("rtree/deep/b.txt")).unwrap(), b"bbb");
}

#[test]
#[ignore = "e2e"]
fn a_remote_delete_of_a_clean_dir_removes_it() {
    let rig = Rig::start();
    rig.server.put("/rtree/a.txt", b"aaa");
    rig.sweep();
    fs::read_dir(rig.path("rtree")).unwrap().count();

    rig.server.rm("/rtree");
    rig.sweep();
    wait_until("the dir to vanish", || !rig.path("rtree").exists());
}

#[test]
#[ignore = "e2e"]
fn an_uploaded_file_becomes_a_placeholder() {
    let rig = Rig::start();
    fs::write(rig.path("l1.txt"), b"local one").unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/l1.txt").unwrap(), b"local one");
    wait_until("the conversion", || is_placeholder(&rig.path("l1.txt")));
}

#[test]
#[ignore = "e2e"]
fn a_local_rename_propagates() {
    let rig = Rig::start();
    fs::write(rig.path("old.txt"), b"same bytes").unwrap();
    rig.settle();

    fs::rename(rig.path("old.txt"), rig.path("new.txt")).unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/new.txt").unwrap(), b"same bytes");
    assert!(rig.server.get("/old.txt").is_none());
}

#[test]
#[ignore = "e2e"]
fn a_new_dir_and_a_move_in_propagate() {
    let rig = Rig::start();
    fs::write(rig.path("doc.txt"), b"content").unwrap();
    rig.settle();

    fs::create_dir(rig.path("sub")).unwrap();
    rig.settle();
    assert!(rig.server.names("/").contains(&"sub".to_string()));

    fs::rename(rig.path("doc.txt"), rig.path("sub/doc.txt")).unwrap();
    rig.settle();
    assert_eq!(rig.server.get("/sub/doc.txt").unwrap(), b"content");
    assert!(rig.server.get("/doc.txt").is_none());
}

#[test]
#[ignore = "e2e"]
fn a_local_file_delete_propagates() {
    let rig = Rig::start();
    fs::write(rig.path("bye.txt"), b"x").unwrap();
    rig.settle();

    fs::remove_file(rig.path("bye.txt")).unwrap();
    rig.settle();
    assert!(rig.server.get("/bye.txt").is_none());
}

#[test]
#[ignore = "e2e"]
fn a_local_dir_delete_propagates() {
    let rig = Rig::start();
    fs::create_dir(rig.path("sub")).unwrap();
    fs::write(rig.path("sub/x.txt"), b"x").unwrap();
    rig.settle();

    fs::remove_dir_all(rig.path("sub")).unwrap();
    rig.settle();
    assert_eq!(rig.server.names("/"), Vec::<String>::new());
}

#[test]
#[ignore = "e2e"]
fn recycling_a_hydrated_placeholder_deletes_upstream() {
    let rig = Rig::start();
    rig.server.put("/rec1.txt", b"to be recycled");
    rig.sweep();
    fs::read(rig.path("rec1.txt")).unwrap();

    recycle(&rig.path("rec1.txt"));
    rig.settle();
    assert!(rig.server.get("/rec1.txt").is_none());
    assert!(!rig.path("rec1.txt").exists());
}

#[test]
#[ignore = "e2e"]
fn recycling_a_dehydrated_placeholder_deletes_upstream() {
    let rig = Rig::start();
    rig.server.put("/rec2.txt", b"never opened");
    rig.sweep();
    wait_until("the placeholder", || rig.path("rec2.txt").exists());

    recycle(&rig.path("rec2.txt"));
    rig.settle();
    assert!(rig.server.get("/rec2.txt").is_none());
    assert!(!rig.path("rec2.txt").exists());
}

#[test]
#[ignore = "e2e"]
fn recycling_a_directory_deletes_upstream() {
    let rig = Rig::start();
    rig.server.put("/recdir/x.txt", b"x");
    rig.sweep();
    fs::read_dir(rig.path("recdir")).unwrap().count();

    recycle(&rig.path("recdir"));
    rig.settle();
    assert!(!rig.server.names("/").contains(&"recdir".to_string()));
    assert!(!rig.path("recdir").exists());
}

#[test]
#[ignore = "e2e"]
fn a_pin_hydrates_the_placeholder() {
    let rig = Rig::start();
    rig.server.put("/pin1.txt", b"pin me");
    rig.sweep();
    wait_until("the placeholder", || rig.path("pin1.txt").exists());
    assert!(is_dehydrated(&rig.path("pin1.txt")));

    wire::set_pinned(&rig.path("pin1.txt")).unwrap();
    wait_until("bytes on disk", || !is_dehydrated(&rig.path("pin1.txt")));
}

#[test]
#[ignore = "e2e"]
fn an_unpin_releases_the_bytes() {
    let rig = Rig::start();
    rig.server.put("/pin1.txt", b"pin me");
    rig.sweep();
    wait_until("the placeholder", || rig.path("pin1.txt").exists());
    wire::set_pinned(&rig.path("pin1.txt")).unwrap();
    wait_until("bytes on disk", || !is_dehydrated(&rig.path("pin1.txt")));

    unpin(&rig.path("pin1.txt"));
    wait_until("bytes released", || is_dehydrated(&rig.path("pin1.txt")));
}

#[test]
#[ignore = "e2e"]
fn a_pinned_file_stays_pinned_across_refreshes() {
    let rig = Rig::start();
    rig.server.put("/pinstay.txt", b"pinned v1");
    rig.sweep();
    wait_until("the placeholder", || rig.path("pinstay.txt").exists());
    wire::set_pinned(&rig.path("pinstay.txt")).unwrap();
    wait_until("hydrated", || !is_dehydrated(&rig.path("pinstay.txt")));

    rig.sweep();
    rig.settle();
    assert!(!is_dehydrated(&rig.path("pinstay.txt")));
    assert!(is_pinned(&rig.path("pinstay.txt")));
}

#[test]
#[ignore = "e2e"]
fn a_pinned_file_tracks_the_remote_change() {
    let rig = Rig::start();
    rig.server.put_at(
        "/pinstay.txt",
        b"pinned v1",
        SystemTime::now() - Duration::from_secs(60),
    );
    rig.sweep();
    wait_until("the placeholder", || rig.path("pinstay.txt").exists());
    wire::set_pinned(&rig.path("pinstay.txt")).unwrap();
    wait_until("hydrated", || !is_dehydrated(&rig.path("pinstay.txt")));

    rig.server.put("/pinstay.txt", b"pinned v2 now much longer");
    rig.sweep();
    wait_until("the new version hydrated", || {
        !is_dehydrated(&rig.path("pinstay.txt"))
            && fs::metadata(rig.path("pinstay.txt")).map(|m| m.len()).ok() == Some(25)
    });
    assert_eq!(
        fs::read(rig.path("pinstay.txt")).unwrap(),
        b"pinned v2 now much longer"
    );
    assert!(is_pinned(&rig.path("pinstay.txt")));
}

#[test]
#[ignore = "e2e"]
fn a_concurrent_edit_keeps_both_versions() {
    let rig = Rig::start();
    fs::write(rig.path("c1.txt"), b"base").unwrap();
    rig.settle();
    wait_until("the conversion", || is_placeholder(&rig.path("c1.txt")));

    rig.server.put("/c1.txt", b"server version");
    fs::write(rig.path("c1.txt"), b"local version").unwrap();
    rig.settle();

    assert_eq!(rig.server.get("/c1.txt").unwrap(), b"server version");
    assert_eq!(
        rig.server.get("/c1 (conflicted copy).txt").unwrap(),
        b"local version"
    );
}

#[test]
#[ignore = "e2e"]
fn a_dirty_file_survives_a_remote_dir_delete() {
    let rig = Rig::start();
    rig.server.put("/ddir/keep.txt", b"server");
    rig.sweep();
    fs::read(rig.path("ddir/keep.txt")).unwrap();

    fs::write(rig.path("ddir/keep.txt"), b"my precious edits").unwrap();
    rig.server.rm("/ddir");
    rig.sweep();
    rig.settle();

    assert_eq!(
        fs::read(rig.path("ddir/keep.txt")).unwrap(),
        b"my precious edits"
    );
    assert_eq!(
        rig.server.get("/ddir/keep.txt").unwrap(),
        b"my precious edits"
    );
}

#[test]
#[ignore = "e2e"]
fn unicode_names_roundtrip() {
    let rig = Rig::start();
    rig.server.put("/café rémote.txt", "unicode remote".as_bytes());
    rig.sweep();
    assert_eq!(
        fs::read(rig.path("café rémote.txt")).unwrap(),
        "unicode remote".as_bytes()
    );

    fs::write(rig.path("café löcal.txt"), "unicode local".as_bytes()).unwrap();
    rig.settle();
    assert_eq!(
        rig.server.get("/café löcal.txt").unwrap(),
        "unicode local".as_bytes()
    );
}

#[test]
#[ignore = "e2e"]
fn a_big_file_roundtrips_bit_exact() {
    let rig = Rig::start();
    let mut x = 42u32;
    let big: Vec<u8> = (0..5 << 20)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8
        })
        .collect();
    rig.server.put("/big.bin", &big);
    rig.sweep();

    assert_eq!(fs::read(rig.path("big.bin")).unwrap(), big);
    rig.settle();
    assert!(rig.server.log().is_empty(), "{:?}", rig.server.log());
}

#[test]
#[ignore = "e2e"]
fn enumeration_is_instant_the_second_time() {
    let rig = Rig::start();
    for i in 0..40 {
        rig.server.put(&format!("/many/f{i}.txt"), format!("file {i}").as_bytes());
    }
    rig.sweep();

    let start = std::time::Instant::now();
    let names: Vec<String> = fs::read_dir(rig.path("many"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let first = start.elapsed();
    let start = std::time::Instant::now();
    fs::read_dir(rig.path("many")).unwrap().count();
    let second = start.elapsed();

    assert_eq!(names.len(), 40);
    assert!(first < Duration::from_secs(10), "first: {first:?}");
    assert!(second < Duration::from_secs(1), "second: {second:?}");
}

use std::time::Duration;

use httpmock::{Method, MockServer};

use super::testkit::*;
use crate::path::RelPath;

#[test]
fn a_held_window_wakes_at_its_release_not_before() {
    use crate::engine::state::State;
    use crate::model::Operation;
    use crate::port::LocalStore;

    let tree = TempTree::new();
    let mut state = State::open(&tree.ledger());
    let path = RelPath::new("f");
    state.write_opened(&path);
    state.record(Operation::Create(path.clone()));
    std::thread::sleep(Duration::from_millis(300));

    let step = state.step(4, false, &crate::config::ignore(&tree.ledger()), |_| false);

    assert!(step.plans.is_empty());
    assert!(!step.idle);
    let wake = step.wake.expect("a held op schedules its release");
    let from_now = wake.saturating_duration_since(std::time::Instant::now());
    assert!(
        from_now > Duration::from_secs(3) && from_now <= Duration::from_secs(5),
        "wake should land at the open-file grace, got {from_now:?}"
    );
}

#[tokio::test]
async fn a_file_open_for_writing_holds_its_save() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/cat");
        then.status(200);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.write_opened(&path);
    engine.local().write("f", b"half-written");
    engine.created(&path);
    engine.modified(&path);

    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    save.assert_hits(0);

    engine.write_closed(&path);
    settle(&engine).await;
    save.assert_hits(1);
}

#[tokio::test]
async fn an_emptied_file_waits_for_its_rewrite() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/f");
        then.status(200).header("Last-Modified", MTIME);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine
        .ledger()
        .observations
        .insert(path.clone(), observed(5));

    engine.local().write("f", b"");
    engine.modified(&path);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    save.assert_hits(0);

    engine.local().write("f", b"the real bytes");
    engine.modified(&path);
    settle(&engine).await;
    save.assert_hits(1);
}

#[tokio::test]
async fn a_local_only_delete_is_vacuous() {
    let server = MockServer::start();
    let rm = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(200);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.ledger().dirty.insert(path.clone());
    engine.delete(&path, false).await.unwrap();
    settle(&engine).await;
    rm.assert_hits(0);
    assert!(engine.ledger().dirty.is_empty());
}

#[tokio::test]
async fn a_failed_remove_stays_owed() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(Method::HEAD).path("/api/files/cat");
        then.status(500);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine
        .ledger()
        .observations
        .insert(path.clone(), observed(5));
    engine.delete(&path, false).await.unwrap();
    engine.flush(Duration::from_millis(1200)).await;
    assert!(engine.ledger().observations.contains_key(&path));
    assert_eq!(engine.state.lock().unwrap().pending(), 1);
}

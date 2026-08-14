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

#[test]
fn a_dirty_rename_keeps_its_mark_across_a_crash() {
    use crate::engine::state::State;
    use crate::model::Operation;
    use crate::port::LocalStore;

    let tree = TempTree::new();
    {
        let mut state = State::open(&tree.ledger());
        state.record(Operation::Write(RelPath::new("a")));
        state.record(Operation::Rename(RelPath::new("a"), RelPath::new("b")));
    }
    let state = State::open(&tree.ledger());
    let dirty: Vec<&str> = state.ledger.dirty.iter().map(|p| p.as_str()).collect();
    assert_eq!(dirty, ["b"], "the edit must be owed at its new name");
}

#[test]
fn a_delete_clears_the_mark_it_supersedes() {
    use crate::engine::state::State;
    use crate::model::Operation;
    use crate::port::LocalStore;

    let tree = TempTree::new();
    {
        let mut state = State::open(&tree.ledger());
        state.record(Operation::Write(RelPath::new("a")));
        state.record(Operation::Delete(RelPath::new("a")));
    }
    let state = State::open(&tree.ledger());
    assert!(state.ledger.dirty.is_empty());
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
    engine.fs().write_opened(&path);
    engine.local().write("f", b"half-written");
    engine.fs().created(&path);
    engine.fs().modified(&path);

    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    save.assert_hits(0);

    engine.fs().write_closed(&path);
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
    engine.fs().modified(&path);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    save.assert_hits(0);

    engine.local().write("f", b"the real bytes");
    engine.fs().modified(&path);
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
    engine.fs().delete(&path, false).await.unwrap();
    settle(&engine).await;
    rm.assert_hits(0);
    assert!(engine.ledger().dirty.is_empty());
}

#[tokio::test]
async fn a_failed_remove_stays_owed() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(500);
    });
    server.mock(|when, then| {
        when.method(Method::HEAD).path("/api/files/cat");
        then.status(200)
            .header("content-length", "5")
            .header("last-modified", MTIME);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine
        .ledger()
        .observations
        .insert(path.clone(), observed(5));
    engine.fs().delete(&path, false).await.unwrap();
    engine.system().flush(Duration::from_millis(1200)).await;
    assert!(engine.ledger().observations.contains_key(&path));
    assert_eq!(engine.state.lock().unwrap().pending(), 1);
}

#[tokio::test]
async fn a_wiped_db_with_a_leftover_cache_mutates_nothing() {
    let server = MockServer::start();
    let (rm, mv, save) = tripwires(&server);
    let tree = TempTree::new();
    tree.write("was-dirty-before-the-wipe.txt", b"leftover");
    let engine = engine_with(&server, tree);

    settle(&engine).await;

    rm.assert_hits(0);
    mv.assert_hits(0);
    save.assert_hits(0);
}

#[tokio::test]
async fn a_journal_with_children_behind_their_dir_still_drains() {
    let owner = TempTree::new();
    {
        let db = rusqlite::Connection::open(&owner.state).unwrap();
        db.execute_batch(
            "CREATE TABLE journal(seq INTEGER PRIMARY KEY, op TEXT NOT NULL, path TEXT NOT NULL, dest TEXT, base TEXT, size INTEGER, time INTEGER);
             INSERT INTO journal(op, path) VALUES ('d', 'd/sub');
             INSERT INTO journal(op, path) VALUES ('d', 'd');
             INSERT INTO journal(op, path, size, time) VALUES ('r', 'd/sub/f', 1, 1);
             INSERT INTO journal(op, path, size, time) VALUES ('r', 'd/g', 1, 1);",
        )
        .unwrap();
    }
    let server = MockServer::start();
    let (rm, mv, save) = tripwires(&server);
    let engine = engine_with(&server, TempTree::reopen(&owner));

    settle(&engine).await;

    assert!(
        engine.state.lock().unwrap().idle(),
        "the wave drains instead of deadlocking"
    );
    rm.assert_hits(4);
    mv.assert_hits(0);
    save.assert_hits(0);
}

#[tokio::test]
async fn a_crash_with_a_pending_remove_replays_it_exactly_once() {
    let server = MockServer::start();
    let mut down = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(500);
    });
    let mut alive = server.mock(|when, then| {
        when.method(Method::HEAD).path("/api/files/cat");
        then.status(200).header("content-length", "5");
    });
    let owner = TempTree::new();
    let path = RelPath::new("doomed.txt");
    {
        let crashed = engine_with(&server, TempTree::reopen(&owner));
        crashed.ledger().observe(&path, observed(5));
        crashed.fs().delete(&path, false).await.unwrap();
        crashed.system().flush(Duration::from_secs(1)).await;
    }
    down.delete();
    alive.delete();

    let (rm, mv, save) = tripwires(&server);
    let engine = engine_with(&server, TempTree::reopen(&owner));
    settle(&engine).await;

    rm.assert_hits(1);
    mv.assert_hits(0);
    save.assert_hits(0);
    assert!(engine.ledger().observations.is_empty());
}

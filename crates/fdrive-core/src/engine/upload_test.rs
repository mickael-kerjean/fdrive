use std::fs;
use std::sync::Arc;
use std::time::Duration;

use httpmock::{Method, MockServer};

use super::testkit::*;
use crate::engine::{Engine, Observation};
use crate::path::RelPath;
use crate::sdk::Sdk;

#[tokio::test]
async fn a_new_file_saves_on_flush() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/f");
        then.status(200);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.local().write("f", b"hello");
    engine.fs().created(&path);
    engine.fs().modified(&path);

    settle(&engine).await;
    save.assert_hits(1);
    assert!(engine.ledger().dirty.is_empty());
    assert_eq!(*engine.local().settled.lock().unwrap(), [path]);
}

#[tokio::test]
async fn a_save_carries_its_lease() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/f")
            .header("If-Unmodified-Since", MTIME);
        then.status(200).header("Last-Modified", MTIME);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.local().write("f", b"hello");
    engine
        .ledger()
        .observations
        .insert(path.clone(), observed(5));
    engine.fs().modified(&path);

    settle(&engine).await;
    save.assert_hits(1);
    assert_eq!(engine.ledger().observations[&path], observed(5));
    assert!(engine.ledger().dirty.is_empty());
}

#[tokio::test]
async fn a_vanished_file_settles_without_the_server() {
    let server = MockServer::start();
    let engine = engine(&server);
    let path = RelPath::new("gone");
    engine.fs().modified(&path);
    settle(&engine).await;
    assert!(engine.ledger().dirty.is_empty());
}

#[tokio::test]
async fn a_failed_save_keeps_the_debt() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/cat");
        then.status(403);
    });
    let mkdir = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/mkdir");
        then.status(200);
    });
    let engine = engine(&server);
    let path = RelPath::new("a/b/f");
    engine.local().write("a/b/f", b"deep");
    engine.fs().modified(&path);

    engine.system().flush(Duration::from_millis(1200)).await;
    mkdir.assert_hits(2);
    save.assert_hits(2);
    assert!(engine.ledger().dirty.contains(&path));
}

#[tokio::test]
async fn a_save_conflict_keeps_both_versions() {
    let server = MockServer::start();
    let reject = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/f");
        then.status(412);
    });
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/f (conflicted copy from testkit)")
            .header("If-Unmodified-Since", "Thu, 01 Jan 1970 00:00:00 GMT");
        then.status(200).header("Last-Modified", MTIME);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.local().write("f", b"ours");
    engine
        .ledger()
        .observations
        .insert(path.clone(), Observation::new(1, None));
    engine.fs().modified(&path);

    settle(&engine).await;
    save.assert_hits(1);
    reject.assert_hits(1);
    assert_eq!(engine.local().read("f"), None);
    assert_eq!(
        engine
            .local()
            .read("f (conflicted copy from testkit)")
            .as_deref(),
        Some(b"ours".as_slice())
    );
    assert!(engine.ledger().dirty.is_empty());
}

#[tokio::test]
async fn conflicts_never_clobber_a_local_copy() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/f");
        then.status(412);
    });
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/f (conflicted copy from testkit 2)");
        then.status(200);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.local().write("f", b"ours");
    engine
        .local()
        .write("f (conflicted copy from testkit)", b"precious");
    engine
        .ledger()
        .observations
        .insert(path.clone(), Observation::new(1, None));
    engine.fs().modified(&path);

    settle(&engine).await;
    save.assert_hits(1);
    assert_eq!(
        engine
            .local()
            .read("f (conflicted copy from testkit)")
            .as_deref(),
        Some(b"precious".as_slice())
    );
    assert_eq!(
        engine
            .local()
            .read("f (conflicted copy from testkit 2)")
            .as_deref(),
        Some(b"ours".as_slice())
    );
}

#[tokio::test]
async fn a_cache_deleted_under_pending_edits_mutates_nothing() {
    let server = MockServer::start();
    let (rm, mv, save) = tripwires(&server);
    let engine = engine(&server);
    let path = RelPath::new("doc.txt");
    engine.fs().created(&path);
    engine.fs().modified(&path);

    settle(&engine).await;

    rm.assert_hits(0);
    mv.assert_hits(0);
    save.assert_hits(0);
    assert!(engine.ledger().dirty.is_empty(), "the debt is written off");
}

#[tokio::test]
async fn a_stale_save_landing_on_a_file_turned_directory_uploads_nothing() {
    let server = MockServer::start();
    let mut down = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/cat");
        then.status(500);
    });
    let owner = TempTree::new();
    let path = RelPath::new("doc.txt");
    {
        let crashed = engine_with(&server, TempTree::reopen(&owner));
        crashed.fs().created(&path);
        crashed.fs().modified(&path);
        owner.write("doc.txt", b"unsent edit");
        crashed.system().flush(Duration::from_secs(1)).await;
    }
    down.delete();
    fs::remove_file(owner.dir.join("doc.txt")).unwrap();
    fs::create_dir(owner.dir.join("doc.txt")).unwrap();
    owner.write("doc.txt/nested", b"the name is a folder now");

    let (rm, mv, save) = tripwires(&server);
    let engine = engine_with(&server, TempTree::reopen(&owner));
    settle(&engine).await;

    rm.assert_hits(0);
    mv.assert_hits(0);
    save.assert_hits(0);
    assert!(
        engine.state.lock().unwrap().idle(),
        "the stale save is retired, not retried"
    );
}

#[tokio::test]
async fn a_dead_server_leaves_no_mark_anywhere() {
    let sdk = Sdk::new("http://127.0.0.1:9").unwrap();
    let rt = tokio::runtime::Handle::current();
    let engine = Engine::start(rt, Arc::new(sdk), TempTree::new());
    let path = RelPath::new("doc.txt");
    engine.ledger().observe(&path, Observation::new(1, None));
    engine.fs().created(&path);
    engine.fs().modified(&path);
    engine.fs().delete(&path, false).await.unwrap();

    engine.system().flush(Duration::from_secs(1)).await;

    assert!(
        !engine.state.lock().unwrap().idle(),
        "the debt is still owed, nothing was dropped"
    );
}

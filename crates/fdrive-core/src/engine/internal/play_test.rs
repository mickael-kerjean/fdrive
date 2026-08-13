use httpmock::{Method, MockServer};

use super::testkit::*;
use crate::path::RelPath;

#[tokio::test]
async fn a_remove_deletes_the_server_copy() {
    let server = MockServer::start();
    let rm = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/f");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine
        .ledger()
        .observations
        .insert(path.clone(), observed(5));

    engine.delete(&path, false).await.unwrap();
    settle(&engine).await;
    rm.assert_hits(1);
    assert!(engine.ledger().observations.is_empty());
}

#[tokio::test]
async fn a_remove_of_an_already_gone_file_retires() {
    // filestash answers rm of a missing target with a 500, not a 404
    let server = MockServer::start();
    let rm = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(500);
    });
    let stat = server.mock(|when, then| {
        when.method(Method::HEAD).path("/api/files/cat");
        then.status(404);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine
        .ledger()
        .observations
        .insert(path.clone(), observed(5));

    engine.delete(&path, false).await.unwrap();
    settle(&engine).await;
    rm.assert_hits(1);
    stat.assert_hits(1);
    assert!(engine.ledger().observations.is_empty());
    assert!(
        engine.state.lock().unwrap().idle(),
        "already-gone is success"
    );
}

#[tokio::test]
async fn a_move_follows_a_vanished_source_with_a_save() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(Method::HEAD).path("/api/files/cat");
        then.status(404);
    });
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/b");
        then.status(200);
    });
    let engine = engine(&server);
    let (a, b) = (RelPath::new("a"), RelPath::new("b"));
    engine.ledger().observations.insert(a.clone(), observed(5));
    engine.local().write("b", b"moved");

    engine.rename(&a, &b, false).await.unwrap();
    settle(&engine).await;
    save.assert_hits(1);
}

use std::fs;

use httpmock::{Method, MockServer};

use super::testkit::*;
use crate::engine::Observation;
use crate::path::RelPath;

#[tokio::test]
async fn unreadable_ledger_quarantines_instead_of_pruning() {
    let server = MockServer::start();
    let tree = TempTree::new();
    tree.write("only-copy.txt", b"bytes");
    fs::write(&tree.state, b"not json").unwrap();

    let engine = engine_with(&server, tree);
    let root = engine.tree().dir.clone();
    engine.prune(&root).unwrap();

    assert!(
        engine.tree().read("only-copy.txt").is_none(),
        "the cache was set aside"
    );
    let prefix = format!(
        "{}.unreadable-",
        root.file_name().unwrap().to_string_lossy()
    );
    let aside = fs::read_dir(root.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .expect("quarantine dir exists");
    assert!(aside.path().join("only-copy.txt").exists());
    fs::remove_dir_all(aside.path()).unwrap();

    engine.tree().write("fresh.txt", b"clean");
    engine.prune(&root).unwrap();
    assert!(
        engine.tree().read("fresh.txt").is_none(),
        "a second prune prunes normally instead of quarantining"
    );
    assert!(
        !fs::read_dir(root.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with(&prefix)),
        "no second quarantine"
    );
}

#[tokio::test]
async fn a_pin_hydrates_the_subtree() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(Method::GET)
            .path("/api/files/ls")
            .query_param("path", "/d/");
        then.status(200).json_body(serde_json::json!({
            "status": "ok",
            "results": [{"name": "f.txt", "size": 5, "time": 0, "type": "file"}]
        }));
    });
    let cat = server.mock(|when, then| {
        when.method(Method::GET)
            .path("/api/files/cat")
            .query_param("path", "/d/f.txt");
        then.status(200).body("hello");
    });
    let engine = engine(&server);
    engine.ledger().pin_set(&RelPath::new("d"));
    engine.hydrate_subtree(&RelPath::new("d")).await;
    cat.assert_hits(1);
    assert_eq!(engine.tree().read("d/f.txt").unwrap(), b"hello");
    assert!(
        engine
            .ledger()
            .observations
            .contains_key(&RelPath::new("d/f.txt")),
        "the walk observed what it listed"
    );

    engine.hydrate_subtree(&RelPath::new("d")).await;
    cat.assert_hits(1);
}

#[tokio::test]
async fn prune_spares_pinned_content() {
    let server = MockServer::start();
    let engine = engine(&server);
    let root = engine.tree().dir.clone();
    let path = RelPath::new("d/f.txt");
    engine.tree().write("d/f.txt", b"hello");
    engine.ledger().observe(&path, Observation::new(5, None));
    engine.ledger().pin_set(&RelPath::new("d"));

    engine.prune(&root).unwrap();
    assert_eq!(engine.tree().read("d/f.txt").unwrap(), b"hello");
    assert!(engine.ledger().observations.contains_key(&path));

    engine.unpin(&RelPath::new("d"));
    engine.prune(&root).unwrap();
    assert!(engine.tree().read("d/f.txt").is_none());
    assert!(
        engine.ledger().observations.contains_key(&path),
        "prune drops bytes, never knowledge"
    );
}

#[tokio::test]
async fn prune_keeps_the_delta_base_across_restarts() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(Method::OPTIONS).path("/api/files/save");
        then.status(200)
            .header("Accept-Post", crate::sdk::DELTA_MEDIA_TYPE);
    });
    let delta = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .header("content-type", crate::sdk::DELTA_MEDIA_TYPE);
        then.status(200)
            .header("last-modified", MTIME)
            .body(r#"{"status":"ok"}"#);
    });
    let engine = engine(&server);
    let root = engine.tree().dir.clone();
    let path = RelPath::new("big.txt");
    let synced = vec![b'a'; 8192];
    engine.tree().write("big.txt", &synced);
    engine.ledger().observe(&path, observed(8192));
    let sig = crate::engine::upload::signature(&synced);
    engine.ledger().sign_set(&path, &sig);

    engine.prune(&root).unwrap();
    assert!(engine.tree().read("big.txt").is_none());
    assert!(
        engine.ledger().sign_get(&path).is_some(),
        "the signature describes server content, it outlives the bytes"
    );
    assert!(
        !engine.content_current(&path, observed(8192)),
        "a kept observation cannot fake freshness"
    );

    let mut edited = synced.clone();
    edited[0] = b'b';
    engine.tree().write("big.txt", &edited);
    engine.modified(&path);
    settle(&engine).await;

    delta.assert_hits(1);
    assert!(!engine.ledger().dirty.contains(&path));
}

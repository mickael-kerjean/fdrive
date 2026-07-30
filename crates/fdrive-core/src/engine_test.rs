use std::time::Duration;

use httpmock::{Method, MockServer};

use super::testkit::*;
use crate::engine::{Engine, Observation};
use crate::path::RelPath;
use crate::sdk::Sdk;
use std::sync::Arc;

#[tokio::test]
async fn modified_marks_the_debt_once() {
    let server = MockServer::start();
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.modified(&path);
    engine.modified(&path);
    assert!(engine.ledger().dirty.contains(&path));
}

#[tokio::test]
async fn created_keeps_the_lease() {
    let server = MockServer::start();
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine
        .ledger()
        .observations
        .insert(path.clone(), Observation::new(1, None));
    engine.created(&path);
    let ledger = engine.ledger();
    assert!(ledger.dirty.contains(&path));
    assert!(
        ledger.observations.contains_key(&path),
        "the observation is the lease the save will carry"
    );
}

#[tokio::test]
async fn the_vim_dance_is_one_save() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/a");
        then.status(200).header("Last-Modified", MTIME);
    });
    let mv = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/mv");
        then.status(200);
    });
    let rm = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(200);
    });
    let engine = engine(&server);
    let (a, backup) = (RelPath::new("a"), RelPath::new("a~"));
    engine.ledger().observations.insert(a.clone(), observed(2));
    engine.local().write("a", b"v2");

    engine.rename(&a, &backup, false).await.unwrap();
    engine.created(&a);
    engine.modified(&a);
    engine.delete(&backup, false).await.unwrap();
    settle(&engine).await;

    save.assert_hits(1);
    mv.assert_hits(0);
    rm.assert_hits(0);
    assert!(engine.ledger().dirty.is_empty());
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn the_backup_dance_moves_then_saves() {
    let server = MockServer::start();
    let stat = server.mock(|when, then| {
        when.method(Method::HEAD)
            .path("/api/files/cat")
            .query_param("path", "/x");
        then.status(200)
            .header("content-length", "5")
            .header("last-modified", MTIME);
    });
    let mv = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/mv")
            .query_param("from", "/x")
            .query_param("to", "/x_original");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/x");
        then.status(200).header("Last-Modified", MTIME);
    });
    let engine = engine(&server);
    let (x, tmp, orig) = (
        RelPath::new("x"),
        RelPath::new("x_tmp"),
        RelPath::new("x_original"),
    );
    engine.ledger().observations.insert(x.clone(), observed(5));
    engine.local().write("x", b"newer");

    engine.created(&tmp);
    engine.modified(&tmp);
    engine.rename(&x, &orig, false).await.unwrap();
    engine.rename(&tmp, &x, false).await.unwrap();
    settle(&engine).await;

    stat.assert_hits(1);
    mv.assert_hits(1);
    save.assert_hits(1);
    let ledger = engine.ledger();
    let keys: Vec<&str> = ledger.observations.keys().map(|p| p.as_str()).collect();
    assert_eq!(keys, ["x", "x_original"]);
    drop(ledger);
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn a_file_deleted_in_the_window_never_touches_the_server() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/cat");
        then.status(200);
    });
    let rm = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(200);
    });
    let engine = engine(&server);
    let path = RelPath::new("db-journal");
    engine.local().write("db-journal", b"tmp");
    engine.created(&path);
    engine.modified(&path);
    engine.released(&path);
    engine.delete(&path, false).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    save.assert_hits(0);
    rm.assert_hits(0);
    assert!(engine.ledger().dirty.is_empty());
}

#[tokio::test]
async fn dir_delete_never_ships_doomed_content() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/d/f");
        then.status(200);
    });
    let rm_file = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/f");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let rm_dir = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    let (dir, child) = (RelPath::new("d"), RelPath::new("d/f"));
    engine.local().write("d/f", b"x");
    engine.created(&child);
    engine.modified(&child);

    engine.delete(&dir, true).await.unwrap();
    settle(&engine).await;
    save.assert_hits(0);
    rm_file.assert_hits(0);
    rm_dir.assert_hits(1);
    assert!(engine.ledger().observations.is_empty());
    assert!(engine.ledger().dirty.is_empty());
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn dir_delete_is_one_recursive_rm() {
    let server = MockServer::start();
    let rm_file = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/f");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let rm_dir = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    let (dir, child) = (RelPath::new("d"), RelPath::new("d/f"));
    engine.ledger().observations.insert(child, observed(1));

    engine.delete(&dir, true).await.unwrap();
    settle(&engine).await;
    rm_file.assert_hits(0);
    rm_dir.assert_hits(1);
    assert!(engine.ledger().observations.is_empty());
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn dir_delete_subsumes_the_wave_beneath_it() {
    let server = MockServer::start();
    let rm_dir = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let rm_children: Vec<_> = (0..5)
        .map(|i| {
            server.mock(|when, then| {
                when.method(Method::POST)
                    .path("/api/files/rm")
                    .query_param("path", format!("/d/f{i}"));
                then.status(200)
                    .json_body(serde_json::json!({"status": "ok"}));
            })
        })
        .collect();
    let engine = engine(&server);
    let dir = RelPath::new("d");
    let children: Vec<RelPath> = (0..5).map(|i| RelPath::new(&format!("d/f{i}"))).collect();
    for child in &children {
        engine
            .ledger()
            .observations
            .insert(child.clone(), observed(1));
    }
    for child in &children {
        engine.delete(child, false).await.unwrap();
    }
    engine.delete(&dir, true).await.unwrap();
    settle(&engine).await;

    rm_dir.assert_hits(1);
    for rm_child in &rm_children {
        rm_child.assert_hits(0);
    }
    assert!(engine.ledger().observations.is_empty());
    assert!(
        engine.state.lock().unwrap().idle(),
        "one plan stood for the whole wave"
    );
}

#[tokio::test]
async fn a_mass_deletion_lands_unimpeded() {
    let server = MockServer::start();
    let rm = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    let paths: Vec<RelPath> = (0..60).map(|i| RelPath::new(&format!("f{i}"))).collect();
    for path in &paths {
        engine
            .ledger()
            .observations
            .insert(path.clone(), observed(1));
    }
    for path in &paths {
        engine.delete(path, false).await.unwrap();
    }
    settle(&engine).await;

    assert_eq!(rm.hits(), 60, "every delete reaches the server");
    assert!(engine.ledger().observations.is_empty());
}

#[tokio::test]
async fn a_folder_delete_slower_than_the_window_cap_is_still_one_rm() {
    let server = MockServer::start();
    let rm = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/rm");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    let dir = RelPath::new("d");
    let children: Vec<RelPath> = (0..60).map(|i| RelPath::new(&format!("d/f{i}"))).collect();
    for child in &children {
        engine
            .ledger()
            .observations
            .insert(child.clone(), observed(1));
    }
    for child in &children {
        engine.delete(child, false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    engine.delete(&dir, true).await.unwrap();
    settle(&engine).await;

    rm.assert_hits(1);
    assert!(engine.ledger().observations.is_empty());
}

#[tokio::test]
async fn nested_dir_deletes_collapse_into_the_root() {
    let server = MockServer::start();
    let rm_root = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let rm_sub = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/sub/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    engine
        .ledger()
        .observations
        .insert(RelPath::new("d/sub/f"), observed(1));

    engine.delete(&RelPath::new("d/sub"), true).await.unwrap();
    engine.delete(&RelPath::new("d"), true).await.unwrap();
    settle(&engine).await;

    rm_root.assert_hits(1);
    rm_sub.assert_hits(0);
    assert!(engine.ledger().observations.is_empty());
    assert!(engine.state.lock().unwrap().idle());
}

#[tokio::test]
async fn a_move_out_of_a_dying_dir_saves_the_file() {
    let server = MockServer::start();
    let rm_dir = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    server.mock(|when, then| {
        when.method(Method::HEAD)
            .path("/api/files/cat")
            .query_param("path", "/d/x");
        then.status(404);
    });
    let save = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/cat")
            .query_param("path", "/e");
        then.status(200).header("Last-Modified", MTIME);
    });
    let engine = engine(&server);
    let (from, to) = (RelPath::new("d/x"), RelPath::new("e"));
    engine
        .ledger()
        .observations
        .insert(from.clone(), observed(1));
    engine.local().write("e", b"survivor");

    engine.rename(&from, &to, false).await.unwrap();
    engine.delete(&RelPath::new("d"), true).await.unwrap();
    settle(&engine).await;

    rm_dir.assert_hits(1);
    save.assert_hits(1);
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn a_move_into_a_dying_dir_dooms_the_source() {
    let server = MockServer::start();
    let rm_dir = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let rm_src = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/e");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let mv = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/mv");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    let (from, to) = (RelPath::new("e"), RelPath::new("d/x"));
    engine
        .ledger()
        .observations
        .insert(from.clone(), observed(1));

    engine.rename(&from, &to, false).await.unwrap();
    engine.delete(&RelPath::new("d"), true).await.unwrap();
    settle(&engine).await;

    rm_dir.assert_hits(1);
    rm_src.assert_hits(1);
    mv.assert_hits(0);
    assert!(engine.ledger().observations.is_empty());
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn dir_delete_takes_unseen_files_with_it() {
    let server = MockServer::start();
    let rm = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/rm")
            .query_param("path", "/d/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    let dir = RelPath::new("d");

    engine.delete(&dir, true).await.unwrap();
    settle(&engine).await;
    rm.assert_hits(1);
    assert!(engine.conflicts().is_empty());
}

#[tokio::test]
async fn dir_rename_propagates_and_remaps() {
    let server = MockServer::start();
    let mv = server.mock(|when, then| {
        when.method(Method::POST)
            .path("/api/files/mv")
            .query_param("from", "/a/")
            .query_param("to", "/z/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    let engine = engine(&server);
    engine
        .ledger()
        .observations
        .insert(RelPath::new("a/x"), Observation::new(1, None));

    engine
        .rename(&RelPath::new("a"), &RelPath::new("z"), true)
        .await
        .unwrap();
    mv.assert_hits(1);
    let ledger = engine.ledger();
    let keys: Vec<&str> = ledger.observations.keys().map(|p| p.as_str()).collect();
    assert_eq!(keys, ["z/x"]);
}

#[tokio::test]
async fn rename_of_an_unuploaded_file_stays_local() {
    let server = MockServer::start();
    let engine = engine(&server);
    let (from, to) = (RelPath::new("f"), RelPath::new("g"));
    engine.local().write("f", b"bytes");
    engine.modified(&from);
    engine.rename(&from, &to, false).await.unwrap();
    assert!(engine.ledger().dirty.contains(&to));
    assert!(!engine.ledger().dirty.contains(&from));
}

#[tokio::test]
async fn an_offline_dir_rename_is_refused_before_touching_anything() {
    let sdk = Sdk::new("http://127.0.0.1:9").unwrap();
    let rt = tokio::runtime::Handle::current();
    let engine = Engine::start(Arc::new(sdk), rt, TempTree::new());
    engine
        .ledger()
        .observations
        .insert(RelPath::new("a/x"), Observation::new(1, None));

    let refused = engine
        .rename(&RelPath::new("a"), &RelPath::new("z"), true)
        .await;
    assert!(refused.is_err(), "the plane rename fails loudly");
    assert!(
        engine
            .ledger()
            .observations
            .contains_key(&RelPath::new("a/x")),
        "nothing was remapped"
    );
    assert!(engine.fates().is_empty(), "nothing is pending");
    assert!(
        engine.state.lock().unwrap().idle(),
        "nothing was queued to replay later"
    );
}

#[tokio::test]
async fn a_failed_save_lands_when_the_server_recovers() {
    let server = MockServer::start();
    let mut broken = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/cat");
        then.status(500);
    });
    let engine = engine(&server);
    let path = RelPath::new("f");
    engine.local().write("f", b"precious");
    engine.created(&path);
    engine.modified(&path);
    engine.flush(Duration::from_millis(1200)).await;
    assert!(
        engine.ledger().dirty.contains(&path),
        "the debt survives the outage"
    );

    broken.delete();
    let save = server.mock(|when, then| {
        when.method(Method::POST).path("/api/files/cat");
        then.status(200);
    });
    settle(&engine).await;
    save.assert_hits(1);
    assert!(engine.ledger().dirty.is_empty());
}

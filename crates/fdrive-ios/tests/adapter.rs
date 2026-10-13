use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use fdrive_ios::adapter::{Adapter, EntryKind};
use fdrive_ios::FsError;
use httpmock::prelude::*;

fn block<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "fdrive-ios-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn adapter(server: &MockServer, data: &TempDir) -> Arc<Adapter> {
    Adapter::new(
        data.0.to_str().unwrap().into(),
        server.base_url(),
        "TOKEN".into(),
    )
    .unwrap()
}

#[test]
fn ls_maps_entries() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/api/files/ls")
            .query_param("path", "/docs/")
            .header("Authorization", "Bearer TOKEN");
        then.status(200).json_body(serde_json::json!({
            "status": "ok",
            "results": [
                {"name": "report.pdf", "size": 1024, "time": 1700000000000i64, "type": "file"},
                {"name": "archive", "size": 0, "time": 0, "type": "directory"},
            ]
        }));
    });

    let data = TempDir::new();
    let entries = block(adapter(&server, &data).ls("/docs/".into())).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "report.pdf");
    assert_eq!(entries[0].kind, EntryKind::File);
    assert_eq!(entries[0].size, Some(1024));
    assert_eq!(entries[0].mtime_ms, Some(1700000000000));
    assert_eq!(entries[1].kind, EntryKind::Directory);
    assert_eq!(entries[1].mtime_ms, None, "time=0 means unknown");
}

#[test]
fn ls_overlays_local_only_files() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/files/ls");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok", "results": []}));
    });

    let data = TempDir::new();
    let adapter = adapter(&server, &data);
    adapter.create("/draft.txt".into()).unwrap();
    let entries = block(adapter.ls("/".into())).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "draft.txt");
}

#[test]
fn open_caches_the_download() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("HEAD")
            .path("/api/files/cat")
            .query_param("path", "/hello.txt");
        then.status(200)
            .header("Content-Length", "11")
            .header("Last-Modified", "Wed, 21 Oct 2015 07:28:00 GMT");
    });
    let cat = server.mock(|when, then| {
        when.method(GET)
            .path("/api/files/cat")
            .query_param("path", "/hello.txt");
        then.status(200)
            .body("hello world")
            .header("Last-Modified", "Wed, 21 Oct 2015 07:28:00 GMT");
    });

    let data = TempDir::new();
    let adapter = adapter(&server, &data);
    let local = block(adapter.open("/hello.txt".into())).unwrap();
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "hello world");
    let again = block(adapter.open("/hello.txt".into())).unwrap();
    assert_eq!(again, local);
    cat.assert_hits(1);
}

#[test]
fn saved_uploads_the_edit() {
    let server = MockServer::start();
    let save = server.mock(|when, then| {
        when.method(POST)
            .path("/api/files/cat")
            .query_param("path", "/note.txt");
        then.status(200);
    });

    let data = TempDir::new();
    let adapter = adapter(&server, &data);
    let local = adapter.create("/note.txt".into()).unwrap();
    std::fs::write(&local, b"jotted").unwrap();
    adapter.saved("/note.txt".into());
    block(adapter.flush(5_000));
    save.assert_hits(1);
}

#[test]
fn a_dir_delete_is_a_verdict_a_file_delete_is_not() {
    let server = MockServer::start();
    let rm = server.mock(|when, then| {
        when.method(POST)
            .path("/api/files/rm")
            .query_param("path", "/docs/");
        then.status(200)
            .json_body(serde_json::json!({"status": "ok"}));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/api/files/rm")
            .query_param("path", "/broken/");
        then.status(500);
    });

    let data = TempDir::new();
    let adapter = adapter(&server, &data);
    block(adapter.delete("/docs/".into())).unwrap();
    rm.assert_hits(1);
    assert!(block(adapter.delete("/broken/".into())).is_err());
    block(adapter.delete("/locked.txt".into())).unwrap();
}

#[test]
fn errors_are_mapped() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/api/files/ls")
            .query_param("path", "/gone/");
        then.status(404);
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/api/files/ls")
            .query_param("path", "/secret/");
        then.status(403);
    });

    let data = TempDir::new();
    let adapter = adapter(&server, &data);
    assert!(matches!(
        block(adapter.ls("/gone/".into())).unwrap_err(),
        FsError::NotFound
    ));
    assert!(matches!(
        block(adapter.ls("/secret/".into())).unwrap_err(),
        FsError::PermissionDenied
    ));
}

#[test]
#[ignore]
fn against_real_server() {
    let env = |name: &str| std::env::var(name).expect(name);
    let data = TempDir::new();
    let adapter = Adapter::new(
        data.0.to_str().unwrap().into(),
        env("FILESTASH_URL"),
        env("FILESTASH_TOKEN"),
    )
    .unwrap();
    let entries = block(adapter.ls("/".into())).unwrap();
    println!("ls / -> {} entries", entries.len());
    if let Some(file) = entries.iter().find(|e| e.kind == EntryKind::File) {
        let local = block(adapter.open(format!("/{}", file.name))).unwrap();
        println!("cached {} at {local}", file.name);
    }
}

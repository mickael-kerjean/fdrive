use httpmock::prelude::*;

use super::{Error, FileType};
use crate::remote::Remote;

#[tokio::test]
async fn http_errors_are_mapped() {
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

    let client = Remote::builder(&std::env::temp_dir())
        .url(server.base_url())
        .token("TOKEN".into())
        .build()
        .unwrap();
    assert!(matches!(
        client.ls("/gone/").await.unwrap_err(),
        Error::NotFound
    ));
    assert!(matches!(
        client.ls("/secret/").await.unwrap_err(),
        Error::PermissionDenied
    ));
}

#[tokio::test]
async fn a_zero_time_means_unknown_mtime() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/files/ls");
        then.status(200).json_body(serde_json::json!({
            "status": "ok",
            "results": [{"name": "archive", "size": 0, "time": 0, "type": "directory"}]
        }));
    });

    let client = Remote::builder(&std::env::temp_dir())
        .url(server.base_url())
        .token("TOKEN".into())
        .build()
        .unwrap();
    let files = client.ls("/").await.unwrap();
    assert_eq!(files[0].kind, FileType::Directory);
    assert_eq!(files[0].mtime, None);
}

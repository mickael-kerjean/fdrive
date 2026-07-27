use httpmock::prelude::*;

use super::{Error, FileType, Sdk};

#[tokio::test]
async fn authenticate_reassembles_split_cookies() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/api/session/auth/");
        then.status(302)
            .header("Set-Cookie", "auth=part1; Path=/; HttpOnly")
            .header("Set-Cookie", "auth1=part2; Path=/; HttpOnly");
    });

    let mut client = Sdk::new(&server.base_url()).unwrap();
    client
        .authenticate("alice", "secret", "my-storage")
        .await
        .unwrap();
    assert_eq!(client.token(), Some("part1part2"));
}

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

    let mut client = Sdk::new(&server.base_url()).unwrap();
    client.set_token("TOKEN".into());
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

    let mut client = Sdk::new(&server.base_url()).unwrap();
    client.set_token("TOKEN".into());
    let files = client.ls("/").await.unwrap();
    assert_eq!(files[0].kind, FileType::Directory);
    assert_eq!(files[0].mtime, None);
}

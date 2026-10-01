use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum Request {
    Status,
    Login {
        server: String,
        token: String,
    },
    Logout,
    Clear,
}

pub fn socket_path() -> std::io::Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .ok_or_else(|| std::io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    Ok(PathBuf::from(dir).join("fdrive-omarchy.sock"))
}

pub async fn call(socket: &Path, request: &Request) -> std::io::Result<Value> {
    let stream = UnixStream::connect(socket).await?;
    let (read, mut write) = stream.into_split();
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    write.write_all(line.as_bytes()).await?;
    write.shutdown().await?;
    let mut reply = String::new();
    BufReader::new(read).read_line(&mut reply).await?;
    serde_json::from_str(&reply).map_err(std::io::Error::other)
}

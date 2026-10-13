mod error;
mod file;
mod filestash;

use std::path::Path;
use std::time::SystemTime;

use self::filestash::Filestash;
use crate::ByteStream;

pub use self::{
    error::{Error, Result},
    file::{FileInfo, FileType, Permissions},
    filestash::{assemble_token, login_url, CatDelta, WatchEvent, WatchMessage, WatchStream},
};

pub enum Remote {
    Filestash(Filestash),
}

impl Remote {
    pub fn builder(data: &Path) -> RemoteBuilder {
        let session = crate::config::load(data);
        RemoteBuilder {
            url: session.url,
            token: session.token,
            insecure: session.insecure,
        }
    }

    pub fn insecure(&self) -> bool {
        match self {
            Self::Filestash(remote) => remote.insecure(),
        }
    }

    pub async fn probe(&self) -> Result<String> {
        match self {
            Self::Filestash(remote) => remote.probe().await,
        }
    }

    pub async fn logout(&self) -> Result<()> {
        match self {
            Self::Filestash(remote) => remote.logout().await,
        }
    }

    pub async fn ls(&self, path: &str) -> Result<Vec<FileInfo>> {
        match self {
            Self::Filestash(remote) => remote.ls(path).await,
        }
    }

    pub async fn search(&self, path: &str, query: &str) -> Result<Vec<(String, FileInfo)>> {
        match self {
            Self::Filestash(remote) => remote.search(path, query).await,
        }
    }

    pub async fn stat(&self, path: &str) -> Result<FileInfo> {
        match self {
            Self::Filestash(remote) => remote.stat(path).await,
        }
    }

    pub async fn cat(&self, path: &str) -> Result<(FileInfo, ByteStream)> {
        match self {
            Self::Filestash(remote) => remote.cat(path).await,
        }
    }

    pub async fn cat_range(&self, path: &str, start: u64, end: u64) -> Result<Vec<u8>> {
        match self {
            Self::Filestash(remote) => remote.cat_range(path, start, end).await,
        }
    }

    pub async fn thumbnail(&self, path: &str) -> Result<Vec<u8>> {
        match self {
            Self::Filestash(remote) => remote.thumbnail(path).await,
        }
    }

    pub async fn save(&self, path: &str, body: ByteStream, since: Option<SystemTime>) -> Result<Option<SystemTime>> {
        match self {
            Self::Filestash(remote) => remote.save(path, body, since).await,
        }
    }

    pub async fn mkdir(&self, path: &str) -> Result<()> {
        match self {
            Self::Filestash(remote) => remote.mkdir(path).await,
        }
    }

    pub async fn rm(&self, path: &str) -> Result<()> {
        match self {
            Self::Filestash(remote) => remote.rm(path).await,
        }
    }

    pub async fn mv(&self, from: &str, to: &str) -> Result<()> {
        match self {
            Self::Filestash(remote) => remote.mv(from, to).await,
        }
    }

    pub async fn delta_supported(&self) -> bool {
        match self {
            Self::Filestash(remote) => remote.delta_supported().await,
        }
    }

    pub async fn cat_delta(&self, path: &str) -> Result<CatDelta> {
        match self {
            Self::Filestash(remote) => remote.cat_delta(path).await,
        }
    }

    pub async fn save_delta(&self, path: &str, body: Vec<u8>, since: SystemTime, base: Option<String>) -> Result<Option<SystemTime>> {
        match self {
            Self::Filestash(remote) => remote.save_delta(path, body, since, base).await,
        }
    }

    pub async fn watch(&self, cursor: Option<&str>) -> Result<WatchStream> {
        match self {
            Self::Filestash(remote) => remote.watch(cursor).await,
        }
    }
}

pub struct RemoteBuilder {
    url: String,
    token: String,
    insecure: bool,
}

impl RemoteBuilder {
    pub fn url(mut self, url: String) -> Self {
        self.url = url;
        self
    }

    pub fn token(mut self, token: String) -> Self {
        self.token = token;
        self
    }

    pub fn insecure(mut self, insecure: bool) -> Self {
        self.insecure = insecure;
        self
    }

    pub fn build(self) -> Result<Remote> {
        Ok(Remote::Filestash(Filestash::new(&self.url, self.token, self.insecure)?))
    }
}

pub fn normalize_server(input: &str) -> String {
    let input = input.trim();
    let url = match input.contains("://") {
        true => input.to_owned(),
        false => format!("https://{input}"),
    };
    url.trim_end_matches('/').to_owned()
}

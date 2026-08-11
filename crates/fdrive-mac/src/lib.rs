use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use fdrive_core::engine::{Engine, Observation};
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::{self, Sdk};
use tokio::runtime::Runtime;

pub mod activity;

uniffi::setup_scaffolding!();

const META_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FsError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("not authenticated")]
    NotAuthenticated,
    #[error("permission denied")]
    PermissionDenied,
    #[error("not found")]
    NotFound,
    #[error("network error: {msg}")]
    Network { msg: String },
    #[error("{msg}")]
    Other { msg: String },
}

impl From<sdk::Error> for FsError {
    fn from(error: sdk::Error) -> Self {
        match error {
            sdk::Error::InvalidCredentials => Self::InvalidCredentials,
            sdk::Error::NotAuthenticated => Self::NotAuthenticated,
            sdk::Error::PermissionDenied => Self::PermissionDenied,
            sdk::Error::NotFound => Self::NotFound,
            sdk::Error::Http(error) => Self::Network { msg: error.to_string() },
            error => Self::Other { msg: error.to_string() },
        }
    }
}

impl From<io::Error> for FsError {
    fn from(error: io::Error) -> Self {
        Self::Other { msg: error.to_string() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub mtime_ms: Option<i64>,
}

impl From<sdk::FileInfo> for Entry {
    fn from(info: sdk::FileInfo) -> Self {
        Self {
            name: info.name,
            kind: match info.kind {
                sdk::FileType::File => EntryKind::File,
                sdk::FileType::Directory => EntryKind::Directory,
            },
            size: info.size,
            mtime_ms: info
                .mtime
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64),
        }
    }
}

#[uniffi::export]
pub fn login(url: String, insecure: bool, user: String, password: String, storage: String) -> Result<String, FsError> {
    let runtime = runtime()?;
    let sdk = runtime.block_on(Sdk::builder(&url).insecure(insecure).login(&user, &password, &storage))?;
    Ok(sdk.token().unwrap_or_default().to_string())
}

#[uniffi::export]
pub fn ping(url: String, insecure: bool, token: String) -> bool {
    let Ok(runtime) = runtime() else { return false };
    let Ok(sdk) = Sdk::builder(&url).insecure(insecure).token(token) else {
        return false;
    };
    runtime.block_on(sdk.ls("/")).is_ok()
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Session {
    pub url: String,
    pub token: String,
    pub insecure: bool,
    pub ok: bool,
}

#[uniffi::export]
pub fn normalize_server(input: String) -> String {
    sdk::normalize_server(&input)
}

#[uniffi::export(async_runtime = "tokio")]
pub async fn probe(url: String, insecure: bool) -> Result<String, FsError> {
    Ok(Sdk::builder(&url).insecure(insecure).probe().await?)
}

#[uniffi::export]
pub fn logout(url: String, insecure: bool, token: String) -> Result<(), FsError> {
    let runtime = runtime()?;
    let sdk = Sdk::builder(&url).insecure(insecure).token(token)?;
    Ok(runtime.block_on(sdk.logout())?)
}

#[uniffi::export]
pub fn assemble_token(cookies: HashMap<String, String>) -> String {
    let cookies: Vec<(String, String)> = cookies.into_iter().collect();
    sdk::assemble_token(&cookies)
}

#[uniffi::export]
pub fn session_recall(data_dir: String) -> Session {
    let session = fdrive_core::config::load(Path::new(&data_dir));
    let ok = session.ok();
    Session { url: session.url, token: session.token, insecure: session.insecure, ok }
}

#[uniffi::export]
pub fn session_remember(data_dir: String, url: String, token: String, insecure: bool) {
    fdrive_core::config::remember(Path::new(&data_dir), &url, &token, insecure);
}

#[uniffi::export]
pub fn session_forget(data_dir: String) {
    fdrive_core::config::forget(Path::new(&data_dir));
}

struct MacTree {
    cache_dir: PathBuf,
    ledger: PathBuf,
    meta: Mutex<HashMap<RelPath, (Instant, Vec<sdk::FileInfo>)>>,
}

impl MacTree {
    fn invalidate(&self, directory: &RelPath) {
        self.meta.lock().unwrap().remove(directory);
    }
}

impl LocalStore for MacTree {
    fn backing(&self, path: &RelPath) -> PathBuf {
        self.cache_dir.join(path.as_str())
    }

    fn relocate(&self, from: &RelPath, to: &RelPath) -> io::Result<()> {
        let destination = self.backing(to);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(self.backing(from), destination)
    }

    fn settled(&self, target: &RelPath, _mtime: Option<std::time::SystemTime>) {
        self.invalidate(&target.parent_or_root());
    }

    fn ledger(&self) -> PathBuf {
        self.ledger.clone()
    }
}

#[derive(uniffi::Object)]
pub struct Adapter {
    runtime: Runtime,
    engine: Arc<Engine<MacTree>>,
}

#[uniffi::export]
impl Adapter {
    #[uniffi::constructor]
    pub fn new(url: String, insecure: bool, token: String, data_dir: String) -> Result<Arc<Self>, FsError> {
        let sdk = Sdk::builder(&url).insecure(insecure).token(token)?;
        let runtime = runtime()?;
        let data_dir = PathBuf::from(data_dir);
        let cache_dir = data_dir.join("cache");
        fs::create_dir_all(&cache_dir)?;
        let tree = MacTree {
            cache_dir: cache_dir.clone(),
            ledger: data_dir.join("fdrive.db"),
            meta: Mutex::new(HashMap::new()),
        };
        let engine = Engine::start(runtime.handle().clone(), Arc::new(sdk), tree);
        engine.prune(&cache_dir)?;
        engine.recover();
        Ok(Arc::new(Self { runtime, engine }))
    }

    pub fn ls(&self, path: String) -> Result<Vec<Entry>, FsError> {
        let path = RelPath::new(&path);
        Ok(self.listing(&path)?.into_iter().map(Entry::from).collect())
    }

    pub fn stat(&self, path: String) -> Result<Entry, FsError> {
        let path = RelPath::new(&path);
        if let Some(metadata) = self.engine.dirty_metadata(&path) {
            return Ok(Entry {
                name: path.name().to_string(),
                kind: EntryKind::File,
                size: Some(metadata.len()),
                mtime_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis() as i64),
            });
        }
        self.listing(&path.parent_or_root())?
            .into_iter()
            .find(|entry| entry.name == path.name())
            .map(Entry::from)
            .ok_or(FsError::NotFound)
    }

    pub fn open(&self, path: String) -> Result<String, FsError> {
        let path = RelPath::new(&path);
        let mut current = None;
        if let Ok(listing) = self.listing(&path.parent_or_root()) {
            if let Some(entry) = listing.iter().find(|entry| entry.name == path.name()) {
                let observation = Observation::of(entry);
                if self.engine.content_current(&path, observation) {
                    return Ok(self.local_path(&path));
                }
                current = Some(observation);
            }
        }
        self.runtime.block_on(self.engine.hydrate(&path, current))?;
        Ok(self.local_path(&path))
    }

    pub fn thumbnail(&self, path: String) -> Result<Vec<u8>, FsError> {
        let path = RelPath::new(&path);
        Ok(self.runtime.block_on(self.engine.sdk().thumbnail(&path.as_file()))?)
    }

    pub fn create(&self, path: String) -> Result<String, FsError> {
        let path = RelPath::new(&path);
        let local = self.engine.local().backing(&path);
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create(&local)?;
        self.engine.created(&path);
        self.engine.local().invalidate(&path.parent_or_root());
        Ok(self.local_path(&path))
    }

    pub fn saved(&self, path: String) {
        let path = RelPath::new(&path);
        self.engine.modified(&path);
        self.engine.released(&path);
    }

    pub fn flush(&self, timeout_ms: u64) {
        self.runtime.block_on(self.engine.flush(Duration::from_millis(timeout_ms)));
    }

    pub fn mkdir(&self, path: String) -> Result<(), FsError> {
        let path = RelPath::new(&path);
        self.runtime.block_on(self.engine.sdk().mkdir(&path.as_dir()))?;
        self.engine.local().invalidate(&path.parent_or_root());
        Ok(())
    }

    pub fn delete(&self, path: String) -> Result<(), FsError> {
        let is_directory = path.ends_with('/');
        let path = RelPath::new(&path);
        self.runtime.block_on(self.engine.delete(&path, is_directory))?;
        let local = self.engine.local().backing(&path);
        let _ = if is_directory {
            fs::remove_dir_all(local)
        } else {
            fs::remove_file(local)
        };
        self.engine.local().invalidate(&path);
        self.engine.local().invalidate(&path.parent_or_root());
        Ok(())
    }

    pub fn rename(&self, from: String, to: String) -> Result<(), FsError> {
        let is_directory = from.ends_with('/');
        let from = RelPath::new(&from);
        let to = RelPath::new(&to);
        self.runtime.block_on(self.engine.rename(&from, &to, is_directory))?;
        let local = self.engine.local().backing(&from);
        if local.exists() {
            self.engine.local().relocate(&from, &to)?;
        }
        self.engine.local().invalidate(&from.parent_or_root());
        self.engine.local().invalidate(&to.parent_or_root());
        Ok(())
    }
}

impl Adapter {
    fn listing(&self, directory: &RelPath) -> Result<Vec<sdk::FileInfo>, FsError> {
        let cached = {
            let metadata = self.engine.local().meta.lock().unwrap();
            metadata
                .get(directory)
                .filter(|(created, _)| created.elapsed() < META_TTL)
                .map(|(_, listing)| listing.clone())
        };
        let listing = match cached {
            Some(listing) => listing,
            None => {
                let listing = self.runtime.block_on(self.engine.sdk().ls(&directory.as_dir()))?;
                self.engine.listed(directory, &listing);
                self.engine
                    .local()
                    .meta
                    .lock()
                    .unwrap()
                    .insert(directory.clone(), (Instant::now(), listing.clone()));
                listing
            }
        };
        Ok(self.engine.overlay(directory, listing))
    }

    fn local_path(&self, path: &RelPath) -> String {
        self.engine.local().backing(path).to_string_lossy().into_owned()
    }
}

fn runtime() -> Result<Runtime, FsError> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| FsError::Other { msg: error.to_string() })
}

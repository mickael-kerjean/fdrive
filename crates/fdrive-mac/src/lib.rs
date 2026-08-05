use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use fdrive_core::engine::{Engine, Observation};
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::{self, Sdk};
use tokio::runtime::Runtime;

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

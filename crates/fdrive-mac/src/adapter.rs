use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use fdrive_core::engine::{Engine, Observation, Watch};
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::{self, Sdk};
use tokio::runtime::Runtime;
use tokio::sync::Mutex as AsyncMutex;

use crate::{runtime, FsError};

const VIEWER_META_TTL: Duration = Duration::from_secs(5);
const BACKGROUND_META_TTL: Duration = Duration::from_secs(60 * 60);

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
    pub time_local: Option<i64>,
    pub time_remote: Option<i64>,
    pub can_read: bool,
    pub can_write: bool,
    pub can_rename: bool,
    pub can_reparent: bool,
    pub can_delete: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Download {
    pub local_path: String,
    pub entry: Entry,
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
            time_local: None,
            time_remote: info
                .mtime
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64),
            can_read: info.perms.read,
            can_write: info.perms.write,
            can_rename: info.perms.rename,
            can_reparent: info.perms.reparent,
            can_delete: info.perms.delete,
        }
    }
}

pub(crate) struct CacheTree {
    cache_dir: PathBuf,
    ledger: PathBuf,
    meta: Arc<Mutex<HashMap<RelPath, MetadataEntry>>>,
}

#[derive(Default)]
struct MetadataEntry {
    generation: u64,
    listing: Option<(Instant, Vec<sdk::FileInfo>)>,
}

impl MetadataEntry {
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.listing = None;
    }
}

impl CacheTree {
    fn invalidate(&self, directory: &RelPath) {
        let mut meta = self.meta.lock().unwrap();
        if let Some(entry) = meta.get_mut(directory) {
            entry.invalidate();
        }
    }
}

impl LocalStore for CacheTree {
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
    watcher: Mutex<Option<Watch>>,
    listing_gates: Mutex<HashMap<RelPath, Arc<AsyncMutex<()>>>>,
    _runtime: Runtime,
    pub(crate) engine: Arc<Engine<CacheTree>>,
}

#[uniffi::export(callback_interface)]
pub trait RemoteObserver: Send + Sync {
    fn changed(&self, directories: Vec<String>);
}

#[uniffi::export(async_runtime = "tokio")]
impl Adapter {
    #[uniffi::constructor]
    pub fn new(url: String, insecure: bool, token: String, data_dir: String) -> Result<Arc<Self>, FsError> {
        let sdk = Sdk::builder(&url).insecure(insecure).token(token)?;
        let runtime = runtime()?;
        let data_dir = PathBuf::from(data_dir);
        let cache_dir = data_dir.join("cache");
        fs::create_dir_all(&cache_dir)?;
        crate::log::init();
        let tree = CacheTree {
            cache_dir: cache_dir.clone(),
            ledger: data_dir.join("fdrive.db"),
            meta: Arc::new(Mutex::new(HashMap::new())),
        };
        let engine = Engine::start(runtime.handle().clone(), Arc::new(sdk), tree);
        engine.cache().evict(&cache_dir)?;
        engine.system().recover();
        Ok(Arc::new(Self {
            watcher: Mutex::new(None),
            listing_gates: Mutex::new(HashMap::new()),
            _runtime: runtime,
            engine,
        }))
    }

    pub fn start_watch(&self, observer: Box<dyn RemoteObserver>) {
        let meta = self.engine.local().meta.clone();
        let mut watcher = self.watcher.lock().unwrap();
        watcher.take();
        *watcher = Some(self.engine.watch(move |changes| {
            {
                let mut meta = meta.lock().unwrap();
                for (directory, entry) in meta.iter_mut() {
                    if changes.affects(directory) {
                        entry.invalidate();
                    }
                }
            }
            observer.changed(changes.directories().map(RelPath::as_dir).collect());
        }));
    }

    pub fn stop_watch(&self) {
        self.watcher.lock().unwrap().take();
    }

    pub async fn ls(&self, path: String, viewer_request: bool) -> Result<Vec<Entry>, FsError> {
        let dir = RelPath::new(&path);
        Ok(self
            .listing(&dir, viewer_request)
            .await?
            .into_iter()
            .map(|info| self.entry(&dir.join(&info.name), info))
            .collect())
    }

    pub async fn stat(&self, path: String, viewer_request: bool) -> Result<Entry, FsError> {
        let path = RelPath::new(&path);
        if let Some(metadata) = self.engine.view().pending_metadata(&path) {
            let mut entry = Entry::from(sdk::FileInfo {
                name: path.name().to_string(),
                kind: sdk::FileType::File,
                size: Some(metadata.len()),
                mtime: metadata.modified().ok(),
                perms: sdk::Permissions::default(),
            });
            entry.time_local = entry.time_remote.take();
            return Ok(entry);
        }
        self.listing(&path.parent_or_root(), viewer_request)
            .await?
            .into_iter()
            .find(|entry| entry.name == path.name())
            .map(|info| self.entry(&path, info))
            .ok_or(FsError::NotFound)
    }

    pub async fn open(&self, path: String, base: Option<String>, viewer_request: bool) -> Result<String, FsError> {
        Ok(self.fetch(path, base, viewer_request).await?.local_path)
    }

    pub async fn fetch(&self, path: String, base: Option<String>, viewer_request: bool) -> Result<Download, FsError> {
        let path = RelPath::new(&path);
        let mut current = None;
        let mut permissions = sdk::Permissions::default();
        if let Ok(listing) = self.listing(&path.parent_or_root(), viewer_request).await {
            if let Some(entry) = listing.iter().find(|entry| entry.name == path.name()) {
                current = Some(Observation::of(entry));
                permissions = entry.perms;
            }
        }
        let observation = self.engine
            .cache()
            .hydrate_observed(&path, current, base.map(PathBuf::from))
            .await?;
        let entry = Entry::from(sdk::FileInfo {
            name: path.name().to_owned(),
            kind: sdk::FileType::File,
            size: Some(observation.size),
            mtime: (observation.time != 0).then(|| UNIX_EPOCH + Duration::from_secs(observation.time)),
            perms: permissions,
        });
        Ok(Download { local_path: self.local_path(&path), entry })
    }

    pub fn cancel(&self, path: String) {
        self.engine.cache().cancel(&RelPath::new(&path));
    }

    pub async fn thumbnail(&self, path: String) -> Result<Vec<u8>, FsError> {
        let path = RelPath::new(&path);
        Ok(self.engine.fs().thumbnail(&path).await?)
    }

    pub fn create(&self, path: String) -> Result<String, FsError> {
        let path = RelPath::new(&path);
        let local = self.engine.local().backing(&path);
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create(&local)?;
        self.engine.fs().created(&path);
        self.engine.local().invalidate(&path.parent_or_root());
        Ok(self.local_path(&path))
    }

    pub fn saved(&self, path: String) {
        let path = RelPath::new(&path);
        self.engine.fs().modified(&path);
        self.engine.fs().released(&path);
    }

    pub async fn flush(&self, timeout_ms: u64) {
        self.engine.system().flush(Duration::from_millis(timeout_ms)).await;
    }

    pub async fn mkdir(&self, path: String) -> Result<(), FsError> {
        let path = RelPath::new(&path);
        self.engine.fs().mkdir(&path).await?;
        self.engine.local().invalidate(&path.parent_or_root());
        Ok(())
    }

    pub async fn delete(&self, path: String) -> Result<(), FsError> {
        let is_directory = path.ends_with('/');
        let path = RelPath::new(&path);
        self.engine.fs().delete(&path, is_directory).await?;
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

    pub async fn rename(&self, from: String, to: String) -> Result<(), FsError> {
        let is_directory = from.ends_with('/');
        let from = RelPath::new(&from);
        let to = RelPath::new(&to);
        self.engine.fs().rename(&from, &to, is_directory).await?;
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
    fn entry(&self, path: &RelPath, info: sdk::FileInfo) -> Entry {
        if info.kind != sdk::FileType::File {
            return Entry::from(info);
        }
        if !self.engine.view().current(path, Observation::of(&info)) {
            return Entry::from(info);
        }
        let mut entry = Entry::from(info);
        if let Ok(modified) = fs::metadata(self.engine.local().backing(path))
            .and_then(|metadata| metadata.modified())
        {
            entry.time_local = modified
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|duration| duration.as_millis() as i64);
        }
        entry
    }

    fn listing_gate(&self, directory: &RelPath) -> Arc<AsyncMutex<()>> {
        let mut gates = self.listing_gates.lock().unwrap();
        gates.retain(|_, gate| Arc::strong_count(gate) > 1);
        gates.entry(directory.clone()).or_default().clone()
    }

    async fn listing(&self, directory: &RelPath, viewer_request: bool) -> Result<Vec<sdk::FileInfo>, FsError> {
        let ttl = if viewer_request { VIEWER_META_TTL } else { BACKGROUND_META_TTL };
        let gate = self.listing_gate(directory);
        let _guard = gate.lock().await;
        let mut retried = false;
        loop {
            let (generation, cached) = {
                let mut metadata = self.engine.local().meta.lock().unwrap();
                let entry = metadata.entry(directory.clone()).or_default();
                (
                    entry.generation,
                    entry
                        .listing
                        .as_ref()
                        .filter(|(created, _)| created.elapsed() < ttl)
                        .map(|(_, listing)| listing.clone()),
                )
            };
            if let Some(listing) = cached {
                return Ok(self.engine.view().merge(directory, listing));
            }
            let listing = self.engine.fs().ls(directory).await?;
            {
                let mut metadata = self.engine.local().meta.lock().unwrap();
                let entry = metadata.entry(directory.clone()).or_default();
                if entry.generation == generation {
                    entry.listing = Some((Instant::now(), listing.clone()));
                } else if !retried {
                    retried = true;
                    continue;
                }
            }
            self.engine.view().note(directory, &listing);
            return Ok(self.engine.view().merge(directory, listing));
        }
    }

    fn local_path(&self, path: &RelPath) -> String {
        self.engine.local().backing(path).to_string_lossy().into_owned()
    }
}

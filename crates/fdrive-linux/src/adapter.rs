use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use fdrive_core::engine::{Engine, RemoteChanges, Watch};
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::{FileInfo, Sdk};

use crate::xattr::XattrDb;

mod cache;
mod deletes;
mod fs;
mod system;
mod utils;
mod xattr;

pub use cache::Cache;
pub use fs::Fs;
pub use system::System;
pub use xattr::Xattr;

const META_TTL: Duration = Duration::from_secs(5);

pub struct Adapter {
    engine: Arc<Engine<CacheTree>>,
    xattrs: XattrDb,
    handles: Handles,
    deletes: Arc<tokio::sync::Mutex<deletes::Deletes>>,
}

pub struct CacheTree {
    cache_dir: PathBuf,
    ledger: PathBuf,
    meta: Arc<Mutex<HashMap<RelPath, MetadataEntry>>>,
}

#[derive(Default)]
struct MetadataEntry {
    generation: u64,
    listing: Option<(Instant, BTreeMap<String, FileInfo>)>,
    removed: BTreeMap<String, bool>, // name -> was a directory
    deleting: Option<Instant>,
}

impl MetadataEntry {
    fn cached(&self) -> Option<&BTreeMap<String, FileInfo>> {
        self.listing
            .as_ref()
            .filter(|(at, _)| at.elapsed() < META_TTL || self.deleting.is_some_and(|at| at.elapsed() < META_TTL))
            .map(|(_, listing)| listing)
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some((at, _)) = &mut self.listing {
            *at = Instant::now() - META_TTL;
        }
    }
}

#[derive(Default)]
struct Handles(Mutex<HandleTable>);

#[derive(Default)]
struct HandleTable {
    open: HashMap<u64, Handle>,
    next: u64,
}

struct Handle {
    path: RelPath,
    file: Option<Arc<std::fs::File>>,
    writable: bool,
}

impl LocalStore for CacheTree {
    fn backing(&self, path: &RelPath) -> PathBuf {
        self.cache_dir.join(path.as_str())
    }

    fn relocate(&self, from: &RelPath, to: &RelPath) -> io::Result<()> {
        let to_backing = self.backing(to);
        utils::ensure_parent(&to_backing)?;
        std::fs::rename(self.backing(from), to_backing)
    }

    fn settled(&self, target: &RelPath, _mtime: Option<SystemTime>) {
        self.invalidate(&target.parent_or_root());
    }

    fn ledger(&self) -> PathBuf {
        self.ledger.clone()
    }
}

impl CacheTree {
    fn invalidate(&self, dir: &RelPath) {
        if let Some(entry) = self.meta.lock().unwrap().get_mut(dir) {
            entry.invalidate();
        }
    }

    fn drop(&self, dir: &RelPath, name: &str, is_dir: bool) {
        let mut meta = self.meta.lock().unwrap();
        let entry = meta.entry(dir.clone()).or_default();
        entry.generation = entry.generation.wrapping_add(1);
        entry.removed.insert(name.to_owned(), is_dir);
        if let Some((_, listing)) = &mut entry.listing {
            listing.remove(name);
        }
        // Watch invalidations remain recorded, but don't refetch ancestors
        // between every unlink during an active recursive deletion.
        let mut dir = dir.clone();
        loop {
            if let Some(entry) = meta.get_mut(&dir) {
                entry.deleting = Some(Instant::now());
            }
            if dir.is_root() {
                break;
            }
            dir = dir.parent_or_root();
        }
    }

    fn created(&self, path: &RelPath) {
        let mut meta = self.meta.lock().unwrap();
        if let Some(entry) = meta.get_mut(&path.parent_or_root()) {
            if entry.removed.remove(path.name()).is_some() {
                entry.deleting = None;
                entry.invalidate();
            }
        }
    }
}

impl Adapter {
    pub fn new(rt: tokio::runtime::Handle, sdk: Arc<Sdk>, data_dir: &Path) -> io::Result<Self> {
        let cache_dir = data_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)?;
        let tree = CacheTree {
            cache_dir,
            ledger: data_dir.join("fdrive.db"),
            meta: Arc::new(Mutex::new(HashMap::new())),
        };
        let deletes = deletes::Deletes::open(data_dir.join("rmdir.json"))?;
        for path in &deletes.dirs {
            tree.drop(&path.parent_or_root(), path.name(), true);
        }
        let adapter = Self {
            engine: Engine::start(rt, sdk, tree),
            xattrs: XattrDb::open(data_dir.join("xattr.json")),
            handles: Handles::default(),
            deletes: Arc::new(tokio::sync::Mutex::new(deletes)),
        };
        adapter.prune()?;
        adapter.engine.system().recover();
        let engine = Arc::downgrade(&adapter.engine);
        let deletes = adapter.deletes.clone();
        adapter.engine.spawn(async move {
            loop {
                tokio::time::sleep(deletes::QUIET).await;
                let Some(engine) = engine.upgrade() else { break };
                let mut deletes = deletes.lock().await;
                if deletes.last.elapsed() >= deletes::QUIET {
                    if let Err(err) = deletes.flush(&engine).await {
                        log::error!("flush directory deletes: {err}");
                    }
                }
            }
        });
        Ok(adapter)
    }

    pub fn watch(&self, notify: impl Fn(RemoteChanges) + Send + 'static) -> Watch {
        let engine = self.engine.clone();
        self.engine.watch(move |changes| {
            for (directory, entry) in &mut *engine.local().meta.lock().unwrap() {
                if changes.affects(directory) {
                    entry.invalidate();
                }
            }
            notify(changes);
        })
    }

    pub fn fs(&self) -> Fs<'_> {
        Fs(self)
    }

    pub fn cache(&self) -> Cache<'_> {
        Cache(self)
    }

    pub fn system(&self) -> System<'_> {
        System(self)
    }

    pub fn status(&self) -> fdrive_core::engine::Status<'_, CacheTree> {
        self.engine.status()
    }

    fn prune(&self) -> io::Result<()> {
        self.engine.cache().evict(&self.engine.local().cache_dir)
    }

    fn entry(&self, path: &RelPath) -> io::Result<Option<FileInfo>> {
        let parent = path.parent_or_root();
        let cached = {
            let meta = self.engine.local().meta.lock().unwrap();
            meta.get(&parent)
                .and_then(MetadataEntry::cached)
                .map(|listing| listing.get(path.name()).cloned())
        };
        if let Some(entry) = cached {
            return Ok(self
                .engine
                .view()
                .merge(&parent, entry.into_iter().collect())
                .into_iter()
                .find(|e| e.name == path.name()));
        }
        match self.fs().ls(&parent) {
            Ok(listing) => Ok(listing.iter().find(|e| e.name == path.name()).cloned()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl Handles {
    fn open(&self, path: &RelPath, file: Option<Arc<std::fs::File>>, writable: bool) -> u64 {
        let mut t = self.0.lock().unwrap();
        t.next += 1;
        let fh = t.next;
        t.open.insert(
            fh,
            Handle {
                path: path.clone(),
                file,
                writable,
            },
        );
        fh
    }

    fn close(&self, fh: u64) -> Option<Handle> {
        self.0.lock().unwrap().open.remove(&fh)
    }

    fn get(&self, fh: u64) -> Option<Arc<std::fs::File>> {
        self.0.lock().unwrap().open.get(&fh)?.file.clone()
    }

    fn set(&self, fh: u64, file: Arc<std::fs::File>) {
        if let Some(handle) = self.0.lock().unwrap().open.get_mut(&fh) {
            handle.file = Some(file);
        }
    }
}

#[cfg(test)]
#[path = "adapter_test.rs"]
mod tests;

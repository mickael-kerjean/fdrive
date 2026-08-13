use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use fdrive_core::engine::Engine;
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::{FileInfo, Sdk};

use crate::xattr::XattrDb;

mod cache;
mod fs;
mod status;
mod system;
mod utils;
mod xattr;

pub use cache::Cache;
pub use fs::Fs;
pub use status::Status;
pub use system::System;
pub use xattr::Xattr;

pub struct Adapter {
    engine: Arc<Engine<CacheTree>>,
    xattrs: XattrDb,
    handles: Handles,
}

pub struct CacheTree {
    cache_dir: PathBuf,
    ledger: PathBuf,
    meta: Mutex<HashMap<RelPath, (Instant, Vec<FileInfo>)>>,
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
        self.meta.lock().unwrap().remove(dir);
    }

    fn drop(&self, dir: &RelPath, name: &str) {
        if let Some((_, listing)) = self.meta.lock().unwrap().get_mut(dir) {
            listing.retain(|e| e.name != name);
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
            meta: Mutex::new(HashMap::new()),
        };
        let adapter = Self {
            engine: Engine::start(rt, sdk, tree),
            xattrs: XattrDb::open(data_dir.join("xattr.json")),
            handles: Handles::default(),
        };
        adapter.prune()?;
        adapter.engine.recover();
        Ok(adapter)
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

    pub fn status(&self) -> Status<'_> {
        Status(self)
    }

    fn prune(&self) -> io::Result<()> {
        self.engine.prune(&self.engine.local().cache_dir)
    }

    fn entry(&self, path: &RelPath) -> io::Result<Option<FileInfo>> {
        let parent = path.parent_or_root();
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

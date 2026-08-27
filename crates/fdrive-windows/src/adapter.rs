use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use fdrive_core::engine::Engine;
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::Sdk;

use crate::wire;

mod cache;
mod fs;
mod pin;
mod reconcile;
mod system;

pub use cache::Cache;
pub use fs::Fs;
pub use system::System;
use reconcile::Reconcile;
pub use wire::pin::Pin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Dehydrated(Pin),
    Cached(Pin),
    Edited,
    New,
    Foreign,
}

pub struct PlaceholderTree {
    root: PathBuf,
    ledger: PathBuf,
    rt: tokio::runtime::Handle,
    suppressed: Mutex<BTreeMap<RelPath, usize>>,
}

impl PlaceholderTree {
    fn abs(&self, path: &RelPath) -> PathBuf {
        wire::abs_of(&self.root, path)
    }

    fn is_suppressed(&self, path: &RelPath) -> bool {
        self.suppressed
            .lock()
            .unwrap()
            .keys()
            .any(|p| p == path || path.is_descendant_of(p))
    }

    fn hold(&self, path: &RelPath) -> Hold<'_> {
        *self
            .suppressed
            .lock()
            .unwrap()
            .entry(path.clone())
            .or_insert(0) += 1;
        Hold {
            tree: self,
            path: path.clone(),
        }
    }

    fn suppress<T>(&self, path: &RelPath, op: impl FnOnce() -> T) -> T {
        let _hold = self.hold(path);
        op()
    }
}

struct Hold<'a> {
    tree: &'a PlaceholderTree,
    path: RelPath,
}

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        let mut suppressed = self.tree.suppressed.lock().unwrap();
        if let Some(n) = suppressed.get_mut(&self.path) {
            *n -= 1;
            if *n == 0 {
                suppressed.remove(&self.path);
            }
        }
    }
}

impl LocalStore for PlaceholderTree {
    fn backing(&self, path: &RelPath) -> PathBuf {
        self.abs(path)
    }

    fn relocate(&self, from: &RelPath, to: &RelPath) -> io::Result<()> {
        self.suppress(from, || std::fs::rename(self.abs(from), self.abs(to)))
    }

    fn settled(&self, target: &RelPath, mtime: Option<SystemTime>) {
        let abs = self.abs(target);
        let what = target.clone();
        self.rt.spawn_blocking(move || {
            if let Err(err) = wire::mark_in_sync_if_unmodified(&abs, &what, mtime) {
                log::debug!("mark in sync {what}: {err}");
            }
        });
    }

    fn ledger(&self) -> PathBuf {
        self.ledger.clone()
    }
}

pub struct Adapter {
    engine: Arc<Engine<PlaceholderTree>>,
    root: PathBuf,
    refreshing: Mutex<BTreeMap<RelPath, Instant>>,
    kept: Mutex<BTreeSet<RelPath>>,
    pinning: Mutex<BTreeSet<RelPath>>,
}

impl Adapter {
    pub fn new(
        sdk: Arc<Sdk>,
        rt: tokio::runtime::Handle,
        root: PathBuf,
        data: &Path,
    ) -> io::Result<Arc<Self>> {
        std::fs::create_dir_all(&root)?;
        Ok(Arc::new(Self {
            root: root.clone(),
            engine: Engine::start(
                rt.clone(),
                sdk,
                PlaceholderTree {
                    root,
                    ledger: data.join("fdrive.db"),
                    rt,
                    suppressed: Mutex::new(BTreeMap::new()),
                },
            ),
            refreshing: Mutex::new(BTreeMap::new()),
            kept: Mutex::new(BTreeSet::new()),
            pinning: Mutex::new(BTreeSet::new()),
        }))
    }

    pub fn fs(self: &Arc<Self>) -> Fs<'_> {
        Fs(self)
    }

    pub fn cache(self: &Arc<Self>) -> Cache<'_> {
        Cache(self)
    }

    pub fn system(self: &Arc<Self>) -> System<'_> {
        System(self)
    }

    pub fn status(self: &Arc<Self>) -> fdrive_core::engine::Status<'_, PlaceholderTree> {
        self.engine.status()
    }

    fn reconcile(self: &Arc<Self>) -> Reconcile<'_> {
        Reconcile(self)
    }

    fn abs(&self, path: &RelPath) -> PathBuf {
        wire::abs_of(&self.root, path)
    }
}

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use crate::path::RelPath;
use crate::port::LocalStore;

use super::{Reader, Engine};

#[derive(Default)]
pub(super) struct Transfers {
    pub(super) hydrating: Mutex<HashMap<RelPath, Arc<tokio::sync::Mutex<()>>>>,
    pub(super) downloads: Mutex<HashMap<RelPath, Arc<Reader>>>,
    pub(super) uploading: Mutex<HashMap<RelPath, Arc<tokio::sync::Mutex<()>>>>,
}

impl Transfers {
    pub(super) fn upload_gate(&self, path: &RelPath) -> Arc<tokio::sync::Mutex<()>> {
        gate(&self.uploading, path)
    }

    pub(super) fn hydrate_gate(&self, path: &RelPath) -> Arc<tokio::sync::Mutex<()>> {
        gate(&self.hydrating, path)
    }
}

fn gate(
    gates: &Mutex<HashMap<RelPath, Arc<tokio::sync::Mutex<()>>>>,
    path: &RelPath,
) -> Arc<tokio::sync::Mutex<()>> {
    let mut gates = gates.lock().unwrap();
    gates.retain(|_, gate| Arc::strong_count(gate) > 1);
    gates.entry(path.clone()).or_default().clone()
}

pub(super) struct Frozen<'a> {
    pub(super) set: &'a Mutex<BTreeSet<RelPath>>,
    pub(super) paths: Vec<RelPath>,
}

impl Drop for Frozen<'_> {
    fn drop(&mut self) {
        let mut set = self.set.lock().unwrap();
        for path in &self.paths {
            set.remove(path);
        }
    }
}

impl<T: LocalStore> Engine<T> {
    pub(super) fn freeze(&self, paths: &[&RelPath]) -> Frozen<'_> {
        let paths: Vec<RelPath> = paths.iter().map(|p| (*p).clone()).collect();
        let mut set = self.frozen.lock().unwrap();
        for path in &paths {
            set.insert(path.clone());
        }
        Frozen {
            set: &self.frozen,
            paths,
        }
    }

    pub(super) fn is_frozen(&self, path: &RelPath) -> bool {
        self.frozen
            .lock()
            .unwrap()
            .iter()
            .any(|p| path == p || path.is_descendant_of(p))
    }

    pub(super) async fn wait_uploads(&self, path: &RelPath, subtree: bool) {
        let gates: Vec<Arc<tokio::sync::Mutex<()>>> = self
            .transfers
            .uploading
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| *p == path || (subtree && p.is_descendant_of(path)))
            .map(|(_, gate)| gate.clone())
            .collect();
        for gate in gates {
            let _gate = gate.lock().await;
        }
    }
}

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::path::RelPath;
use crate::port::LocalStore;

use super::Engine;
use crate::model::Observation;

pub struct Cache<'a, T: LocalStore>(pub(super) &'a Engine<T>);

impl<T: LocalStore> Engine<T> {
    pub(super) fn pin_sweep(&self) {
        self.scheduler.sweep();
    }

    pub(super) async fn sweep_pins(self: Arc<Self>) {
        let roots: Vec<RelPath> = self.ledger().pins.iter().cloned().collect();
        for root in roots {
            self.hydrate_subtree(&root).await;
        }
    }

    pub(super) async fn hydrate_subtree(&self, root: &RelPath) {
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            let listing = match self.sdk.ls(&dir.as_dir()).await {
                Ok(listing) => listing,
                Err(_) if dir == *root => {
                    if let Err(err) = self.cache().hydrate(root, None, None).await {
                        log::debug!("pin {root}: {err}");
                    }
                    return;
                }
                Err(err) => {
                    log::debug!("pin {dir}: {err}");
                    continue;
                }
            };
            self.view().note(&dir, &listing);
            for entry in listing {
                let child = dir.join(&entry.name);
                if child.parent_or_root() != dir {
                    continue;
                }
                match entry.kind {
                    crate::sdk::FileType::Directory => dirs.push(child),
                    crate::sdk::FileType::File => {
                        let hint = Observation::of(&entry);
                        if self.view().current(&child, hint) {
                            continue;
                        }
                        if let Err(err) = self.cache().hydrate(&child, Some(hint), None).await {
                            log::debug!("pin {child}: {err}");
                        }
                    }
                }
            }
        }
    }
}

impl<'a, T: LocalStore> Cache<'a, T> {
    pub fn pin(&self, path: &RelPath) {
        self.0.ledger().pin_set(path);
        log::info!("pinned {path}");
        self.0.pin_sweep();
    }

    pub fn unpin(&self, path: &RelPath) {
        self.0.ledger().pin_clear(path);
        log::info!("unpinned {path}");
    }

    pub fn pinned(&self, path: &RelPath) -> bool {
        self.0
            .ledger()
            .pins
            .iter()
            .any(|p| path == p || path.is_descendant_of(p))
    }

    pub fn evict(&self, cache_root: &Path) -> io::Result<()> {
        if std::mem::take(&mut self.0.ledger().unreadable) {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let aside = cache_root.with_file_name(format!(
                "{}.unreadable-{stamp}",
                cache_root.file_name().unwrap_or_default().to_string_lossy()
            ));
            log::error!(
                "the ledger was unreadable; moving the cache to {} instead of pruning it",
                aside.display()
            );
            fs::rename(cache_root, &aside)?;
            fs::create_dir_all(cache_root)?;
            return Ok(());
        }
        let owed: BTreeSet<RelPath> = self.0.state().owed();
        let ledger = self.0.ledger();
        let pins = ledger.pins.clone();
        let keep: Vec<PathBuf> = ledger
            .dirty
            .iter()
            .chain(owed.iter())
            .chain(pins.iter())
            .map(|p| self.0.local.backing(p))
            .collect();
        drop(ledger);
        prune_dir(cache_root, &keep)?;
        Ok(())
    }
}

fn prune_dir(dir: &Path, keep: &[PathBuf]) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if keep.iter().any(|k| path.starts_with(k)) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            if keep.iter().any(|k| k.starts_with(&path)) {
                prune_dir(&path, keep)?;
            } else {
                fs::remove_dir_all(&path)?;
            }
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

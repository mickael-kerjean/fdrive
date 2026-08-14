use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::FileInfo;

use super::Engine;
use crate::model::{Fate, Observation};

pub struct View<'a, T: LocalStore>(pub(super) &'a Engine<T>);

impl<T: LocalStore> Engine<T> {
    pub(super) fn fates(&self) -> BTreeMap<RelPath, Fate> {
        self.state().view().fates
    }

    pub(super) fn upstream_of(&self, path: &RelPath) -> Option<RelPath> {
        match self.fates().get(path) {
            Some(Fate::Arrived { from, .. }) => Some(from.clone()),
            _ => None,
        }
    }
}

impl<'a, T: LocalStore> View<'a, T> {
    pub fn note(&self, dir: &RelPath, entries: &[FileInfo]) {
        let fates = self.0.fates();
        let mut ledger = self.0.ledger();
        for e in entries {
            if e.kind != crate::sdk::FileType::File {
                continue;
            }
            let path = dir.join(&e.name);
            if ledger.dirty.contains(&path) || fates.contains_key(&path) {
                continue;
            }
            let obs = Observation::of(e);
            if ledger.observations.get(&path) == Some(&obs) {
                continue;
            }
            let advance = match fs::metadata(self.0.local.backing(&path)) {
                Ok(md) => Observation::of_local(&md) == obs,
                Err(_) => true,
            };
            if advance {
                ledger.observe(&path, obs);
            }
        }
    }

    pub fn merge(&self, dir: &RelPath, mut listing: Vec<FileInfo>) -> Vec<FileInfo> {
        let fates = self.0.fates();
        listing.retain(|e| {
            let path = dir.join(&e.name);
            !matches!(fates.get(&path), Some(Fate::Gone))
        });
        for (path, fate) in &fates {
            let Fate::Arrived { was, .. } = fate else {
                continue;
            };
            if path.parent_or_root() != *dir {
                continue;
            }
            let name = path.name();
            if listing.iter().any(|e| e.name == name) {
                continue;
            }
            let (size, mtime) = match fs::metadata(self.0.local.backing(path)) {
                Ok(md) => (md.len(), md.modified().ok()),
                Err(_) => (was.size, Some(UNIX_EPOCH + Duration::from_secs(was.time))),
            };
            listing.push(FileInfo {
                name: name.to_string(),
                kind: crate::sdk::FileType::File,
                size: Some(size),
                mtime,
                perms: Default::default(),
            });
        }
        let extras: Vec<RelPath> = {
            let ledger = self.0.ledger();
            ledger
                .dirty
                .iter()
                .filter(|p| ledger.local_only(p) && p.parent_or_root() == *dir)
                .cloned()
                .collect()
        };
        for path in extras {
            let name = path.name();
            if !listing.iter().any(|e| e.name == name) {
                if let Ok(md) = fs::metadata(self.0.local.backing(&path)) {
                    listing.push(FileInfo {
                        name: name.to_string(),
                        kind: crate::sdk::FileType::File,
                        size: Some(md.len()),
                        mtime: md.modified().ok(),
                        perms: Default::default(),
                    });
                }
            }
        }
        listing
    }

    pub fn remembered(&self, dir: &RelPath) -> Vec<FileInfo> {
        let ledger = self.0.ledger();
        let mut listing = Vec::new();
        let mut dirs = std::collections::BTreeSet::new();
        for (path, obs) in &ledger.observations {
            if path.parent_or_root() == *dir {
                listing.push(FileInfo {
                    name: path.name().to_string(),
                    kind: crate::sdk::FileType::File,
                    size: Some(obs.size),
                    mtime: Some(UNIX_EPOCH + Duration::from_secs(obs.time)),
                    perms: Default::default(),
                });
            } else if dir.is_root() || path.is_descendant_of(dir) {
                let rest = match dir.is_root() {
                    true => path.as_str(),
                    false => &path.as_str()[dir.as_str().len() + 1..],
                };
                if let Some((child, _)) = rest.split_once('/') {
                    dirs.insert(child.to_string());
                }
            }
        }
        for name in dirs {
            listing.push(FileInfo {
                name,
                kind: crate::sdk::FileType::Directory,
                size: Some(0),
                mtime: None,
                perms: Default::default(),
            });
        }
        listing
    }

    pub fn seen(&self, path: &RelPath) -> Option<Observation> {
        self.0.ledger().observations.get(path).copied()
    }

    pub fn dirty(&self, path: &RelPath) -> bool {
        self.0.ledger().dirty.contains(path)
    }

    pub fn untracked(&self, path: &RelPath) -> bool {
        let ledger = self.0.ledger();
        !ledger.observations.contains_key(path) && !ledger.dirty.contains(path)
    }

    pub async fn overwriting(&self, path: &RelPath) {
        if self.untracked(path) {
            if let Ok(info) = self.0.sdk.stat(&path.as_file()).await {
                self.0.ledger().observe(path, Observation::of(&info));
            }
        }
    }

    pub fn current(&self, path: &RelPath, remote: Observation) -> bool {
        let (observed, dirty) = {
            let ledger = self.0.ledger();
            (
                ledger.observations.get(path).copied(),
                ledger.dirty.contains(path),
            )
        };
        dirty || (observed == Some(remote) && self.0.local.backing(path).is_file())
    }

    pub fn pending_metadata(&self, path: &RelPath) -> Option<fs::Metadata> {
        if !self.0.ledger().dirty.contains(path) {
            return None;
        }
        fs::metadata(self.0.local.backing(path)).ok()
    }
}

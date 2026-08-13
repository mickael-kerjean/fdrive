use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fdrive_core::engine::Observation;
use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::{FileInfo, FileType};

use crate::wire;

use super::utils::pin_of;
use super::{Adapter, FileState, Pin};

#[derive(Clone, Copy)]
pub(super) struct Reconcile<'a>(pub(super) &'a Arc<Adapter>);

impl Reconcile<'_> {
    pub(super) fn subdirs(self, dir: &RelPath) -> Vec<RelPath> {
        let mut dirs = Vec::new();
        let Ok(read) = fs::read_dir(self.0.abs(dir)) else {
            return dirs;
        };
        for entry in read.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok(md) = entry.metadata() else { continue };
            if !md.is_dir() {
                continue;
            }
            match wire::placeholder_state(&entry.path()) {
                Ok(st) if st.placeholder && st.partial => continue,
                _ => dirs.push(dir.join(&name)),
            }
        }
        dirs
    }

    pub(super) fn classify(self, abs: &Path, path: &RelPath) -> io::Result<FileState> {
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
        let md = fs::symlink_metadata(abs)?;
        let ps = wire::placeholder_state(abs)?;
        if !ps.placeholder {
            return Ok(if self.0.engine.observed(path).is_some() {
                FileState::Foreign
            } else {
                FileState::New
            });
        }
        if !ps.in_sync {
            return Ok(FileState::Edited);
        }
        let attrs = std::os::windows::fs::MetadataExt::file_attributes(&md);
        Ok(
            if attrs & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0 != 0 || ps.partial {
                FileState::Dehydrated(pin_of(&md))
            } else {
                FileState::Cached(pin_of(&md))
            },
        )
    }

    pub(super) fn dir(
        self,
        dir: &RelPath,
        dir_abs: &Path,
        listing: Vec<FileInfo>,
    ) -> io::Result<()> {
        self.0.engine.listed(dir, &listing);
        let listing = self.0.engine.overlay(dir, listing);
        let mut local: BTreeMap<String, fs::Metadata> = BTreeMap::new();
        for entry in fs::read_dir(dir_abs)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(md) = entry.metadata() {
                    local.insert(name.to_string(), md);
                }
            }
        }
        for entry in &listing {
            let child = dir.join(&entry.name);
            if child.parent_or_root() != *dir {
                continue;
            }
            match local.remove(&entry.name) {
                None => self.place(dir, entry),
                Some(md) => self.entry(&child, entry, &md),
            }
        }
        for (name, md) in local {
            let child = dir.join(&name);
            if child.parent_or_root() != *dir {
                continue;
            }
            self.discard(&child, md.is_dir());
        }
        Ok(())
    }

    pub(super) fn place(self, dir: &RelPath, entry: &FileInfo) {
        let child = dir.join(&entry.name);
        if child.parent_or_root() != *dir {
            log::warn!("skipping hostile name from the server in {dir}");
            return;
        }
        let mtime = entry.mtime.unwrap_or_else(SystemTime::now);
        let result = match entry.kind {
            FileType::Directory => wire::create_dir_placeholder(&self.0.root, &child, mtime),
            FileType::File => {
                wire::create_placeholder(&self.0.root, &child, entry.size.unwrap_or(0), mtime)
            }
        };
        match result {
            Ok(()) if entry.kind == FileType::File => {
                self.0.engine.ledger().observe(&child, Observation::of(entry))
            }
            Ok(()) => {}
            Err(err) => log::debug!("place {child}: {err}"),
        }
    }

    fn entry(self, path: &RelPath, remote: &FileInfo, md: &fs::Metadata) {
        let abs = self.0.abs(path);
        match (remote.kind, md.is_dir()) {
            (FileType::Directory, true) => {
                if matches!(wire::placeholder_state(&abs), Ok(st) if !st.placeholder) {
                    match wire::mark_in_sync(&abs, path) {
                        Ok(()) => log::info!("re-adopted directory {path}"),
                        Err(err) => log::debug!("adopt dir {path}: {err}"),
                    }
                }
            }
            (FileType::File, false) => match self.classify(&abs, path) {
                Ok(FileState::Cached(_) | FileState::Dehydrated(_)) => {
                    self.freshen(path, remote, md)
                }
                Ok(FileState::Foreign | FileState::New) => self.adopt(path, &abs, md),
                Ok(FileState::Edited) | Err(_) => {}
            },
            _ => {}
        }
    }

    fn adopt(self, path: &RelPath, abs: &Path, md: &fs::Metadata) {
        if self.0.engine.is_dirty(path) {
            return;
        }
        let observed = self.0.engine.observed(path);
        match observed {
            Some(rec) if Observation::of_local(md) == rec => {
                match wire::mark_in_sync_if_unmodified(abs, path, md.modified().ok()) {
                    Ok(()) => log::info!("re-adopted {path}"),
                    Err(err) => log::debug!("adopt {path}: {err}"),
                }
            }
            Some(rec) if md.len() == 0 && rec.size > 0 => {
                log::debug!(
                    "{path} is an empty husk of {} observed bytes; leaving it untouched",
                    rec.size
                );
            }
            Some(_) => {
                log::info!("adopting local edit {path}");
                self.0.engine.modified(path);
            }
            None => self.0.engine.modified(path),
        }
    }

    fn freshen(self, path: &RelPath, remote: &FileInfo, md: &fs::Metadata) {
        if self.0.engine.is_dirty(path) {
            return;
        }
        let remote_rec = Observation::of(remote);
        let unchanged = match self.0.engine.observed(path) {
            Some(rec) => rec == remote_rec,
            None => Observation::of_local(md) == remote_rec,
        };
        if unchanged {
            return;
        }
        let abs = self.0.abs(path);
        if matches!(
            self.classify(&abs, path),
            Ok(FileState::Cached(Pin::Pinned))
        ) {
            let engine = self.0.engine.clone();
            let what = path.clone();
            self.0.engine.spawn(async move {
                *engine
                    .local()
                    .suppressed
                    .lock()
                    .unwrap()
                    .entry(what.clone())
                    .or_insert(0) += 1;
                let result = match engine.hydrate(&what, Some(remote_rec)).await {
                    Ok(()) => {
                        let done = what.clone();
                        let done_abs = engine.local().backing(&done);
                        tokio::task::spawn_blocking(move || {
                            let mut result = Err(io::Error::other("pinned refresh incomplete"));
                            for _ in 0..20 {
                                result = wire::mark_in_sync(&done_abs, &done)
                                    .and_then(|()| wire::set_pinned(&done_abs));
                                if result.is_ok() {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            result
                        })
                        .await
                        .map_err(io::Error::other)
                        .and_then(|result| result)
                    }
                    Err(err) => Err(err),
                };
                {
                    let mut suppressed = engine.local().suppressed.lock().unwrap();
                    if let Some(n) = suppressed.get_mut(&what) {
                        *n -= 1;
                        if *n == 0 {
                            suppressed.remove(&what);
                        }
                    }
                }
                match result {
                    Ok(()) => log::info!("refreshed pinned {what}"),
                    Err(err) => log::warn!("refresh pinned {what}: {err}"),
                }
            });
            return;
        }
        match self.rebuild(
            path,
            remote.size.unwrap_or(0),
            remote.mtime.unwrap_or_else(SystemTime::now),
        ) {
            Ok(()) => self.0.engine.ledger().observe(path, remote_rec),
            Err(err) => log::debug!("update {path}: {err}"),
        }
    }

    pub(super) fn rebuild(self, path: &RelPath, size: u64, mtime: SystemTime) -> io::Result<()> {
        let abs = self.0.abs(path);
        let pinned = matches!(
            self.classify(&abs, path),
            Ok(FileState::Cached(Pin::Pinned) | FileState::Dehydrated(Pin::Pinned))
        );
        let result = self.0.engine.local().suppress(path, || {
            match wire::delete_if_clean(&abs) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            wire::create_placeholder(&self.0.root, path, size, mtime)
        });
        if result.is_ok() {
            self.0.engine.ledger().unobserve(path);
            log::info!("rebuilt placeholder {path} ({size} bytes)");
            if pinned {
                if let Err(err) = wire::set_pinned(&abs) {
                    log::warn!("re-pin {path}: {err}");
                }
            }
        }
        result
    }

    fn discard(self, path: &RelPath, is_dir: bool) {
        let abs = self.0.abs(path);
        if is_dir {
            if !self.tree_clean(&abs, path, true) {
                if self.0.kept.lock().unwrap().insert(path.clone()) {
                    log::info!("{path} gone remotely but holds local edits; keeping");
                }
                return;
            }
        } else if !self.clean(&abs, path) {
            if matches!(self.classify(&abs, path), Ok(FileState::New))
                && !self.0.engine.is_dirty(path)
            {
                log::info!("found new local file {path}");
                self.0.engine.modified(path);
            }
            return;
        }
        let removed = self.0.engine.local().suppress(path, || {
            if is_dir {
                fs::remove_dir_all(&abs)
            } else {
                wire::delete_if_clean(&abs)
            }
        });
        match removed {
            Ok(()) => {
                log::info!("dropped {path} (gone remotely)");
                self.0.engine.ledger().forget(path);
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                self.0.engine.ledger().forget(path);
            }
            Err(err) => log::warn!("drop {path}: {err}"),
        }
        self.0.kept.lock().unwrap().remove(path);
    }

    fn clean(self, abs: &Path, path: &RelPath) -> bool {
        !self.0.engine.is_dirty(path)
            && matches!(
                self.classify(abs, path),
                Ok(FileState::Cached(_) | FileState::Dehydrated(_))
            )
    }

    fn tree_clean(self, abs: &Path, path: &RelPath, root: bool) -> bool {
        let Ok(state) = wire::placeholder_state(abs) else {
            return false;
        };
        if root && !state.placeholder {
            return false;
        }
        if state.placeholder && state.partial {
            return true;
        }
        let Ok(read) = fs::read_dir(abs) else {
            return false;
        };
        for entry in read.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                return false;
            };
            let child = path.join(&name);
            let child_abs = entry.path();
            let clean = match entry.metadata() {
                Ok(md) if md.is_dir() => self.tree_clean(&child_abs, &child, false),
                Ok(_) => self.clean(&child_abs, &child),
                Err(_) => false,
            };
            if !clean {
                return false;
            }
        }
        true
    }

    pub(super) fn sweep(self) -> Vec<RelPath> {
        let mut armed = Vec::new();
        let mut dehydrated = 0u32;
        let mut hydrated = 0u32;
        let mut pending = vec![(RelPath::root(), false)];
        while let Some((dir, inherited)) = pending.pop() {
            let Ok(read) = fs::read_dir(self.0.abs(&dir)) else {
                continue;
            };
            for entry in read.flatten() {
                let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                let child = dir.join(&name);
                let abs = entry.path();
                let Ok(md) = entry.metadata() else { continue };
                let pin = match pin_of(&md) {
                    Pin::Unspecified if inherited => match wire::set_pinned(&abs) {
                        Ok(()) => Pin::Pinned,
                        Err(err) => {
                            log::debug!("pin {child}: {err}");
                            Pin::Unspecified
                        }
                    },
                    pin => pin,
                };
                if md.is_dir() {
                    match wire::placeholder_state(&abs) {
                        Ok(st) if st.placeholder && st.partial => {
                            if pin == Pin::Pinned {
                                pending.push((child, true));
                            }
                        }
                        _ => pending.push((child, pin == Pin::Pinned)),
                    }
                    continue;
                }
                match self.classify(&abs, &child) {
                    Ok(FileState::Edited) if !self.0.engine.is_dirty(&child) => {
                        self.0.engine.modified(&child);
                        armed.push(child);
                    }
                    Ok(FileState::Dehydrated(Pin::Pinned)) => match wire::set_hydration(&abs, true)
                    {
                        Ok(()) => hydrated += 1,
                        Err(err) => log::debug!("hydrate {child}: {err}"),
                    },
                    Ok(FileState::Cached(Pin::Unpinned)) => {
                        match wire::set_hydration(&abs, false) {
                            Ok(()) => dehydrated += 1,
                            Err(err) => log::debug!("dehydrate {child}: {err}"),
                        }
                    }
                    Ok(FileState::Foreign) => self.adopt(&child, &abs, &md),
                    _ => {}
                }
            }
        }
        if dehydrated > 0 || hydrated > 0 {
            log::info!("pin sweep: {hydrated} hydrated, {dehydrated} dehydrated");
        }
        armed
    }

    pub(super) fn pin(self, dir: &RelPath) {
        {
            let mut pinning = self.0.pinning.lock().unwrap();
            if pinning.iter().any(|p| p == dir || dir.is_descendant_of(p)) {
                return;
            }
            pinning.insert(dir.clone());
        }
        self.pin_walk(dir);
        self.0.pinning.lock().unwrap().remove(dir);
    }

    fn pin_walk(self, dir: &RelPath) {
        let Ok(read) = fs::read_dir(self.0.abs(dir)) else {
            return;
        };
        for entry in read.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let child = dir.join(&name);
            let abs = entry.path();
            let Ok(md) = entry.metadata() else { continue };
            match pin_of(&md) {
                Pin::Unpinned => continue,
                Pin::Pinned => {}
                Pin::Unspecified => {
                    if let Err(err) = wire::set_pinned(&abs) {
                        log::debug!("pin {child}: {err}");
                        continue;
                    }
                }
            }
            if md.is_dir() {
                self.pin_walk(&child);
            } else if matches!(self.classify(&abs, &child), Ok(FileState::Dehydrated(_))) {
                match wire::set_hydration(&abs, true) {
                    Ok(()) => log::info!("hydrated {child} (pinned)"),
                    Err(err) => log::warn!("hydrate {child}: {err}"),
                }
            }
        }
    }

    pub(super) fn vacuum(self, dir: &RelPath) -> io::Result<bool> {
        let mut emptied = true;
        for entry in fs::read_dir(self.0.abs(dir))? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                emptied = false;
                continue;
            };
            let child = dir.join(&name);
            let abs = entry.path();
            let Ok(md) = entry.metadata() else {
                emptied = false;
                continue;
            };
            if md.is_dir() {
                let state = wire::placeholder_state(&abs).ok();
                let placeholder = state.is_some_and(|s| s.placeholder);
                if placeholder && state.is_some_and(|s| s.partial) {
                    if fs::remove_dir_all(&abs).is_err() {
                        emptied = false;
                    }
                } else if placeholder && self.vacuum(&child).unwrap_or(false) {
                    if fs::remove_dir(&abs).is_err() {
                        emptied = false;
                    }
                } else {
                    let _ = self.vacuum(&child);
                    emptied = false;
                }
            } else if self.clean(&abs, &child) {
                if wire::delete_if_clean(&abs).is_err() {
                    emptied = false;
                }
            } else {
                emptied = false;
            }
        }
        Ok(emptied)
    }
}

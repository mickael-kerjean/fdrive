use std::fs;
use std::io;
use std::sync::Arc;

use fdrive_core::path::RelPath;
use fdrive_core::sdk::Error as SdkError;

use crate::wire;

use super::pin;
use super::{Adapter, FileState, Pin};

#[derive(Clone, Copy)]
pub struct Fs<'a>(pub(super) &'a Arc<Adapter>);

impl Fs<'_> {
    pub fn populate(self, dir: &RelPath) -> io::Result<()> {
        let gate = self.0.refreshing.lock().unwrap().entry(dir.clone()).or_default().clone();
        let _guard = self.0.engine.block_on(gate.lock());
        self.0.populated.lock().unwrap().insert(dir.clone());
        let listing = match self.0.engine.block_on(self.0.engine.fs().ls(dir)) {
            Ok(listing) => listing,
            Err(SdkError::NotFound) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        for entry in &listing {
            self.0.reconcile().place(dir, entry);
        }
        Ok(())
    }

    pub fn on_delete(self, path: &RelPath, is_dir: bool) -> io::Result<()> {
        if self.0.engine.local().is_suppressed(path) {
            return Ok(());
        }
        self.0.engine.block_on(self.0.engine.fs().delete(path, is_dir))
    }

    pub fn on_rename(self, from: &RelPath, to: &RelPath, is_dir: bool) -> io::Result<()> {
        if self.0.engine.local().is_suppressed(from) {
            return Ok(());
        }
        self.0.engine.block_on(self.0.engine.fs().rename(from, to, is_dir))
    }

    pub async fn on_change(self, path: &RelPath) {
        let internal = path
            .as_str()
            .strip_suffix(".part")
            .and_then(|path| path.rsplit_once('.'))
            .is_some_and(|(_, sequence)| sequence.parse::<u64>().is_ok());
        if internal || self.0.engine.local().is_suppressed(path) {
            return;
        }
        let abs = self.0.abs(path);
        let Ok(md) = fs::symlink_metadata(&abs) else {
            return;
        };
        if md.is_dir() {
            let Ok(state) = wire::placeholder_state(&abs) else {
                return;
            };
            if !state.placeholder {
                match self.0.engine.fs().mkdir(path).await {
                    Ok(()) => log::info!("mkdir {path}"),
                    Err(err) => log::debug!("mkdir {path}: {err}"),
                }
                let what = path.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(err) = wire::mark_in_sync(&abs, &what) {
                        log::debug!("convert dir {what}: {err}");
                    }
                });
            } else if wire::pin::of(&md) == Pin::Pinned {
                let this = self.0.clone();
                let what = path.clone();
                tokio::task::spawn_blocking(move || pin::walk(&this, &what));
            }
            return;
        }
        let Ok(fstate) = self.0.reconcile().classify(&abs, path) else {
            return;
        };
        match fstate {
            FileState::Edited | FileState::New => self.0.engine.fs().modified(path),
            FileState::Dehydrated(Pin::Pinned) | FileState::Cached(Pin::Unpinned) => {
                let what = path.clone();
                tokio::task::spawn_blocking(move || pin::enforce(&abs, &what, fstate));
            }
            _ => {}
        }
    }

    pub async fn refresh(self, dir: &RelPath) -> io::Result<()> {
        self.refresh_inner(dir, false).await
    }

    pub(super) async fn refresh_remote(self, dir: &RelPath) -> io::Result<()> {
        self.refresh_inner(dir, true).await
    }

    async fn refresh_inner(self, dir: &RelPath, wait: bool) -> io::Result<()> {
        let _busy = self.0.working();
        let gate = self.0.refreshing.lock().unwrap().entry(dir.clone()).or_default().clone();
        let _guard = if wait {
            gate.lock().await
        } else {
            match gate.try_lock() {
                Ok(guard) => guard,
                Err(_) => return Ok(()),
            }
        };
        let dir_abs = self.0.abs(dir);
        if !dir.is_root() {
            match wire::placeholder_state(&dir_abs) {
                Ok(state) if state.placeholder && state.partial => return Ok(()),
                Err(_) => return Ok(()),
                _ => {}
            }
        }
        self.0.populated.lock().unwrap().insert(dir.clone());
        let result = match self.0.engine.fs().ls(dir).await {
            Ok(listing) => {
                let this = self.0.clone();
                let dir2 = dir.clone();
                tokio::task::spawn_blocking(move || this.reconcile().dir(&dir2, &dir_abs, listing))
                    .await
                    .map_err(io::Error::other)?
            }
            Err(SdkError::NotFound) => Ok(()),
            Err(err) => Err(err.into()),
        };
        result
    }
}

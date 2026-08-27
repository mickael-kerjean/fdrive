use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::path::RelPath;

use crate::wire;
use crate::wire::pin::Pin;

use super::{Adapter, FileState};

pub(super) fn walk(adapter: &Arc<Adapter>, dir: &RelPath) {
    {
        let mut pinning = adapter.pinning.lock().unwrap();
        if pinning.iter().any(|p| p == dir || dir.is_descendant_of(p)) {
            return;
        }
        pinning.insert(dir.clone());
    }
    descend(adapter, dir);
    adapter.pinning.lock().unwrap().remove(dir);
}

pub(super) fn enforce(abs: &Path, path: &RelPath, state: FileState) {
    match state {
        FileState::Dehydrated(Pin::Pinned) => match wire::pin::hydrate(abs) {
            Ok(()) => log::info!("hydrated {path} (pinned)"),
            Err(err) => log::debug!("hydrate {path}: {err}"),
        },
        FileState::Cached(Pin::Unpinned) => match wire::pin::dehydrate(abs) {
            Ok(()) => log::info!("dehydrated {path}"),
            Err(err) => log::debug!("dehydrate {path}: {err}"),
        },
        _ => {}
    }
}

pub(super) async fn repin(abs: PathBuf, path: RelPath) -> io::Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut result = Err(io::Error::other("pinned refresh incomplete"));
        for _ in 0..20 {
            result = wire::mark_in_sync(&abs, &path).and_then(|()| wire::pin::set_pinned(&abs));
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

fn descend(adapter: &Arc<Adapter>, dir: &RelPath) {
    let abs = adapter.abs(dir);
    relist(adapter, dir, &abs);
    let Ok(read) = fs::read_dir(&abs) else {
        return;
    };
    let entries: Vec<_> = read.flatten().collect();
    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let child = dir.join(&name);
        let abs = entry.path();
        let Ok(md) = entry.metadata() else { continue };
        match wire::pin::of(&md) {
            Pin::Unpinned => continue,
            Pin::Pinned => {}
            Pin::Unspecified => {
                if let Err(err) = wire::pin::set_pinned(&abs) {
                    log::debug!("pin {child}: {err}");
                    continue;
                }
            }
        }
        if md.is_dir() {
            descend(adapter, &child);
        } else if let Ok(state) = adapter.reconcile().classify(&abs, &child) {
            enforce(&abs, &child, state);
        }
    }
}

fn relist(adapter: &Arc<Adapter>, dir: &RelPath, abs: &Path) {
    let listed = adapter
        .engine
        .block_on(adapter.engine.fs().ls(dir))
        .map_err(io::Error::from)
        .and_then(|listing| adapter.reconcile().dir(dir, abs, listing))
        .and_then(|()| wire::mark_populated(abs));
    if let Err(err) = listed {
        log::warn!("pin list {dir}: {err}");
    }
}

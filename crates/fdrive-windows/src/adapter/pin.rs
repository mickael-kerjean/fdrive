use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use fdrive_core::path::RelPath;

use crate::wire;
use crate::wire::pin::Pin;

use super::{Adapter, FileState};

const CONCURRENCY: usize = 6;

pub(super) fn walk(adapter: &Arc<Adapter>, dir: &RelPath) {
    {
        let mut pinning = adapter.pinning.lock().unwrap();
        if pinning.iter().any(|p| p == dir || dir.is_descendant_of(p)) {
            return;
        }
        pinning.insert(dir.clone());
    }
    let (root, (missing, queue)) = (adapter.abs(dir), mpsc::channel());
    let queue = Mutex::new(queue);
    let _ = wire::pin::pending(&root);
    wire::pin::notify(&root);
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY {
            scope.spawn(|| hydrate(Some(&root), &queue));
        }
        descend(adapter, dir, &root, &missing);
        drop(missing);
    });
    let busy = |p: &RelPath| adapter.pinning.lock().unwrap().iter().any(|w| w != dir && (w == p || w.is_descendant_of(p)));
    let mut up = dir.clone();
    while !up.is_root() && pinned(&adapter.abs(&up)) && !busy(&up) {
        let abs = adapter.abs(&up);
        if wire::placeholder_state(&abs).is_ok_and(|s| !s.in_sync) {
            let _ = wire::mark_in_sync(&abs, &up);
            wire::pin::notify(&abs);
        }
        up = up.parent_or_root();
    }
    adapter.pinning.lock().unwrap().remove(dir);
}

pub(super) fn fetch(missing: Vec<(PathBuf, RelPath, FileState)>) {
    let (tx, queue) = mpsc::channel();
    for item in missing {
        let _ = tx.send(item);
    }
    drop(tx);
    let queue = Mutex::new(queue);
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY {
            scope.spawn(|| hydrate(None, &queue));
        }
    });
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

fn descend(adapter: &Arc<Adapter>, dir: &RelPath, root: &Path, missing: &mpsc::Sender<(PathBuf, RelPath, FileState)>) {
    let abs = adapter.abs(dir);
    if !pinned(root) {
        log::info!("pin walk {dir}: no longer pinned, stopping");
        return;
    }
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
            if !adapter.pinning.lock().unwrap().contains(&child) {
                descend(adapter, &child, root, missing);
            }
        } else if pinned(root) && pinned(&abs) {
            if let Ok(state) = adapter.reconcile().classify(&abs, &child) {
                let _ = missing.send((abs, child, state));
            }
        }
    }
}

fn hydrate(root: Option<&Path>, queue: &Mutex<mpsc::Receiver<(PathBuf, RelPath, FileState)>>) {
    loop {
        let Ok((abs, path, state)) = queue.lock().unwrap().recv() else { break };
        if root.is_none_or(pinned) && pinned(&abs) {
            enforce(&abs, &path, state);
        }
    }
}

fn pinned(abs: &Path) -> bool {
    fs::symlink_metadata(abs).is_ok_and(|md| wire::pin::of(&md) == Pin::Pinned)
}

fn relist(adapter: &Arc<Adapter>, dir: &RelPath, abs: &Path) {
    let listed = adapter
        .engine
        .block_on(adapter.engine.fs().ls(dir))
        .map_err(io::Error::from)
        .and_then(|listing| adapter.reconcile().dir(dir, abs, listing))
        .and_then(|()| wire::mark_populated(abs, true));
    if let Err(err) = listed {
        log::warn!("pin list {dir}: {err}");
    }
}

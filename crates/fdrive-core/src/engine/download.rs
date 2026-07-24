use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures_util::TryStreamExt;
use tokio::sync::watch;

use crate::path::RelPath;
use crate::port::LocalTree;

use crate::activity::{Direction, Mode};
use crate::sdk::{CatDelta, Error as SdkError, FileInfo};

use super::Engine;
use crate::model::{Fate, Observation};

fn part_file(abs: &Path) -> PathBuf {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut tmp = abs.as_os_str().to_owned();
    tmp.push(format!(".{}.part", COUNTER.fetch_add(1, Ordering::Relaxed)));
    PathBuf::from(tmp)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    Running,
    Done,
    Failed,
}

pub struct Download {
    file: fs::File,
    state: watch::Receiver<(u64, DownloadStatus)>,
}

impl Download {
    pub async fn read(&self, offset: u64, size: u32) -> io::Result<Vec<u8>> {
        let end = offset + size as u64;
        let mut state = self.state.clone();
        loop {
            let (written, status) = *state.borrow_and_update();
            match status {
                DownloadStatus::Failed => return Err(io::Error::other("download failed")),
                DownloadStatus::Done => break,
                DownloadStatus::Running if written >= end => break,
                DownloadStatus::Running => {
                    if state.changed().await.is_err() {
                        return Err(io::Error::other("download aborted"));
                    }
                }
            }
        }
        let mut buf = vec![0u8; size as usize];
        let mut filled = 0;
        while filled < buf.len() {
            let n = pread(&self.file, &mut buf[filled..], offset + filled as u64)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        Ok(buf)
    }

    pub async fn done(&self) -> io::Result<()> {
        let mut state = self.state.clone();
        loop {
            let status = state.borrow_and_update().1;
            match status {
                DownloadStatus::Done => return Ok(()),
                DownloadStatus::Failed => return Err(io::Error::other("download failed")),
                DownloadStatus::Running => {
                    if state.changed().await.is_err() {
                        return Err(io::Error::other("download aborted"));
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
fn pread(file: &fs::File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(windows)]
fn pread(file: &fs::File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

#[cfg(unix)]
fn pwrite(file: &fs::File, buf: &[u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(windows)]
fn pwrite(file: &fs::File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = std::os::windows::fs::FileExt::seek_write(file, buf, offset)?;
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

impl<T: LocalTree> Engine<T> {
    pub async fn hydrate(&self, path: &RelPath, current: Option<Observation>) -> io::Result<()> {
        self.hydrate_start(path, current).await?;
        let download = self.transfers.downloads.lock().unwrap().get(path).cloned();
        match download {
            Some(download) => download.done().await,
            None => Ok(()),
        }
    }

    pub async fn hydrate_start(
        &self,
        path: &RelPath,
        current: Option<Observation>,
    ) -> io::Result<()> {
        let gate = self.transfers.hydrate_gate(path);
        let _gate = gate.lock().await;
        self.fetch_start(path, current).await
    }

    pub fn download(&self, path: &RelPath) -> Option<Arc<Download>> {
        if self.ledger().dirty.contains(path) {
            return None;
        }
        self.transfers.downloads.lock().unwrap().get(path).cloned()
    }

    async fn fetch_start(&self, path: &RelPath, current: Option<Observation>) -> io::Result<()> {
        if self.transfers.downloads.lock().unwrap().contains_key(path) {
            return Ok(());
        }
        let (observed, dirty) = {
            let ledger = self.ledger();
            (
                ledger.observations.get(path).copied(),
                ledger.dirty.contains(path),
            )
        };
        if dirty {
            return Ok(());
        }
        let upstream = match self.fates().get(path) {
            Some(Fate::Gone) => return Err(io::ErrorKind::NotFound.into()),
            Some(Fate::Arrived { from, .. }) => from.clone(),
            None => path.clone(),
        };
        let abs = self.tree.backing(path);
        let current = match current {
            Some(current) => current,
            None => match self.sdk.stat(&upstream.as_file()).await {
                Ok(info) => Observation::of(&info),
                Err(err @ (SdkError::NotFound | SdkError::PermissionDenied)) => {
                    return Err(err.into())
                }
                Err(err) if abs.is_file() => {
                    log::debug!("hydrate {path} unreachable, serving the cache: {err}");
                    return Ok(());
                }
                Err(err) => return Err(err.into()),
            },
        };
        if observed == Some(current) && abs.is_file() {
            return Ok(());
        }
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = part_file(&abs);
        fs::File::create(&tmp)?;
        let file = fs::File::open(&tmp)?;
        let (tx, state) = watch::channel((0u64, DownloadStatus::Running));
        self.transfers
            .downloads
            .lock()
            .unwrap()
            .insert(path.clone(), Arc::new(Download { file, state }));
        self.spawner
            .spawn(|engine| engine.stream(path.clone(), tmp, tx, current));
        Ok(())
    }

    async fn stream(
        self: Arc<Self>,
        path: RelPath,
        tmp: PathBuf,
        tx: watch::Sender<(u64, DownloadStatus)>,
        expected: Observation,
    ) {
        let act = self.activity.begin(&path.as_file(), Direction::Down, expected.size);
        let fail = |err: &dyn std::fmt::Display| {
            log::warn!("hydrate {path}: {err}");
            self.activity.finish(act, Err(err.to_string()));
            let _ = fs::remove_file(&tmp);
            self.transfers.downloads.lock().unwrap().remove(&path);
            tx.send_modify(|s| s.1 = DownloadStatus::Failed);
        };
        let downloaded = async {
            let upstream = self.upstream_of(&path).unwrap_or_else(|| path.clone());
            if let Some(done) = self
                .fetch_delta(&path, &upstream, &tmp, &tx, expected, act)
                .await?
            {
                return Ok(done);
            }
            let (info, mut stream) = self.sdk.cat(&upstream.as_file()).await?;
            let mut file = fs::File::options().write(true).truncate(true).open(&tmp)?;
            let mut size: u64 = 0;
            while let Some(chunk) = stream.try_next().await? {
                io::Write::write_all(&mut file, &chunk)?;
                size += chunk.len() as u64;
                tx.send_modify(|s| s.0 = size);
                self.activity.wire(act, chunk.len() as u64);
                self.activity.progress(act, size);
            }
            Ok::<(u64, FileInfo), io::Error>((size, info))
        }
        .await;
        let (size, info) = match downloaded {
            Ok(downloaded) => downloaded,
            Err(err) => return fail(&err),
        };
        if self.ledger().dirty.contains(&path) {
            return fail(&"superseded by a local edit");
        }
        if let Err(err) = fs::rename(&tmp, self.tree.backing(&path)) {
            return fail(&err);
        }
        self.ledger()
            .observe(&path, Observation::new(size, info.mtime));
        if let Ok(data) = fs::read(self.tree.backing(&path)) {
            self.ledger()
                .sign_set(&path, &super::upload::signature(&data));
        }
        self.transfers.downloads.lock().unwrap().remove(&path);
        tx.send_modify(|s| s.1 = DownloadStatus::Done);
        self.activity.finish(act, Ok(()));
        log::info!("cached {path} ({size} bytes)");
    }

    async fn fetch_delta(
        &self,
        path: &RelPath,
        upstream: &RelPath,
        tmp: &Path,
        tx: &watch::Sender<(u64, DownloadStatus)>,
        expected: Observation,
        act: u64,
    ) -> io::Result<Option<(u64, FileInfo)>> {
        if expected.size < 1 << 20 {
            return Ok(None);
        }
        let Ok(base) = fs::read(self.tree.backing(path)) else {
            return Ok(None);
        };
        if base.is_empty() {
            return Ok(None);
        }
        match self.sdk.cat_delta(&upstream.as_file()).await {
            Ok(CatDelta::Signature {
                info,
                signature,
                sha256,
            }) => {
                self.activity.wire(act, signature.len() as u64 + 32);
                let assembled = self
                    .assemble(path, upstream, tmp, expected.size, &base, signature, sha256, act)
                    .await;
                match assembled {
                    Ok(size) => {
                        self.activity.mode(act, Mode::Delta);
                        tx.send_modify(|s| s.0 = size);
                        Ok(Some((size, info)))
                    }
                    Err(err) => {
                        log::debug!("delta {path}: {err}");
                        Ok(None)
                    }
                }
            }
            Ok(CatDelta::Full(info, mut stream)) => {
                let mut file = fs::File::options().write(true).truncate(true).open(tmp)?;
                let mut size: u64 = 0;
                while let Some(chunk) = stream.try_next().await? {
                    io::Write::write_all(&mut file, &chunk)?;
                    size += chunk.len() as u64;
                    tx.send_modify(|s| s.0 = size);
                    self.activity.wire(act, chunk.len() as u64);
                    self.activity.progress(act, size);
                }
                Ok(Some((size, info)))
            }
            Err(err) => {
                log::debug!("delta {path}: {err}");
                Ok(None)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn assemble(
        &self,
        path: &RelPath,
        upstream: &RelPath,
        tmp: &Path,
        size: u64,
        base: &[u8],
        signature: Vec<u8>,
        sha256: [u8; 32],
        act: u64,
    ) -> io::Result<u64> {
        let sig = fast_rsync::Signature::deserialize(signature).map_err(io::Error::other)?;
        let mut delta = Vec::new();
        fast_rsync::diff(&sig.index(), base, &mut delta).map_err(io::Error::other)?;
        let copies =
            super::delta::copy_map(&delta).ok_or_else(|| io::Error::other("unreadable delta"))?;
        let ranges = super::delta::missing_ranges(&copies, size, 64 * 1024);
        let missing: u64 = ranges.iter().map(|(start, end)| end - start).sum();
        if missing * 10 > size * 6 {
            return Err(io::Error::other(format!("{missing} of {size} bytes missing")));
        }
        let file = fs::File::options().write(true).open(tmp)?;
        for (server, local, len) in &copies {
            let chunk = usize::try_from(*local)
                .ok()
                .and_then(|at| base.get(at..at + *len as usize))
                .ok_or_else(|| io::Error::other("copy out of bounds"))?;
            pwrite(&file, chunk, *server)?;
        }
        for (start, end) in &ranges {
            let data = self
                .sdk
                .cat_range(&upstream.as_file(), *start, *end - 1)
                .await
                .map_err(io::Error::from)?;
            if data.len() as u64 != end - start {
                return Err(io::Error::other("short range"));
            }
            self.activity.wire(act, data.len() as u64);
            pwrite(&file, &data, *start)?;
        }
        file.set_len(size)?;
        use sha2::Digest;
        if sha2::Sha256::digest(fs::read(tmp)?).as_slice() != sha256 {
            return Err(io::Error::other("checksum mismatch"));
        }
        log::info!("delta {path} ({missing} of {size} bytes fetched)");
        Ok(size)
    }
}

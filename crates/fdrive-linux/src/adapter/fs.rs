use std::fs;
use std::io;
use std::os::unix::fs::FileExt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use fdrive_core::path::RelPath;
use fdrive_core::port::LocalStore;
use fdrive_core::sdk::{self, FileInfo, FileType};

use super::utils::{ensure_parent, fill_at, remove_path};
use super::{Adapter, Xattr};

const META_TTL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
pub struct Fs<'a>(pub(super) &'a Adapter);

impl<'a> Fs<'a> {
    pub fn ls(self, dir: &RelPath) -> io::Result<Vec<FileInfo>> {
        let cached = self
            .0
            .engine
            .local()
            .meta
            .lock()
            .unwrap()
            .get(dir)
            .and_then(|(at, listing)| (at.elapsed() < META_TTL).then(|| listing.clone()));
        let listing = match cached {
            Some(listing) => listing,
            None => match self.0.engine.block_on(self.0.engine.ls(dir)) {
                Ok(fetched) => {
                    self.0.engine.listed(dir, &fetched);
                    self.0
                        .engine
                        .local()
                        .meta
                        .lock()
                        .unwrap()
                        .insert(dir.clone(), (Instant::now(), fetched.clone()));
                    fetched
                }
                Err(err @ (sdk::Error::NotFound | sdk::Error::PermissionDenied)) => return Err(err.into()),
                Err(err) => {
                    let meta = self.0.engine.local().meta.lock().unwrap();
                    match meta.get(dir) {
                        Some((_, listing)) => {
                            log::debug!("ls {dir} unreachable, serving stale: {err}");
                            listing.clone()
                        }
                        None => {
                            drop(meta);
                            log::debug!("ls {dir} unreachable, serving the ledger: {err}");
                            self.0.engine.remembered(dir)
                        }
                    }
                }
            },
        };
        Ok(self.0.engine.overlay(dir, listing))
    }

    pub fn attr(self, path: &RelPath) -> io::Result<Option<(bool, u64, SystemTime)>> {
        if path.is_root() {
            return Ok(Some((true, 0, SystemTime::UNIX_EPOCH)));
        }
        if let Some(md) = self.0.engine.dirty_metadata(path) {
            return Ok(Some((false, md.len(), md.modified().unwrap_or(SystemTime::UNIX_EPOCH))));
        }
        Ok(self.0.entry(path)?.map(|e| {
            (
                e.kind == FileType::Directory,
                e.size.unwrap_or(0),
                e.mtime.unwrap_or(SystemTime::UNIX_EPOCH),
            )
        }))
    }

    pub fn opened(self, path: &RelPath, writable: bool) -> u64 {
        if writable {
            self.0.engine.write_opened(path);
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.0.engine.local().backing(path))
            .ok()
            .map(Arc::new);
        self.0.handles.open(path, file, writable)
    }

    pub fn closed(self, fh: u64) {
        if let Some(handle) = self.0.handles.close(fh) {
            if handle.writable {
                self.0.engine.write_closed(&handle.path);
            }
            self.0.engine.released(&handle.path);
        }
    }

    pub fn read(self, fh: u64, path: &RelPath, offset: u64, size: u32) -> io::Result<Vec<u8>> {
        if let Some(download) = self.0.engine.download(path) {
            return self.0.engine.block_on(download.read(offset, size));
        }
        let mut buf = vec![0u8; size as usize];
        let filled = match self.file(fh, path) {
            Some(file) => fill_at(&file, &mut buf, offset)?,
            None => fill_at(&fs::File::open(self.0.engine.local().backing(path))?, &mut buf, offset)?,
        };
        buf.truncate(filled);
        Ok(buf)
    }

    pub fn write(self, fh: u64, path: &RelPath, offset: u64, data: &[u8]) -> io::Result<u32> {
        match self.file(fh, path) {
            Some(file) => {
                self.0.engine.modified(path);
                file.write_all_at(data, offset)?;
            }
            None => {
                self.0.cache().hydrate(path)?;
                self.0.engine.modified(path);
                let file_path = self.0.engine.local().backing(path);
                ensure_parent(&file_path)?;
                fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&file_path)?
                    .write_all_at(data, offset)?;
            }
        }
        Ok(data.len() as u32)
    }

    pub fn truncate(self, path: &RelPath, size: u64) -> io::Result<()> {
        if size > 0 {
            self.0.cache().hydrate(path)?;
        } else if self.0.engine.needs_baseline(path) {
            self.0.engine.block_on(self.0.engine.overwriting(path));
        }
        let file_path = self.0.engine.local().backing(path);
        ensure_parent(&file_path)?;
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&file_path)?;
        self.0.engine.modified(path);
        file.set_len(size)?;
        Ok(())
    }

    pub fn create(self, path: &RelPath) -> io::Result<()> {
        let file_path = self.0.engine.local().backing(path);
        ensure_parent(&file_path)?;
        self.0.engine.created(path);
        fs::File::create(&file_path)?;
        Ok(())
    }

    pub fn mkdir(self, path: &RelPath) -> io::Result<()> {
        self.0.engine.block_on(self.0.engine.mkdir(path))?;
        self.0.engine.local().invalidate(&path.parent_or_root());
        Ok(())
    }

    pub fn delete(self, path: &RelPath, is_dir: bool) -> io::Result<()> {
        self.0.engine.block_on(self.0.engine.delete(path, is_dir))?;
        remove_path(&self.0.engine.local().backing(path))?;
        self.0.xattrs.forget(path);
        self.0.engine.local().invalidate(path);
        self.0.engine.local().drop(&path.parent_or_root(), path.name());
        Ok(())
    }

    pub fn rmdir(self, path: &RelPath) -> io::Result<()> {
        self.0.engine.local().invalidate(path);
        match self.ls(path) {
            Ok(listing) if listing.is_empty() => self.delete(path, true),
            Ok(_) => Err(io::Error::from_raw_os_error(libc::ENOTEMPTY)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => self.delete(path, true),
            Err(err) => Err(err),
        }
    }

    pub fn rename(self, from: &RelPath, to: &RelPath) -> io::Result<()> {
        let is_dir = matches!(self.attr(from)?, Some((true, ..)));
        self.0.engine.block_on(self.0.engine.rename(from, to, is_dir))?;
        let from_backing = self.0.engine.local().backing(from);
        if from_backing.exists() {
            let to_backing = self.0.engine.local().backing(to);
            ensure_parent(&to_backing)?;
            remove_path(&to_backing)?;
            fs::rename(&from_backing, &to_backing)?;
        }
        self.0.xattrs.remap(from, to);
        self.0.engine.local().invalidate(&from.parent_or_root());
        self.0.engine.local().invalidate(&to.parent_or_root());
        Ok(())
    }

    pub fn xattr(self) -> Xattr<'a> {
        Xattr(self.0)
    }

    fn file(self, fh: u64, path: &RelPath) -> Option<Arc<fs::File>> {
        use std::os::unix::fs::MetadataExt;
        let file = self.0.handles.get(fh)?;
        let same = file
            .metadata()
            .ok()
            .zip(fs::metadata(self.0.engine.local().backing(path)).ok())
            .is_some_and(|(a, b)| a.ino() == b.ino() && a.dev() == b.dev());
        if same {
            return Some(file);
        }
        let reopened = Arc::new(fs::OpenOptions::new().read(true).write(true).open(self.0.engine.local().backing(path)).ok()?);
        self.0.handles.set(fh, reopened.clone());
        Some(reopened)
    }
}

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use fdrive_core::engine::Engine;
use fdrive_core::path::RelPath;

use super::CacheTree;

pub(super) const QUIET: Duration = Duration::from_millis(100);

// Hold rmdir briefly so a subsequent parent rmdir can subsume its children.
// Persist before acknowledging FUSE; replay through core's existing journal.
pub(super) struct Deletes {
    pub dirs: BTreeSet<RelPath>,
    pub last: Instant,
    file: PathBuf,
}

impl Deletes {
    pub fn open(file: PathBuf) -> io::Result<Self> {
        let dirs: BTreeSet<RelPath> = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
            Err(err) => return Err(err),
        };
        if dirs.iter().any(|p| p.is_root() || *p != RelPath::new(p.as_str())) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid pending directory deletion",
            ));
        }
        Ok(Self {
            dirs,
            last: Instant::now(),
            file,
        })
    }

    pub fn push(&mut self, path: &RelPath) -> io::Result<()> {
        if path.is_root() {
            return Err(io::Error::from_raw_os_error(libc::EBUSY));
        }
        let mut dirs = self.dirs.clone();
        if !dirs.iter().any(|p| p == path || path.is_descendant_of(p)) {
            dirs.retain(|p| !p.is_descendant_of(path));
            dirs.insert(path.clone());
        }
        fdrive_core::write_atomic(&self.file, &serde_json::to_vec(&dirs)?)?;
        self.dirs = dirs;
        Ok(())
    }

    pub async fn flush(&mut self, engine: &Engine<CacheTree>) -> io::Result<()> {
        if self.dirs.is_empty() {
            return Ok(());
        }
        for path in &self.dirs {
            engine.fs().delete(path, true).await?;
        }
        fdrive_core::write_atomic(&self.file, b"[]")?;
        self.dirs.clear();
        Ok(())
    }
}

use std::io;
use std::time::Duration;

use crate::model::Operation;
use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::{Error as SdkError, FileInfo, Result};
use crate::ByteStream;

use super::Engine;

pub struct Fs<'a, T: LocalStore>(pub(super) &'a Engine<T>);

impl<'a, T: LocalStore> Fs<'a, T> {
    pub fn created(&self, path: &RelPath) {
        self.0.record(Operation::Create(path.clone()));
    }

    pub fn modified(&self, path: &RelPath) {
        self.0.record(Operation::Write(path.clone()));
    }

    pub async fn delete(&self, path: &RelPath, is_dir: bool) -> io::Result<()> {
        if is_dir {
            self.0.state().plan_delete_dir(path);
            self.0.kick();
            log::info!("journaled rmdir {path}/");
            return Ok(());
        }
        self.0.record(Operation::Delete(path.clone()));
        Ok(())
    }

    pub async fn rename(&self, from: &RelPath, to: &RelPath, is_dir: bool) -> io::Result<()> {
        if is_dir {
            self.0.scheduler.flush(Duration::from_secs(60)).await;
            let _frozen = self.0.freeze(&[from, to]);
            self.0.wait_uploads(from, true).await;
            self.0.wait_uploads(to, false).await;
            match self.0.sdk.mv(&from.as_dir(), &to.as_dir()).await {
                Ok(()) | Err(SdkError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
            self.0.ledger().remap(from, to);
            log::info!("renamed {from}/ -> {to}/");
            return Ok(());
        }
        self.0.record(Operation::Rename(from.clone(), to.clone()));
        Ok(())
    }

    pub fn released(&self, path: &RelPath) {
        if self.0.ledger().dirty.contains(path) {
            self.0.kick();
        }
    }

    pub fn write_opened(&self, path: &RelPath) {
        self.0.state().write_opened(path);
    }

    pub fn write_closed(&self, path: &RelPath) {
        self.0.state().write_closed(path);
        self.0.kick();
    }

    pub async fn ls(&self, dir: &RelPath) -> Result<Vec<FileInfo>> {
        self.0.sdk.ls(&dir.as_dir()).await
    }

    pub async fn mkdir(&self, dir: &RelPath) -> Result<()> {
        self.0.sdk.mkdir(&dir.as_dir()).await
    }

    pub async fn stat(&self, path: &RelPath) -> Result<FileInfo> {
        self.0.sdk.stat(&path.as_file()).await
    }

    pub async fn cat(&self, path: &RelPath) -> Result<(FileInfo, ByteStream)> {
        self.0.sdk.cat(&path.as_file()).await
    }

    pub async fn thumbnail(&self, path: &RelPath) -> Result<Vec<u8>> {
        self.0.sdk.thumbnail(&path.as_file()).await
    }
}

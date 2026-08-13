use std::io;
use std::time::Duration;

use crate::model::Operation;
use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::{Error as SdkError, FileInfo, Result};
use crate::ByteStream;

use super::Engine;

impl<T: LocalStore> Engine<T> {
    pub fn created(&self, path: &RelPath) {
        self.record(Operation::Create(path.clone()));
    }

    pub fn modified(&self, path: &RelPath) {
        self.record(Operation::Write(path.clone()));
    }

    pub async fn delete(&self, path: &RelPath, is_dir: bool) -> io::Result<()> {
        if is_dir {
            self.state().plan_delete_dir(path);
            self.kick();
            log::info!("journaled rmdir {path}/");
            return Ok(());
        }
        self.record(Operation::Delete(path.clone()));
        Ok(())
    }

    pub async fn rename(&self, from: &RelPath, to: &RelPath, is_dir: bool) -> io::Result<()> {
        if is_dir {
            self.flush(Duration::from_secs(60)).await;
            let _frozen = self.freeze(&[from, to]);
            self.wait_uploads(from, true).await;
            self.wait_uploads(to, false).await;
            match self.sdk.mv(&from.as_dir(), &to.as_dir()).await {
                Ok(()) | Err(SdkError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
            self.ledger().remap(from, to);
            log::info!("renamed {from}/ -> {to}/");
            return Ok(());
        }
        self.record(Operation::Rename(from.clone(), to.clone()));
        Ok(())
    }

    pub fn released(&self, path: &RelPath) {
        if self.ledger().dirty.contains(path) {
            self.kick();
        }
    }

    pub fn write_opened(&self, path: &RelPath) {
        self.state().write_opened(path);
    }

    pub fn write_closed(&self, path: &RelPath) {
        self.state().write_closed(path);
        self.kick();
    }

    pub async fn ls(&self, dir: &RelPath) -> Result<Vec<FileInfo>> {
        self.sdk.ls(&dir.as_dir()).await
    }

    pub async fn mkdir(&self, dir: &RelPath) -> Result<()> {
        self.sdk.mkdir(&dir.as_dir()).await
    }

    pub async fn stat(&self, path: &RelPath) -> Result<FileInfo> {
        self.sdk.stat(&path.as_file()).await
    }

    pub async fn cat(&self, path: &RelPath) -> Result<(FileInfo, ByteStream)> {
        self.sdk.cat(&path.as_file()).await
    }

    pub async fn thumbnail(&self, path: &RelPath) -> Result<Vec<u8>> {
        self.sdk.thumbnail(&path.as_file()).await
    }

    pub async fn logout(&self) -> Result<()> {
        self.sdk.logout().await
    }
}

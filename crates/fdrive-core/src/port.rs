use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::path::RelPath;

pub trait LocalStore: Send + Sync + 'static {
    fn backing(&self, path: &RelPath) -> PathBuf;

    fn relocate(&self, from: &RelPath, to: &RelPath) -> io::Result<()>;

    fn settled(&self, target: &RelPath, mtime: Option<SystemTime>);

    fn device(&self) -> String {
        gethostname::gethostname().to_string_lossy().into_owned()
    }

    fn ledger(&self) -> PathBuf;
}

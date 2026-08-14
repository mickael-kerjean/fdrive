use fdrive_core::path::RelPath;

use super::Adapter;

const PIN_XATTR: &str = "user.fdrive.pin";

#[derive(Clone, Copy)]
pub struct Xattr<'a>(pub(super) &'a Adapter);

impl Xattr<'_> {
    pub fn get(self, path: &RelPath, name: &str) -> Option<Vec<u8>> {
        if name == PIN_XATTR {
            return self.0.engine.cache().pinned(path).then(|| b"always".to_vec());
        }
        self.0.xattrs.get(path, name)
    }

    pub fn set(self, path: &RelPath, name: &str, value: &[u8], flags: i32) -> Result<(), fuser::Errno> {
        if name == PIN_XATTR {
            match value {
                b"always" => self.0.engine.cache().pin(path),
                b"auto" => self.0.engine.cache().unpin(path),
                _ => return Err(fuser::Errno::EINVAL),
            }
            return Ok(());
        }
        self.0.xattrs.set(path, name, value, flags)
    }

    pub fn remove(self, path: &RelPath, name: &str) -> Result<(), fuser::Errno> {
        if name == PIN_XATTR {
            self.0.engine.cache().unpin(path);
            return Ok(());
        }
        self.0.xattrs.remove(path, name)
    }

    pub fn list(self, path: &RelPath) -> Vec<u8> {
        self.0.xattrs.list(path)
    }
}

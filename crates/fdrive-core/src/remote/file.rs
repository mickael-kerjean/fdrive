use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub name: String,
    pub kind: FileType,
    pub size: Option<u64>,
    pub mtime: Option<SystemTime>,
    pub perms: Permissions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub rename: bool,
    pub reparent: bool,
    pub delete: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self { read: true, write: true, rename: true, reparent: true, delete: true }
    }
}

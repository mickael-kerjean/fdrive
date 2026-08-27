use std::fs;
use std::io;
use std::path::Path;

use windows::Win32::Storage::CloudFilters::{
    CfDehydratePlaceholder, CfHydratePlaceholder, CfSetPinState, CF_DEHYDRATE_FLAG_NONE,
    CF_HYDRATE_FLAG_NONE, CF_OPEN_FILE_FLAG_WRITE_ACCESS, CF_PIN_STATE_PINNED,
    CF_SET_PIN_FLAG_NONE,
};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_PINNED, FILE_ATTRIBUTE_UNPINNED};

use super::with_oplock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pin {
    Pinned,
    Unpinned,
    Unspecified,
}

pub fn of(md: &fs::Metadata) -> Pin {
    let attrs = std::os::windows::fs::MetadataExt::file_attributes(md);
    if attrs & FILE_ATTRIBUTE_PINNED.0 != 0 {
        Pin::Pinned
    } else if attrs & FILE_ATTRIBUTE_UNPINNED.0 != 0 {
        Pin::Unpinned
    } else {
        Pin::Unspecified
    }
}

pub fn set_pinned(abs: &Path) -> io::Result<()> {
    with_oplock(abs, CF_OPEN_FILE_FLAG_WRITE_ACCESS, |handle| {
        unsafe { CfSetPinState(handle, CF_PIN_STATE_PINNED, CF_SET_PIN_FLAG_NONE, None) }
            .map_err(|err| io::Error::other(format!("CfSetPinState: {err}")))
    })
}

pub fn hydrate(abs: &Path) -> io::Result<()> {
    with_oplock(abs, CF_OPEN_FILE_FLAG_WRITE_ACCESS, |handle| {
        unsafe { CfHydratePlaceholder(handle, 0, -1, CF_HYDRATE_FLAG_NONE, None) }
            .map_err(|err| io::Error::other(format!("CfHydratePlaceholder: {err}")))
    })
}

pub fn dehydrate(abs: &Path) -> io::Result<()> {
    with_oplock(abs, CF_OPEN_FILE_FLAG_WRITE_ACCESS, |handle| {
        unsafe { CfDehydratePlaceholder(handle, 0, -1, CF_DEHYDRATE_FLAG_NONE, None) }
            .map_err(|err| io::Error::other(format!("CfDehydratePlaceholder: {err}")))
    })
}

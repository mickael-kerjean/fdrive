use std::fs;

use super::Pin;

pub(super) fn pin_of(md: &fs::Metadata) -> Pin {
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_PINNED, FILE_ATTRIBUTE_UNPINNED};
    let attrs = std::os::windows::fs::MetadataExt::file_attributes(md);
    if attrs & FILE_ATTRIBUTE_PINNED.0 != 0 {
        Pin::Pinned
    } else if attrs & FILE_ATTRIBUTE_UNPINNED.0 != 0 {
        Pin::Unpinned
    } else {
        Pin::Unspecified
    }
}

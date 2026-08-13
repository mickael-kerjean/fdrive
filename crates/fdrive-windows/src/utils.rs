use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub(crate) fn wstr(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

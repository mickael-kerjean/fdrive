use std::fs;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;

pub(super) fn ensure_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent),
        None => Ok(()),
    }
}

pub(super) fn fill_at(file: &fs::File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read_at(&mut buf[filled..], offset + filled as u64)? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

pub(super) fn remove_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(md) if md.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
    .or_else(|err| match err.kind() {
        io::ErrorKind::NotFound => Ok(()),
        _ => Err(err),
    })
}

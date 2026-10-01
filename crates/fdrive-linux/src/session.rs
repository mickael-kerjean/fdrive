use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::config as store;
use fdrive_core::sdk::Sdk;
use fuser::{Config, MountOption};

use crate::adapter::Adapter;
use crate::wire::MountFs;

pub struct Session {
    pub remote_watch: fdrive_core::engine::Watch,
    pub adapter: Arc<Adapter>,
    pub fuse: fuser::BackgroundSession,
}

pub async fn connect(creds: &store::Session, mount: &Path, data: &Path) -> Result<Session, Box<dyn std::error::Error>> {
    if let Err(err) = std::fs::symlink_metadata(mount) {
        if err.raw_os_error() == Some(libc::ENOTCONN) {
            log::warn!("stale mount at {}, detaching", mount.display());
            let _ = std::process::Command::new("fusermount3").arg("-uz").arg(mount).status();
        }
    }
    std::fs::create_dir_all(mount)?;
    let sdk = Sdk::builder(&creds.url).insecure(creds.insecure).token(creds.token.clone())?;
    store::remember(data, &creds.url, sdk.token().unwrap_or_default(), creds.insecure);
    let adapter = Arc::new(Adapter::new(tokio::runtime::Handle::current(), Arc::new(sdk), data)?);
    let mount_config = {
        let mut c = Config::default();
        c.mount_options = vec![MountOption::FSName("filestash".to_string()), MountOption::DefaultPermissions];
        c
    };
    let filesystem = MountFs::new(adapter.clone(), tokio::runtime::Handle::current());
    let fuse = fuser::spawn_mount2(filesystem.clone(), mount, &mount_config)?;
    log::info!("mounted {}", mount.display());
    Ok(Session {
        remote_watch: filesystem.watch(fuse.notifier()),
        adapter,
        fuse,
    })
}

pub async fn disconnect(session: Session, data: &Path, forget: bool) {
    log::info!("unmounting");
    let Session {
        adapter,
        fuse,
        remote_watch,
    } = session;
    drop(remote_watch);
    if fuse.guard.is_finished() {
        let _ = fuse.join();
    } else if let Err(err) = fuse.umount_and_join() {
        log::warn!("unmount: {err}");
    }
    adapter.system().flush(Duration::from_secs(30)).await;
    if forget {
        if let Err(err) = adapter.system().vacuum() {
            log::warn!("vacuum on logout: {err}");
        }
        store::forget(data);
        adapter.system().logout().await;
    }
}

pub fn instance_lock(data: &Path) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(data.join("fdrive.lock"))
        .map_err(|err| format!("fdrive.lock: {err}"))?;
    match unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } {
        0 => {
            std::mem::forget(file);
            Ok(())
        }
        _ => Err(format!(
            "another instance is already running on {} — quit it first",
            data.display()
        )),
    }
}

pub fn default_data() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("filestash")
}

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

mod app;
mod log;

use fdrive_core::config as store;
use fdrive_core::engine::UploadStatus;
use fdrive_core::sdk::Sdk;
use fdrive_linux::adapter::Adapter;
use fdrive_linux::gui::{self, Boot, Credentials, Status, Tray, TrayEvent};
use fdrive_linux::wire::MountFs;
use fuser::{Config, MountOption};
use tokio::sync::mpsc::UnboundedReceiver;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app::Setup {
        mount,
        data,
        prefill,
        boot,
    } = app::init()?;
    let (tray, mut events) = gui::init(data.clone(), mount.clone(), &boot).await?;

    let mut session = match &boot {
        Boot::Fresh(creds) => match login(creds, &mount, &data, &tray).await {
            Ok(session) => Some(session),
            Err(err) => {
                tray.shutdown().await;
                return Err(err);
            }
        },
        Boot::Restored(creds) => login(creds, &mount, &data, &tray).await.ok(),
        Boot::Prompt | Boot::Idle => None,
    };
    while let Some(event) = next(&mut session, &tray, &mut events).await {
        match event {
            TrayEvent::Quit => break,
            TrayEvent::Login => {
                if let Some(creds) = tray.login(prefill.clone()).await {
                    if let Some(old) = session.take() {
                        disconnect(old, &data, &tray, true).await;
                    }
                    session = login(&creds, &mount, &data, &tray).await.ok();
                }
            }
            TrayEvent::Logout => {
                if let Some(session) = session.take() {
                    disconnect(session, &data, &tray, true).await;
                }
                tray.set(Status::LoggedOut, false).await;
            }
        }
    }
    if let Some(session) = session {
        disconnect(session, &data, &tray, false).await;
    }
    tray.shutdown().await;
    Ok(())
}

async fn next(
    session: &mut Option<Session>,
    tray: &Tray,
    events: &mut UnboundedReceiver<TrayEvent>,
) -> Option<TrayEvent> {
    let Some(session) = session else {
        return tokio::select! {
            event = events.recv() => event,
            _ = tokio::signal::ctrl_c() => None,
        };
    };

    loop {
        tokio::select! {
            event = events.recv() => return event,
            _ = session.upload_status.changed() => {
                tray.set(match *session.upload_status.borrow() {
                    UploadStatus::Idle => Status::Ok,
                    UploadStatus::Busy => Status::Syncing,
                    UploadStatus::Error => Status::Error,
                }, true).await;
            }
            _ = tokio::signal::ctrl_c() => return None,
            _ = session.fuse_watch.tick() => {
                if session.fuse.guard.is_finished() {
                    log::info!("unmounted externally, ending session");
                    return None;
                }
            }
        }
    }
}

struct Session {
    adapter: Arc<Adapter>,
    fuse: fuser::BackgroundSession,
    upload_status: tokio::sync::watch::Receiver<UploadStatus>,
    fuse_watch: tokio::time::Interval,
}

async fn login(
    creds: &Credentials,
    mount: &Path,
    data: &Path,
    tray: &Tray,
) -> Result<Session, Box<dyn std::error::Error>> {
    tray.set(Status::Syncing, true).await;
    match connect(creds, mount, data).await {
        Ok(session) => {
            log::info!("mounted {}", mount.display());
            tray.attach(session.adapter.status().activity()).await;
            tray.set(Status::Ok, true).await;
            Ok(session)
        }
        Err(err) => {
            log::error!("connect: {err}");
            tray.set(Status::Error, false).await;
            Err(err)
        }
    }
}

async fn connect(
    creds: &Credentials,
    mount: &Path,
    data: &Path,
) -> Result<Session, Box<dyn std::error::Error>> {
    if let Err(err) = std::fs::symlink_metadata(mount) {
        if err.raw_os_error() == Some(libc::ENOTCONN) {
            log::warn!("stale mount at {}, detaching", mount.display());
            let _ = std::process::Command::new("fusermount3").arg("-uz").arg(mount).status();
        }
    }
    std::fs::create_dir_all(mount)?;
    let builder = Sdk::builder(&creds.url).insecure(creds.insecure);
    let sdk = if creds.token.is_empty() {
        builder.login(&creds.user, &creds.password, &creds.storage).await?
    } else {
        builder.token(creds.token.clone())?
    };
    store::remember(data, &creds.url, sdk.token().unwrap_or_default(), creds.insecure);
    let adapter = Arc::new(Adapter::new(tokio::runtime::Handle::current(), Arc::new(sdk), data)?);
    let mount_config = {
        let mut c = Config::default();
        c.mount_options = vec![MountOption::FSName("filestash".to_string()), MountOption::DefaultPermissions];
        c
    };
    let filesystem = MountFs::new(adapter.clone(), tokio::runtime::Handle::current());
    let fuse = fuser::spawn_mount2(filesystem, mount, &mount_config)?;

    Ok(Session {
        upload_status: adapter.status().watch(),
        adapter,
        fuse,
        fuse_watch: tokio::time::interval(Duration::from_secs(2)),
    })
}

async fn disconnect(session: Session, data: &Path, tray: &Tray, forget: bool) {
    log::info!("unmounting");
    tray.set(Status::Syncing, true).await;
    let Session { adapter, fuse, .. } = session;
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

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

mod args;
mod log;

use fdrive_core::config as session_store;
use fdrive_core::engine::UploadStatus;
use fdrive_core::sdk::Sdk;
use fdrive_linux::adapter::Adapter;
use fdrive_linux::gui::{self, Credentials, Status, Tray, TrayEvent};
use fdrive_linux::wire::MountFs;
use fuser::{Config, MountOption};
use tokio::sync::mpsc::UnboundedReceiver;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args::Setup {
        mount,
        data,
        prefill,
        mut credentials,
        mut launching,
        prompt_login,
    } = args::init()?;
    log::init(&data)?;
    let (tray, mut events) = gui::init(data.clone(), mount.clone(), prompt_login).await?;

    loop {
        let creds = match credentials.take() {
            Some(creds) => creds,
            None => {
                launching = false;
                match wait_for_login(&mut events, &tray, &prefill).await {
                    Some(creds) => creds,
                    None => break,
                }
            }
        };
        tray.set(Status::Syncing, true).await;
        match session(&creds, &mount, &data, &mut events, &tray).await {
            Ok(SessionEnd::Quit) => break,
            Ok(SessionEnd::Logout) => tray.set(Status::LoggedOut, false).await,
            Ok(SessionEnd::Restart) => credentials = Some(creds),
            Err(err) if launching => {
                log::error!("session: {err}");
                tray.shutdown().await;
                return Err(err);
            }
            Err(err) => {
                log::error!("session: {err}");
                tray.set(Status::Error, false).await;
            }
        }
        launching = false;
    }
    tray.shutdown().await;
    Ok(())
}

async fn wait_for_login(
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
    prefill: &Credentials,
) -> Option<Credentials> {
    loop {
        let attempt = tokio::select! {
            event = events.recv() => match event {
                None | Some(TrayEvent::Quit) => return None,
                Some(TrayEvent::Login) => tray.login(prefill.clone()).await,
                Some(TrayEvent::Logout | TrayEvent::Restart) => continue,
            },
            _ = tokio::signal::ctrl_c() => return None,
        };
        if attempt.is_some() {
            return attempt;
        }
    }
}

async fn session(
    creds: &Credentials,
    mount: &Path,
    data: &Path,
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
) -> Result<SessionEnd, Box<dyn std::error::Error>> {
    let drive = connect(creds, mount, data).await?;
    log::info!("mounted {}", mount.display());
    tray.attach(drive.adapter.status().activity()).await;
    tray.set(Status::Ok, true).await;

    let end = serve(&drive, events, tray).await;

    log::info!("unmounting {}", mount.display());
    let Drive { adapter, fuse } = drive;
    if fuse.guard.is_finished() {
        let _ = fuse.join();
    } else {
        fuse.umount_and_join()?;
    }
    adapter.system().flush(Duration::from_secs(30)).await;
    if matches!(end, SessionEnd::Logout) {
        adapter.system().vacuum()?;
        session_store::forget(data);
        adapter.system().logout().await;
    }
    Ok(end)
}

struct Drive {
    adapter: Arc<Adapter>,
    fuse: fuser::BackgroundSession,
}

async fn connect(
    creds: &Credentials,
    mount: &Path,
    data: &Path,
) -> Result<Drive, Box<dyn std::error::Error>> {
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
    session_store::remember(data, &creds.url, sdk.token().unwrap_or_default(), creds.insecure);
    let adapter = Arc::new(Adapter::new(tokio::runtime::Handle::current(), Arc::new(sdk), data)?);
    let mount_config = {
        let mut c = Config::default();
        c.mount_options = vec![MountOption::FSName("filestash".to_string()), MountOption::DefaultPermissions];
        c
    };
    let filesystem = MountFs::new(adapter.clone(), tokio::runtime::Handle::current());
    let fuse = fuser::spawn_mount2(filesystem, mount, &mount_config)?;

    Ok(Drive { adapter, fuse })
}

async fn serve(
    drive: &Drive,
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
) -> SessionEnd {
    let mut upload_status = drive.adapter.status().watch();
    loop {
        tokio::select! {
            event = events.recv() => match event {
                None | Some(TrayEvent::Quit) => break SessionEnd::Quit,
                Some(TrayEvent::Logout) => break SessionEnd::Logout,
                Some(TrayEvent::Restart) => break SessionEnd::Restart,
                Some(TrayEvent::Login) => {}
            },
            _ = upload_status.changed() => {
                tray.set(tray_status(*upload_status.borrow()), true).await;
            },
            _ = tokio::signal::ctrl_c() => break SessionEnd::Quit,
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                if drive.fuse.guard.is_finished() {
                    log::info!("unmounted externally, ending session");
                    break SessionEnd::Quit;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SessionEnd {
    Logout,
    Restart,
    Quit,
}

fn tray_status(upload: UploadStatus) -> Status {
    match upload {
        UploadStatus::Idle => Status::Ok,
        UploadStatus::Busy => Status::Syncing,
        UploadStatus::Error => Status::Error,
    }
}

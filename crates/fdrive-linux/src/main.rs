use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

mod args;
mod log;

use fdrive_core::config as session;
use fdrive_core::engine::UploadStatus;
use fdrive_core::sdk::Sdk;
use fdrive_linux::adapter::Adapter;
use fdrive_linux::gui::{self, Credentials, Status, Tray, TrayEvent};
use fdrive_linux::wire::MountFs;
use fuser::{Config, MountOption};
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone, Copy)]
enum SessionEnd {
    Logout,
    Restart,
    Quit,
}

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

    'app: loop {
        if let Some(creds) = credentials.as_ref() {
            tray.set(Status::Syncing, true).await;
            match run(creds, &mount, &data, &mut events, &tray).await {
                Ok(SessionEnd::Quit) => break 'app,
                Ok(SessionEnd::Logout) => {
                    credentials = None;
                    tray.set(Status::LoggedOut, false).await;
                }
                Ok(SessionEnd::Restart) => {}
                Err(err) if launching => {
                    log::error!("session: {err}");
                    tray.shutdown().await;
                    return Err(err);
                }
                Err(err) => {
                    log::error!("session: {err}");
                    credentials = None;
                    tray.set(Status::Error, false).await;
                }
            }
            launching = false;
            continue;
        }
        tokio::select! {
            event = events.recv() => match event {
                None | Some(TrayEvent::Quit) => break 'app,
                Some(TrayEvent::Login) => {
                    if let Some(creds) = tray.login(prefill.clone()).await {
                        credentials = Some(creds);
                    }
                }
                Some(TrayEvent::Logout | TrayEvent::Restart) => {}
            },
            _ = tokio::signal::ctrl_c() => break 'app,
        }
    }
    tray.shutdown().await;
    Ok(())
}

async fn run(
    creds: &Credentials,
    mount: &Path,
    data: &Path,
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
) -> Result<SessionEnd, Box<dyn std::error::Error>> {
    if let Err(err) = std::fs::symlink_metadata(mount) {
        if err.raw_os_error() == Some(libc::ENOTCONN) {
            log::warn!("stale mount at {}, detaching", mount.display());
            let _ = std::process::Command::new("fusermount3")
                .arg("-uz")
                .arg(mount)
                .status();
        }
    }
    std::fs::create_dir_all(mount)?;
    let builder = Sdk::builder(&creds.url).insecure(creds.insecure);
    let sdk = if creds.token.is_empty() {
        builder
            .login(&creds.user, &creds.password, &creds.storage)
            .await?
    } else {
        builder.token(creds.token.clone())?
    };
    session::remember(
        data,
        &creds.url,
        sdk.token().unwrap_or_default(),
        creds.insecure,
    );
    let sdk = Arc::new(sdk);
    let adapter = Arc::new(Adapter::new(
        sdk.clone(),
        tokio::runtime::Handle::current(),
        data,
    )?);
    let mut mount_config = Config::default();
    mount_config.mount_options = vec![
        MountOption::FSName("filestash".to_string()),
        MountOption::DefaultPermissions,
    ];
    let filesystem = MountFs::new(adapter.clone());
    let session = fuser::spawn_mount2(filesystem, mount, &mount_config)?;
    let mut upload_status = adapter.upload_status();
    let mut unmounted = false;

    log::info!("mounted {}", mount.display());
    tray.attach(adapter.activity()).await;
    tray.set(Status::Ok, true).await;
    let end = loop {
        tokio::select! {
            event = events.recv() => match event {
                None | Some(TrayEvent::Quit) => break SessionEnd::Quit,
                Some(TrayEvent::Logout) => break SessionEnd::Logout,
                Some(TrayEvent::Restart) => break SessionEnd::Restart,
                Some(TrayEvent::Login) => {}
            },
            _ = tokio::signal::ctrl_c() => break SessionEnd::Quit,
            _ = upload_status.changed() => {
                let status = match *upload_status.borrow() {
                    UploadStatus::Idle => Status::Ok,
                    UploadStatus::Busy => Status::Syncing,
                    UploadStatus::Error => Status::Error,
                };
                tray.set(status, true).await;
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                if session.guard.is_finished() {
                    log::info!("unmounted externally, ending session");
                    unmounted = true;
                    break SessionEnd::Quit;
                }
            }
        }
    };

    log::info!("unmounting {}", mount.display());
    if unmounted {
        let _ = session.join();
    } else {
        session.umount_and_join()?;
    }
    adapter.flush(Duration::from_secs(30)).await;
    if matches!(end, SessionEnd::Logout) {
        adapter.vacuum()?;
        session::forget(data);
        let _ = sdk.logout().await;
    }
    Ok(end)
}

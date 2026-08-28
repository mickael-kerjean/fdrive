#![windows_subsystem = "windows"]

mod args;
mod log;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::config as session_store;
use fdrive_core::engine::UploadStatus;
use fdrive_core::path::RelPath;
use fdrive_core::sdk::Sdk;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use fdrive_windows::adapter::Adapter;
use fdrive_windows::config::AppConfig;
use fdrive_windows::gui::{self, Credentials, Status, Tray, TrayEvent};
use fdrive_windows::wire::{self, shell, viewer, watcher};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args::Setup {
        root,
        data,
        config,
        unregister,
        prefill_url,
        credentials,
        prompt_login,
        fresh_credentials,
    } = args::init();
    log::init(&data)?;
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
    }));
    log::info!("fdrive-windows {} starting", env!("CARGO_PKG_VERSION"));

    if unregister {
        shell::vacuum(&config.windows.provider_name, "");
        let _ = wire::unregister(&root);
        session_store::forget(&data);
        log::info!("unregistered {}", root.display());
        gui::info(&format!("Unregistered {}", root.display()));
        return Ok(());
    }

    shell::ensure_autostart(&data.join("autostart.off"));
    let (tray, mut events) = gui::init(&data, prefill_url, prompt_login)?;
    tray.set_autostart(shell::autostart_enabled());

    let mut pending = match credentials {
        Some(creds) => Some((creds, fresh_credentials)),
        None => wait_for_login(&mut events, &tray, &root, &data).await,
    };
    while let Some((creds, reveal)) = pending {
        pending = match session(&creds, &config, &root, &data, &mut events, &tray, reveal).await {
            Ok(SessionEnd::Quit) => None,
            Ok(SessionEnd::Logout) => {
                tray.set_status(Status::LoggedOut);
                wait_for_login(&mut events, &tray, &root, &data).await
            }
            Err(err) => {
                log::error!("session: {err}");
                tray.set_status(Status::Error);
                wait_for_login(&mut events, &tray, &root, &data).await
            }
        };
    }
    Ok(())
}

async fn wait_for_login(
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
    root: &Path,
    data: &Path,
) -> Option<(Credentials, bool)> {
    loop {
        match events.recv().await? {
            TrayEvent::Quit => return None,
            TrayEvent::Login(creds) => return Some((creds, true)),
            TrayEvent::Browse => gui::open_folder(root),
            TrayEvent::Autostart => toggle_autostart(data, tray),
            TrayEvent::Logout => {}
        }
    }
}

async fn session(
    creds: &Credentials,
    config: &AppConfig,
    root: &Path,
    data: &Path,
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
    reveal: bool,
) -> Result<SessionEnd, Box<dyn std::error::Error>> {
    let mut drive = connect(creds, config, root, data, tray, reveal).await?;
    let end = serve(&mut drive, events, tray, root, data, config).await;

    log::info!("disconnecting");
    tray.set_status(Status::Syncing);
    tray.clear_click();
    drive.adapter.system().flush(Duration::from_secs(30)).await;
    let logout = matches!(end, SessionEnd::Logout);
    if logout {
        if let Err(err) = drive.adapter.system().vacuum() {
            log::warn!("vacuum on logout: {err}");
        }
    }
    drop(drive.connection);
    if logout {
        match shell::unregister(&drive.sync_root_id) {
            Ok(()) => log::info!("sync root unregistered"),
            Err(err) => log::warn!("unregister on logout: {err}"),
        }
        session_store::forget(data);
        let _ = drive.sdk.logout().await;
    }
    Ok(end)
}

struct Drive {
    sdk: Arc<Sdk>,
    adapter: Arc<Adapter>,
    connection: wire::Connection,
    sync_root_id: String,
    changes: UnboundedReceiver<RelPath>,
    views: UnboundedReceiver<(RelPath, bool)>,
}

async fn connect(
    creds: &Credentials,
    config: &AppConfig,
    root: &Path,
    data: &Path,
    tray: &Tray,
    reveal: bool,
) -> Result<Drive, Box<dyn std::error::Error>> {
    tray.account(creds);
    tray.set_status(Status::Syncing);
    let builder = Sdk::builder(&creds.url).insecure(creds.insecure);
    let sdk = if creds.token.is_empty() {
        builder
            .login(&creds.user, &creds.password, &creds.storage)
            .await?
    } else {
        builder.token(creds.token.clone())?
    };
    session_store::remember(
        data,
        &creds.url,
        sdk.token().unwrap_or_default(),
        creds.insecure,
    );
    let sdk = Arc::new(sdk);
    let adapter = Adapter::new(
        sdk.clone(),
        tokio::runtime::Handle::current(),
        root.to_path_buf(),
        data,
    )?;

    let rest = creds
        .url
        .split_once("://")
        .map_or(creds.url.as_str(), |(_, rest)| rest);
    let host = rest.split(['/', '?']).next().unwrap_or(rest);
    let account = match creds.user.is_empty() {
        true => host.to_string(),
        false => format!("{}@{host}/{}", creds.user, creds.storage),
    };
    let sync_root_id = shell::sync_root_id(&config.windows.provider_name, &account, root)?;
    shell::vacuum(&config.windows.provider_name, &sync_root_id);
    shell::register(
        root,
        &shell::Registration {
            id: sync_root_id.clone(),
            display_name: config.windows.provider_name.clone(),
            icon: config
                .windows
                .icon
                .clone()
                .unwrap_or_else(shell::default_icon),
            allow_pinning: config.windows.allow_pinning,
            provider_id: wire::PROVIDER_ID,
        },
    )?;
    let connection = adapter.system().connect(root)?;
    log::info!("sync root {} connected", root.display());
    if reveal {
        gui::open_folder(root);
    }

    let (changes_tx, changes) = tokio::sync::mpsc::unbounded_channel();
    watcher::spawn(root, changes_tx)?;
    let (views_tx, views) = tokio::sync::mpsc::unbounded_channel();
    viewer::spawn(root, views_tx)?;
    adapter.system().recover().await?;

    let activity = adapter.status().activity();
    tray.on_click(move || gui::dashboard(activity.clone()));
    tray.set_status(Status::Ok);

    Ok(Drive {
        sdk,
        adapter,
        connection,
        sync_root_id,
        changes,
        views,
    })
}

async fn serve(
    drive: &mut Drive,
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
    root: &Path,
    data: &Path,
    config: &AppConfig,
) -> SessionEnd {
    let adapter = &drive.adapter;
    let upload_status = adapter.status().watch();
    let mut beat = tokio::time::interval(Duration::from_secs(1));
    let refresh_every = Duration::from_secs(config.windows.refresh_secs.max(2));
    let mut refreshed: HashMap<RelPath, Instant> = HashMap::new();
    let mut sweep_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut sweep = tokio::time::interval(Duration::from_secs(30));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            event = events.recv() => {
                log::info!("session event: {event:?}");
                match event {
                    None | Some(TrayEvent::Quit) => break SessionEnd::Quit,
                    Some(TrayEvent::Logout) => break SessionEnd::Logout,
                    Some(TrayEvent::Browse) => gui::open_folder(root),
                    Some(TrayEvent::Autostart) => toggle_autostart(data, tray),
                    Some(TrayEvent::Login(_)) => {}
                }
            },
            Some(path) = drive.changes.recv() => {
                adapter.fs().on_change(&path).await;
            }
            Some((dir, newly)) = drive.views.recv() => {
                let due = newly
                    || refreshed
                        .get(&dir)
                        .is_none_or(|at| at.elapsed() >= refresh_every);
                if due {
                    refreshed.insert(dir.clone(), Instant::now());
                    let adapter = adapter.clone();
                    tokio::spawn(async move {
                        if let Err(err) = adapter.fs().refresh(&dir).await {
                            log::warn!("refresh {dir}: {err}");
                        }
                    });
                }
            }
            _ = beat.tick() => {
                tray.set_status(tray_status(*upload_status.borrow(), adapter.busy()));
            }
            _ = sweep.tick() => {
                if sweep_task.as_ref().is_none_or(|task| task.is_finished()) {
                    let adapter = adapter.clone();
                    sweep_task = Some(tokio::spawn(async move {
                        if let Err(err) = adapter.system().recover().await {
                            log::error!("sweep: {err}");
                        }
                    }));
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SessionEnd {
    Logout,
    Quit,
}

fn toggle_autostart(data: &Path, tray: &Tray) {
    let opt_out = data.join("autostart.off");
    let result = if shell::autostart_enabled() {
        std::fs::write(&opt_out, []).and_then(|()| shell::set_autostart(false))
    } else {
        let _ = std::fs::remove_file(&opt_out);
        shell::set_autostart(true)
    };
    if let Err(err) = result {
        log::error!("autostart: {err}");
    }
    tray.set_autostart(shell::autostart_enabled());
}

fn tray_status(upload: UploadStatus, busy: bool) -> Status {
    match (upload, busy) {
        (UploadStatus::Error, _) => Status::Error,
        (UploadStatus::Busy, _) | (_, true) => Status::Syncing,
        _ => Status::Ok,
    }
}

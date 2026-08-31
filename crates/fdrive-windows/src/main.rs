#![windows_subsystem = "windows"]

mod app;
mod log;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::config as store;
use fdrive_core::engine::UploadStatus;
use fdrive_core::path::RelPath;
use fdrive_core::sdk::Sdk;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use fdrive_windows::adapter::Adapter;
use fdrive_windows::config::AppConfig;
use fdrive_windows::gui::{self, Boot, Credentials, Status, Tray, TrayEvent};
use fdrive_windows::wire::{self, shell, viewer, watcher};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app::Setup {
        root,
        data,
        config,
        unregister,
        boot,
    } = app::init();
    log::info!("fdrive-windows {} starting", env!("CARGO_PKG_VERSION"));

    if unregister {
        shell::vacuum(&config.windows.provider_name, "");
        let _ = wire::unregister(&root);
        store::forget(&data);
        log::info!("unregistered {}", root.display());
        gui::info(&format!("Unregistered {}", root.display()));
        return Ok(());
    }

    shell::ensure_autostart(&data.join("autostart.off"));
    std::fs::create_dir_all(&root)?;
    let (tray, mut events) = gui::init(&data, &boot)?;
    tray.set_autostart(shell::autostart_enabled());

    if let Boot::Fresh(_) = &boot {
        gui::open_folder(&root);
    }
    let mut session = match &boot {
        Boot::Fresh(creds) | Boot::Restored(creds) => login(creds, &root, &data, &config, &tray).await,
        Boot::Prompt(_) | Boot::Idle(_) => None,
    };
    while let Some(event) = next(&mut session, &tray, &mut events).await {
        match event {
            TrayEvent::Quit => break,
            TrayEvent::Browse => gui::open_folder(&root),
            TrayEvent::Autostart => toggle_autostart(&data, &tray),
            TrayEvent::Login(creds) => {
                if let Some(old) = session.take() {
                    disconnect(old, &data, &tray, true).await;
                }
                gui::open_folder(&root);
                session = login(&creds, &root, &data, &config, &tray).await;
            }
            TrayEvent::Logout => {
                if let Some(session) = session.take() {
                    disconnect(session, &data, &tray, true).await;
                }
                tray.set_status(Status::LoggedOut);
            }
        }
    }
    if let Some(session) = session {
        disconnect(session, &data, &tray, false).await;
    }
    Ok(())
}

async fn next(
    session: &mut Option<Session>,
    tray: &Tray,
    events: &mut UnboundedReceiver<TrayEvent>,
) -> Option<TrayEvent> {
    let Some(session) = session else { return events.recv().await };
    let adapter = &session.adapter;

    loop {
        tokio::select! {
            event = events.recv() => {
                log::info!("session event: {event:?}");
                return event;
            },
            Some(path) = session.changes.recv() => {
                adapter.fs().on_change(&path).await;
            }
            Some((dir, newly)) = session.views.recv() => {
                let due = newly
                    || session.refreshed
                        .get(&dir)
                        .is_none_or(|at| at.elapsed() >= session.refresh_every);
                if due {
                    session.refreshed.insert(dir.clone(), Instant::now());
                    let adapter = adapter.clone();
                    tokio::spawn(async move {
                        if let Err(err) = adapter.fs().refresh(&dir).await {
                            log::warn!("refresh {dir}: {err}");
                        }
                    });
                }
            }
            _ = session.beat.tick() => {
                tray.set_status(match (adapter.upload_status(), adapter.busy()) {
                    (UploadStatus::Error, _) => Status::Error,
                    (UploadStatus::Busy, _) | (_, true) => Status::Syncing,
                    _ => Status::Ok,
                });
                tray.set_rates(&adapter.status().activity().snapshot());
            }
            _ = session.sweep.tick() => {
                if session.sweep_task.as_ref().is_none_or(|task| task.is_finished()) {
                    let adapter = adapter.clone();
                    session.sweep_task = Some(tokio::spawn(async move {
                        if let Err(err) = adapter.system().recover().await {
                            log::error!("sweep: {err}");
                        }
                    }));
                }
            }
        }
    }
}

struct Session {
    sdk: Arc<Sdk>,
    adapter: Arc<Adapter>,
    connection: wire::Connection,
    sync_root_id: String,
    changes: UnboundedReceiver<RelPath>,
    views: UnboundedReceiver<(RelPath, bool)>,
    refresh_every: Duration,
    beat: tokio::time::Interval,
    sweep: tokio::time::Interval,
    refreshed: HashMap<RelPath, Instant>,
    sweep_task: Option<tokio::task::JoinHandle<()>>,
}

async fn login(
    creds: &Credentials,
    root: &Path,
    data: &Path,
    config: &AppConfig,
    tray: &Tray,
) -> Option<Session> {
    tray.account(creds);
    tray.set_status(Status::Syncing);
    match connect(creds, root, data, config).await {
        Ok(session) => {
            let activity = session.adapter.status().activity();
            let root = root.to_path_buf();
            tray.on_click(move || gui::dashboard(activity.clone(), root.clone()));
            tray.set_status(Status::Ok);
            Some(session)
        }
        Err(err) => {
            log::error!("connect: {err}");
            tray.set_status(Status::Error);
            None
        }
    }
}

async fn connect(
    creds: &Credentials,
    root: &Path,
    data: &Path,
    config: &AppConfig,
) -> Result<Session, Box<dyn std::error::Error>> {
    let builder = Sdk::builder(&creds.url).insecure(creds.insecure);
    let sdk = if creds.token.is_empty() {
        builder.login(&creds.user, &creds.password, &creds.storage).await?
    } else {
        builder.token(creds.token.clone())?
    };
    store::remember(
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

    let sync_root_id = shell::sync_root_id(
        &config.windows.provider_name,
        &creds.account(),
        root,
    )?;
    shell::vacuum(&config.windows.provider_name, &sync_root_id);
    shell::register(
        root,
        &shell::Registration {
            id: sync_root_id.clone(),
            display_name: config.windows.provider_name.clone(),
            icon: config.windows.icon.clone().unwrap_or_else(shell::default_icon),
            allow_pinning: config.windows.allow_pinning,
            provider_id: wire::PROVIDER_ID,
        },
    )?;
    let connection = adapter.system().connect(root)?;
    log::info!("sync root {} connected", root.display());

    let (changes_tx, changes) = tokio::sync::mpsc::unbounded_channel();
    watcher::spawn(root, changes_tx)?;
    let (views_tx, views) = tokio::sync::mpsc::unbounded_channel();
    viewer::spawn(root, views_tx)?;
    adapter.system().recover().await?;

    let mut sweep = tokio::time::interval(Duration::from_secs(30));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    Ok(Session {
        sdk,
        adapter,
        connection,
        sync_root_id,
        changes,
        views,
        refresh_every: Duration::from_secs(config.windows.refresh_secs.max(2)),
        beat: tokio::time::interval(Duration::from_secs(1)),
        sweep,
        refreshed: HashMap::new(),
        sweep_task: None,
    })
}

async fn disconnect(session: Session, data: &Path, tray: &Tray, forget: bool) {
    log::info!("disconnecting");
    tray.set_status(Status::Syncing);
    tray.reset();
    session.adapter.system().flush(Duration::from_secs(30)).await;
    if forget {
        if let Err(err) = session.adapter.system().vacuum() {
            log::warn!("vacuum on logout: {err}");
        }
    }
    drop(session.connection);
    if forget {
        match shell::unregister(&session.sync_root_id) {
            Ok(()) => log::info!("sync root unregistered"),
            Err(err) => log::warn!("unregister on logout: {err}"),
        }
        store::forget(data);
        let _ = session.sdk.logout().await;
    }
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

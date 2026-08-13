#![windows_subsystem = "windows"]

mod args;
mod log;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::config as session;
use fdrive_core::engine::UploadStatus;
use fdrive_core::path::RelPath;
use fdrive_core::sdk::Sdk;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use fdrive_windows::adapter::Adapter;
use fdrive_windows::config::AppConfig;
use fdrive_windows::gui::{self, Credentials, Status, Tray, TrayEvent};
use fdrive_windows::wire::{self, shell, viewer, watcher};

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args::Setup {
        root,
        data,
        config,
        unregister,
        prefill_url,
        mut credentials,
        prompt_login,
        mut fresh_credentials,
    } = args::init();
    log::init(&data)?;
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
    }));
    log::info!("fdrive-windows {} starting", env!("CARGO_PKG_VERSION"));

    if unregister {
        shell::vacuum(&config.windows.provider_name, "");
        let _ = wire::unregister(&root);
        session::forget(&data);
        log::info!("unregistered {}", root.display());
        gui::info(&format!("Unregistered {}", root.display()));
        return Ok(());
    }

    shell::ensure_autostart(&data.join("autostart.off"));
    let (tray, mut events) = gui::init(&data, prefill_url, prompt_login)?;

    'app: loop {
        if let Some(creds) = credentials.as_ref() {
            tray.account(creds);
            tray.set_status(Status::Syncing);
            match run(
                creds,
                &config,
                &root,
                &data,
                &mut events,
                &tray,
                fresh_credentials,
            )
            .await
            {
                Ok(SessionEnd::Quit) => break 'app,
                Ok(SessionEnd::Logout) => {
                    credentials = None;
                    tray.set_status(Status::LoggedOut);
                }
                Ok(SessionEnd::Restart) => {}
                Err(err) => {
                    log::error!("session: {err}");
                    credentials = None;
                    tray.set_status(Status::Error);
                }
            }
            fresh_credentials = false;
            continue;
        }
        match events.recv().await {
            None | Some(TrayEvent::Quit) => break 'app,
            Some(TrayEvent::Login(creds)) => {
                credentials = Some(creds);
                fresh_credentials = true;
            }
            Some(TrayEvent::Browse) => gui::open_folder(&root),
            Some(TrayEvent::Restart | TrayEvent::Logout | TrayEvent::Refresh) => {}
        }
    }
    Ok(())
}

async fn run(
    creds: &Credentials,
    config: &AppConfig,
    root: &Path,
    data: &Path,
    events: &mut UnboundedReceiver<TrayEvent>,
    tray: &Tray,
    browse: bool,
) -> Result<SessionEnd, Box<dyn std::error::Error>> {
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
    let adapter = Adapter::new(
        sdk.clone(),
        tokio::runtime::Handle::current(),
        root.to_path_buf(),
        data,
    )?;
    let mut upload_status = adapter.status().watch();

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
    if browse {
        gui::open_folder(root);
    }

    let (changes_tx, mut changes) = tokio::sync::mpsc::unbounded_channel();
    watcher::spawn(root, changes_tx)?;
    let (views_tx, mut views) = tokio::sync::mpsc::unbounded_channel();
    viewer::spawn(root, views_tx)?;
    adapter.system().recover().await?;

    tray.attach(adapter.status().activity());
    tray.set_status(Status::Ok);
    let refresh_every = Duration::from_secs(config.windows.refresh_secs.max(2));
    let mut refreshed: HashMap<RelPath, Instant> = HashMap::new();
    let mut sweep_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut sweep = tokio::time::interval(Duration::from_secs(30));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let end = loop {
        tokio::select! {
            event = events.recv() => {
                log::info!("session event: {event:?}");
                match event {
                    None | Some(TrayEvent::Quit) => break SessionEnd::Quit,
                    Some(TrayEvent::Logout) => break SessionEnd::Logout,
                    Some(TrayEvent::Restart) => break SessionEnd::Restart,
                    Some(TrayEvent::Browse) => gui::open_folder(root),
                    Some(TrayEvent::Refresh) => {
                        let adapter = adapter.clone();
                        let tray = tray.clone();
                        let status = adapter.status().watch();
                        tokio::spawn(async move {
                            tray.set_status(Status::Syncing);
                            if let Err(err) = adapter.system().resync().await {
                                log::warn!("refresh: {err}");
                            }
                            tray.set_status(tray_status(*status.borrow()));
                        });
                    }
                    Some(TrayEvent::Login(_)) => {}
                }
            },
            Some(path) = changes.recv() => {
                adapter.fs().on_change(&path).await;
            }
            Some((dir, newly)) = views.recv() => {
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
            _ = upload_status.changed() => {
                tray.set_status(tray_status(*upload_status.borrow()));
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
    };

    log::info!("disconnecting");
    adapter.system().flush(Duration::from_secs(30)).await;
    if matches!(end, SessionEnd::Logout) {
        connection.disconnect();
        if let Err(err) = adapter.system().vacuum() {
            log::warn!("vacuum on logout: {err}");
        }
        match shell::unregister(&sync_root_id) {
            Ok(()) => log::info!("sync root unregistered"),
            Err(err) => log::warn!("unregister on logout: {err}"),
        }
        session::forget(data);
        let _ = sdk.logout().await;
    } else {
        connection.disconnect();
    }
    Ok(end)
}

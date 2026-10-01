use std::path::Path;
use std::time::Duration;

mod app;
mod log;

use fdrive_core::config as store;
use fdrive_core::engine::UploadStatus;
use fdrive_linux::gui::{self, Boot, Credentials, Status, Tray, TrayEvent};
use fdrive_linux::session;
use tokio::sync::mpsc::UnboundedReceiver;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app::Setup {
        mount,
        data,
        prefill,
        boot,
    } = app::init()?;

    match gui::init(data.clone(), mount.clone(), &boot).await {
        Ok((tray, mut events)) => {
            let mut session = match &boot {
                Boot::Fresh(creds) => match connect(creds, &mount, &data, Some(&tray)).await {
                    Ok(session) => Some(session),
                    Err(err) => {
                        tray.shutdown().await;
                        return Err(err);
                    }
                },
                Boot::Restored(creds) => connect(creds, &mount, &data, Some(&tray)).await.ok(),
                Boot::Prompt | Boot::Idle => None,
            };
            while let Some(event) = next_gui(&mut session, &tray, &mut events).await {
                match event {
                    TrayEvent::Quit => break,
                    TrayEvent::Login => {
                        if let Some(creds) = tray.login(prefill.clone()).await {
                            if let Some(old) = session.take() {
                                disconnect(old, &data, true, Some(&tray)).await;
                            }
                            session = connect(&creds, &mount, &data, Some(&tray)).await.ok();
                        }
                    }
                    TrayEvent::Logout => {
                        if let Some(session) = session.take() {
                            disconnect(session, &data, true, Some(&tray)).await;
                        }
                        tray.set(Status::LoggedOut, false).await;
                    }
                }
            }
            if let Some(session) = session {
                disconnect(session, &data, false, Some(&tray)).await;
            }
            tray.shutdown().await;
        },
        Err(_err) => {
            let mut session = match &boot {
                Boot::Fresh(creds) => match connect(creds, &mount, &data, None).await {
                    Ok(session) => Some(session),
                    Err(err) => {
                        return Err(err);
                    }
                },
                Boot::Restored(creds) => connect(creds, &mount, &data, None).await.ok(),
                Boot::Prompt | Boot::Idle => match gui::login_tty(&prefill).await {
                    Some(creds) => Some(connect(&creds, &mount, &data, None).await?),
                    None => None,
                },
            };
            next_tty(&mut session).await;
            if let Some(session) = session {
                disconnect(session, &data, false, None).await;
            }
        }
    }
    Ok(())
}

struct Session {
    inner: session::Session,
    upload_status: tokio::sync::watch::Receiver<UploadStatus>,
    fuse_watch: tokio::time::Interval,
}

async fn connect(
    creds: &Credentials,
    mount: &Path,
    data: &Path,
    tray: Option<&Tray>,
) -> Result<Session, Box<dyn std::error::Error>> {
    if let Some(tray) = tray {
        tray.set(Status::Syncing, true).await;
    }
    let creds = store::Session {
        url: creds.url.clone(),
        token: creds.token.clone(),
        insecure: creds.insecure,
    };
    match session::connect(&creds, mount, data).await {
        Ok(inner) => {
            if let Some(tray) = tray {
                tray.attach(inner.adapter.status().activity()).await;
                tray.set(Status::Ok, true).await;
            }
            Ok(Session {
                upload_status: inner.adapter.status().watch(),
                fuse_watch: tokio::time::interval(Duration::from_secs(2)),
                inner,
            })
        }
        Err(err) => {
            log::error!("connect: {err}");
            if let Some(tray) = tray {
                tray.set(Status::Error, false).await;
            }
            Err(err)
        }
    }
}

async fn disconnect(session: Session, data: &Path, forget: bool, tray: Option<&Tray>) {
    if let Some(tray) = tray {
        tray.set(Status::Syncing, true).await;
    }
    session::disconnect(session.inner, data, forget).await;
}

async fn next_gui(
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
                if session.inner.fuse.guard.is_finished() {
                    log::info!("unmounted");
                    return None;
                }
            }
        }
    }
}

async fn next_tty(session: &mut Option<Session>) {
    let Some(session) = session else {
        log::info!("session not defined ...");
        return
    };
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("exiting ...");
                return
            },
            _ = session.fuse_watch.tick() => {
                if session.inner.fuse.guard.is_finished() {
                    log::info!("unmounted");
                    return;
                }
            }
        }
    }
}

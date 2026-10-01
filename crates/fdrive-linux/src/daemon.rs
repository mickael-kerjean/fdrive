use std::path::PathBuf;
use std::sync::Arc;

use fdrive_linux::drive::Drive;
use fdrive_linux::session;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};

use crate::ipc::{self, Request};

pub async fn run(mount: PathBuf, data: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    session::instance_lock(&data)?;
    let socket = ipc::socket_path()?;
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    log::info!("listening on {}", socket.display());

    let drive = Drive::new(mount, data);
    drive.start();

    let mut term = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => { tokio::spawn(serve(drive.clone(), stream)); }
                Err(err) => log::warn!("accept: {err}"),
            },
            _ = tokio::signal::ctrl_c() => break,
            _ = term.recv() => break,
        }
    }
    let _ = std::fs::remove_file(&socket);
    drive.stop().await;
    Ok(())
}

async fn serve(drive: Arc<Drive>, stream: UnixStream) {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    let _ = stream.read_line(&mut line).await;
    let reply = match serde_json::from_str(&line) {
        Ok(request) => handle(&drive, request).await,
        Err(err) => json!({ "error": format!("bad request: {err}") }),
    };
    if let Err(err) = stream.get_mut().write_all(format!("{reply}\n").as_bytes()).await {
        log::debug!("reply: {err}");
    }
}

async fn handle(drive: &Drive, request: Request) -> Value {
    match request {
        Request::Status => json!(drive.state().await),
        Request::Login { server, token } => match drive.login(&server, token).await {
            Ok(()) => json!({}),
            Err(err) => json!({ "error": err }),
        },
        Request::Logout => {
            drive.logout().await;
            json!({})
        }
        Request::Clear => {
            drive.clear().await;
            json!({})
        }
    }
}

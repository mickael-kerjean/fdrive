use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::activity::{rate_line, sparkline, Direction, Outcome, Transfer};
use fdrive_core::config as store;
use fdrive_core::engine::UploadStatus;
use fdrive_core::sdk::{normalize_server, Sdk};
use fdrive_linux::session::{self, Session};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::Mutex;

use crate::ipc::{self, Request};

const RETRY: Duration = Duration::from_secs(30);

pub struct Daemon {
    mount: PathBuf,
    data: PathBuf,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    connecting: bool,
    last_error: String,
}

pub async fn run(mount: PathBuf, data: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    session::instance_lock(&data)?;
    let socket = ipc::socket_path()?;
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    log::info!("listening on {}", socket.display());

    let daemon = Arc::new(Daemon {
        mount,
        data,
        inner: Mutex::new(Inner::default()),
    });
    tokio::spawn(daemon.clone().restore());
    tokio::spawn(daemon.clone().supervise());

    let mut term = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => { tokio::spawn(daemon.clone().serve(stream)); }
                Err(err) => log::warn!("accept: {err}"),
            },
            _ = tokio::signal::ctrl_c() => break,
            _ = term.recv() => break,
        }
    }
    let _ = std::fs::remove_file(&socket);
    if let Some(session) = daemon.inner.lock().await.session.take() {
        session::disconnect(session, &daemon.data, false).await;
    }
    Ok(())
}

impl Daemon {
    async fn serve(self: Arc<Self>, stream: UnixStream) {
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        let _ = stream.read_line(&mut line).await;
        let reply = match serde_json::from_str(&line) {
            Ok(request) => self.handle(request).await,
            Err(err) => json!({ "error": format!("bad request: {err}") }),
        };
        if let Err(err) = stream.get_mut().write_all(format!("{reply}\n").as_bytes()).await {
            log::debug!("reply: {err}");
        }
    }

    async fn handle(&self, request: Request) -> Value {
        match request {
            Request::Status => self.state().await,
            Request::Login { server, token } => match self.login(&server, token).await {
                Ok(()) => json!({}),
                Err(err) => json!({ "error": err }),
            },
            Request::Logout => {
                let session = self.inner.lock().await.session.take();
                if let Some(session) = session {
                    session::disconnect(session, &self.data, true).await;
                } else {
                    store::forget(&self.data);
                }
                self.inner.lock().await.last_error.clear();
                json!({})
            }
            Request::Clear => {
                if let Some(session) = self.inner.lock().await.session.as_ref() {
                    session.adapter.status().activity().clear();
                }
                json!({})
            }
        }
    }

    async fn state(&self) -> Value {
        let inner = self.inner.lock().await;
        let saved = store::load(&self.data);
        let phase = match &inner.session {
            Some(session) => match *session.adapter.status().watch().borrow() {
                UploadStatus::Idle => "ok",
                UploadStatus::Busy => "syncing",
                UploadStatus::Error => "error",
            },
            None if inner.connecting => "connecting",
            None if saved.ok() && !inner.last_error.is_empty() => "error",
            None => "loggedOut",
        };
        let snap = inner.session.as_ref().map(|s| s.adapter.status().activity().snapshot());
        json!({
            "phase": phase,
            "server": saved.url,
            "mount": self.mount,
            "lastError": inner.last_error,
            "sparkline": snap.as_ref().map(|s| sparkline(s, 24)).unwrap_or_default(),
            "rate": snap.as_ref().map(rate_line).unwrap_or_default(),
            "transfers": snap.map(|s| s.transfers.iter().map(transfer).collect()).unwrap_or_else(Vec::new),
        })
    }

    async fn login(&self, server: &str, token: String) -> Result<(), String> {
        let url = normalize_server(server);
        let sdk = Sdk::builder(&url)
            .token(token.clone())
            .map_err(|err| err.to_string())?;
        sdk.ls("/").await.map_err(|err| format!("token rejected: {err}"))?;
        let previous = self.inner.lock().await.session.take();
        if let Some(previous) = previous {
            let forget = store::load(&self.data).url != url;
            session::disconnect(previous, &self.data, forget).await;
        }
        self.connect(&store::Session { url, token, insecure: false }).await
    }

    async fn connect(&self, creds: &store::Session) -> Result<(), String> {
        {
            let mut inner = self.inner.lock().await;
            if inner.connecting || inner.session.is_some() {
                return Err("already connecting, try again in a moment".to_owned());
            }
            inner.connecting = true;
        }
        let result = session::connect(creds, &self.mount, &self.data).await.map_err(|err| err.to_string());
        let mut inner = self.inner.lock().await;
        inner.connecting = false;
        match result {
            Ok(session) => {
                inner.session = Some(session);
                inner.last_error.clear();
                Ok(())
            }
            Err(err) => {
                log::error!("connect: {err}");
                inner.last_error = err.clone();
                Err(err)
            }
        }
    }

    async fn restore(self: Arc<Self>) {
        loop {
            let saved = store::load(&self.data);
            if !saved.ok() || self.inner.lock().await.session.is_some() {
                return;
            }
            if self.connect(&saved).await.is_ok() {
                return;
            }
            tokio::time::sleep(RETRY).await;
        }
    }

    async fn supervise(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            let mut inner = self.inner.lock().await;
            if inner.session.as_ref().is_some_and(|s| s.fuse.guard.is_finished()) {
                log::info!("unmounted");
                let session = inner.session.take().expect("checked above");
                inner.last_error = "the folder was unmounted".to_owned();
                drop(inner);
                session::disconnect(session, &self.data, false).await;
            }
        }
    }
}

fn transfer(t: &Transfer) -> Value {
    let (outcome, error) = match &t.outcome {
        Outcome::Running => ("running", ""),
        Outcome::Done => ("done", ""),
        Outcome::Failed(err) => ("failed", err.as_str()),
    };
    json!({
        "path": t.path,
        "direction": match t.direction {
            Direction::Up => "up",
            Direction::Down => "down",
        },
        "size": t.size,
        "progress": t.progress,
        "outcome": outcome,
        "error": error,
    })
}

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::activity::{fmt_bytes, rate_line, sparkline, Direction, Outcome};
use fdrive_core::config as store;
use fdrive_core::engine::UploadStatus;
use fdrive_core::sdk::{normalize_server, Sdk};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::session::{self, Session};

const RETRY: Duration = Duration::from_secs(30);

pub struct Drive {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub phase: Phase,
    pub phase_text: &'static str,
    pub server: String,
    pub host: String,
    pub mount: PathBuf,
    pub last_error: String,
    pub sparkline: String,
    pub rate: String,
    pub transfers: Vec<Transfer>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Ok,
    Syncing,
    Connecting,
    Error,
    LoggedOut,
}

impl Phase {
    pub fn text(self) -> &'static str {
        match self {
            Phase::Ok => "Up to date",
            Phase::Syncing => "Syncing",
            Phase::Connecting => "Connecting",
            Phase::Error => "Sync trouble",
            Phase::LoggedOut => "Not signed in",
        }
    }
}

#[derive(Serialize)]
pub struct Transfer {
    pub path: String,
    pub name: String,
    pub detail: String,
    pub uri: String,
    pub direction: &'static str,
    pub size: u64,
    pub progress: u64,
    pub outcome: &'static str,
    pub error: String,
}

impl Drive {
    pub fn new(mount: PathBuf, data: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            mount,
            data,
            inner: Mutex::new(Inner::default()),
        })
    }

    pub fn start(self: &Arc<Self>) {
        tokio::spawn(self.clone().restore());
        tokio::spawn(self.clone().supervise());
    }

    pub async fn stop(&self) {
        if let Some(session) = self.inner.lock().await.session.take() {
            session::disconnect(session, &self.data, false).await;
        }
    }

    pub async fn state(&self) -> State {
        let inner = self.inner.lock().await;
        let saved = store::load(&self.data);
        let phase = match &inner.session {
            Some(session) => match *session.adapter.status().watch().borrow() {
                UploadStatus::Idle => Phase::Ok,
                UploadStatus::Busy => Phase::Syncing,
                UploadStatus::Error => Phase::Error,
            },
            None if inner.connecting => Phase::Connecting,
            None if saved.ok() && !inner.last_error.is_empty() => Phase::Error,
            None => Phase::LoggedOut,
        };
        let snap = inner.session.as_ref().map(|s| s.adapter.status().activity().snapshot());
        State {
            phase,
            phase_text: phase.text(),
            host: host_of(&saved.url),
            server: saved.url,
            mount: self.mount.clone(),
            last_error: inner.last_error.clone(),
            sparkline: snap.as_ref().map(|s| sparkline(s, 24)).unwrap_or_default(),
            rate: snap.as_ref().map(rate_line).unwrap_or_default(),
            transfers: snap
                .map(|s| s.transfers.iter().map(|t| transfer(&self.mount, t)).collect())
                .unwrap_or_default(),
        }
    }

    pub async fn login(&self, server: &str, token: String) -> Result<(), String> {
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

    pub async fn logout(&self) {
        let session = self.inner.lock().await.session.take();
        if let Some(session) = session {
            session::disconnect(session, &self.data, true).await;
        } else {
            store::forget(&self.data);
        }
        self.inner.lock().await.last_error.clear();
    }

    pub async fn clear(&self) {
        if let Some(session) = self.inner.lock().await.session.as_ref() {
            session.adapter.status().activity().clear();
        }
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

fn host_of(server: &str) -> String {
    let rest = server.split_once("://").map_or(server, |(_, rest)| rest);
    rest.split('/').next().unwrap_or_default().to_owned()
}

fn transfer(mount: &std::path::Path, t: &fdrive_core::activity::Transfer) -> Transfer {
    let (outcome, error) = match &t.outcome {
        Outcome::Running => ("running", String::new()),
        Outcome::Done => ("done", String::new()),
        Outcome::Failed(err) => ("failed", err.clone()),
    };
    let (folder, name) = t.path.rsplit_once('/').unwrap_or(("", &t.path));
    let folder = if folder.is_empty() { "/" } else { folder };
    let detail = match &t.outcome {
        Outcome::Failed(err) => format!("Failed · {}", if err.is_empty() { folder } else { err }),
        Outcome::Running => {
            let percent = if t.size > 0 { 100 * t.progress / t.size } else { 0 };
            format!("{percent}% of {} · {folder}", fmt_bytes(t.size))
        }
        Outcome::Done => format!("{} · {folder}", fmt_bytes(t.size)),
    };
    let uri = url::Url::from_file_path(mount.join(t.path.trim_start_matches('/')))
        .map(String::from)
        .unwrap_or_default();
    Transfer {
        path: t.path.clone(),
        name: name.to_owned(),
        detail,
        uri,
        direction: match t.direction {
            Direction::Up => "up",
            Direction::Down => "down",
        },
        size: t.size,
        progress: t.progress,
        outcome,
        error,
    }
}

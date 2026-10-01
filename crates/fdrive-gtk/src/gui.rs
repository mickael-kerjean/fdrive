use std::path::PathBuf;

use tokio::sync::mpsc::UnboundedReceiver;

mod dashboard;
mod login_gui;
mod login_tty;
mod tray;
mod webview;

pub use fdrive_linux::session::default_data;
pub use fdrive_core::sdk::normalize_server;
pub use login_tty::login_tty;
pub use tray::Tray;

pub async fn init(data: PathBuf, mount: PathBuf, boot: &Boot) -> std::io::Result<(Tray, UnboundedReceiver<TrayEvent>)> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    if let Boot::Prompt = boot {
        let _ = tx.send(TrayEvent::Login);
    }
    let tray = Tray::spawn(tx, data, mount)
        .await
        .map_err(|err| std::io::Error::other(format!("could not start the tray: {err}")))?;
    Ok((tray, rx))
}

#[derive(Debug)]
pub enum Boot {
    Fresh(Credentials),
    Restored(Credentials),
    Prompt,
    Idle,
}

#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub url: String,
    pub token: String,
    pub insecure: bool,
}

impl From<fdrive_core::config::Session> for Credentials {
    fn from(session: fdrive_core::config::Session) -> Self {
        Self {
            url: session.url,
            token: session.token,
            insecure: session.insecure,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TrayEvent {
    Login,
    Logout,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    LoggedOut,
    Ok,
    Syncing,
    Error,
}

impl Status {
    fn tip(self) -> &'static str {
        match self {
            Self::LoggedOut => "Filestash — not signed in",
            Self::Ok => "Filestash",
            Self::Syncing => "Filestash — syncing",
            Self::Error => "Filestash — sync error",
        }
    }

    fn icon_name(self) -> &'static str {
        match self {
            Self::LoggedOut => "icon-base",
            Self::Ok => "icon-ok",
            Self::Syncing => "icon-sync",
            Self::Error => "icon-error",
        }
    }
}

#[cfg(test)]
#[path = "gui_test.rs"]
mod tests;

use std::path::PathBuf;

mod dashboard;
mod login;
mod tray;

pub use fdrive_core::sdk::normalize_server;
pub use tray::Tray;

#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub url: String,
    pub token: String,
    pub user: String,
    pub password: String,
    pub storage: String,
    pub insecure: bool,
}

impl From<fdrive_core::config::Session> for Credentials {
    fn from(session: fdrive_core::config::Session) -> Self {
        Self {
            url: session.url,
            token: session.token,
            insecure: session.insecure,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub enum TrayEvent {
    Login,
    Logout,
    Restart,
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

pub fn default_data() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("filestash")
}

#[cfg(test)]
#[path = "gui_test.rs"]
mod tests;

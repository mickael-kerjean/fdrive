use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetDlgItem, GetWindowTextLengthW, GetWindowTextW, SetWindowTextW, SW_SHOWNORMAL,
};

use crate::utils::wstr;

mod alert;
mod dashboard;
mod login;
mod tray;
mod webview;

pub use alert::{alert, info};
pub use dashboard::toggle as dashboard;
pub use tray::Tray;

pub fn init(
    data: &std::path::Path,
    boot: &Boot,
) -> std::io::Result<(Tray, tokio::sync::mpsc::UnboundedReceiver<TrayEvent>)> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let tray = tray::spawn(
        Arc::new(Mutex::new(TrayState {
            url: boot.url(),
            ..Default::default()
        })),
        tx,
        data.to_path_buf(),
    )?;
    if let Boot::Prompt(_) = boot {
        tray.prompt_login();
    }
    Ok((tray, rx))
}

#[derive(Debug)]
pub enum Boot {
    Fresh(Credentials),
    Restored(Credentials),
    Prompt(String),
    Idle(Option<String>),
}

impl Boot {
    fn url(&self) -> Option<String> {
        match self {
            Boot::Fresh(creds) | Boot::Restored(creds) => Some(creds.url.clone()),
            Boot::Prompt(url) => Some(url.clone()),
            Boot::Idle(url) => url.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub url: String,
    pub token: String,
    pub user: String,
    pub password: String,
    pub storage: String,
    pub insecure: bool,
}

impl Credentials {
    pub fn account(&self) -> String {
        let rest = self
            .url
            .split_once("://")
            .map_or(self.url.as_str(), |(_, rest)| rest);
        let host = rest.split(['/', '?']).next().unwrap_or(rest);
        match self.user.is_empty() {
            true => host.to_string(),
            false => format!("{}@{host}/{}", self.user, self.storage),
        }
    }
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

#[derive(Debug)]
pub enum TrayEvent {
    Browse,
    Autostart,
    Login(Credentials),
    Logout,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Status {
    #[default]
    LoggedOut,
    Ok,
    Syncing,
    Error,
}

impl Status {
    fn icon_bytes(self) -> &'static [u8] {
        match self {
            Status::LoggedOut => include_bytes!(concat!(env!("OUT_DIR"), "/tray-unlogged.ico")),
            Status::Ok => include_bytes!(concat!(env!("OUT_DIR"), "/tray-ok.ico")),
            Status::Syncing => include_bytes!(concat!(env!("OUT_DIR"), "/tray-sync.ico")),
            Status::Error => include_bytes!(concat!(env!("OUT_DIR"), "/tray-error.ico")),
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Status::LoggedOut => "Filestash — not signed in",
            Status::Ok => "Filestash",
            Status::Syncing => "Filestash — syncing",
            Status::Error => "Filestash — sync error",
        }
    }
}

#[derive(Default)]
pub struct TrayState {
    pub status: Status,
    pub url: Option<String>,
    pub user: String,
    pub storage: String,
    pub rates: String,
    pub autostart: bool,
    pub on_click: Option<Arc<dyn Fn() + Send + Sync>>,
}

struct Ctx {
    state: Arc<Mutex<TrayState>>,
    events: tokio::sync::mpsc::UnboundedSender<TrayEvent>,
    data: PathBuf,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

fn ctx<T>(f: impl FnOnce(&Ctx) -> T) -> T {
    CTX.with_borrow(|ctx| f(ctx.as_ref().expect("tray ctx")))
}

fn state<T>(f: impl FnOnce(&mut TrayState) -> T) -> T {
    ctx(|ctx| f(&mut ctx.state.lock().unwrap()))
}

fn send(event: TrayEvent) {
    ctx(|ctx| {
        let _ = ctx.events.send(event);
    });
}

pub fn open_folder(path: &std::path::Path) {
    let wide = wstr(path);
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

fn set_text(hwnd: HWND, id: i32, text: &str) {
    let wide = wstr(text);
    unsafe {
        if let Ok(ctl) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowTextW(ctl, PCWSTR(wide.as_ptr()));
        }
    }
}

fn get_text(hwnd: HWND, id: i32) -> String {
    unsafe {
        let Ok(ctl) = GetDlgItem(Some(hwnd), id) else {
            return String::new();
        };
        let len = GetWindowTextLengthW(ctl);
        let mut buf = vec![0u16; len as usize + 1];
        let got = GetWindowTextW(ctl, &mut buf);
        String::from_utf16_lossy(&buf[..got as usize])
    }
}

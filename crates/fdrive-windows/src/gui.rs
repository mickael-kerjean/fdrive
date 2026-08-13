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
pub use tray::Tray;

pub fn init(
    data: &std::path::Path,
    prefill_url: Option<String>,
    prompt_login: bool,
) -> std::io::Result<(Tray, tokio::sync::mpsc::UnboundedReceiver<TrayEvent>)> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let tray = tray::spawn(
        Arc::new(Mutex::new(TrayState {
            url: prefill_url,
            ..Default::default()
        })),
        tx,
        data.join("fdrive.log"),
        data.join("autostart.off"),
    )?;
    if prompt_login {
        tray.prompt_login();
    }
    Ok((tray, rx))
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
    Refresh,
    Login(Credentials),
    Logout,
    Restart,
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
    pub activity: Option<Arc<fdrive_core::activity::Activity>>,
}

struct Ctx {
    state: Arc<Mutex<TrayState>>,
    events: tokio::sync::mpsc::UnboundedSender<TrayEvent>,
    log_path: PathBuf,
    autostart_opt_out: PathBuf,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
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

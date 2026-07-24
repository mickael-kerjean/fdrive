use std::cell::RefCell;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetDlgItem, GetWindowTextLengthW, GetWindowTextW, MessageBoxW, SetWindowTextW, MB_ICONERROR,
    MB_ICONINFORMATION, MB_OK, MESSAGEBOX_STYLE, SW_SHOWNORMAL,
};

mod dashboard;
mod login;
mod tray;

pub use tray::{spawn, Tray};

pub fn alert(message: &str) {
    message_box(message, MB_ICONERROR);
}

pub fn info(message: &str) {
    message_box(message, MB_ICONINFORMATION);
}

fn message_box(message: &str, icon: MESSAGEBOX_STYLE) {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), w!("Filestash"), MB_OK | icon);
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

fn wide_path(path: &std::path::Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn open_folder(path: &std::path::Path) {
    let wide = wide_path(path);
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
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
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

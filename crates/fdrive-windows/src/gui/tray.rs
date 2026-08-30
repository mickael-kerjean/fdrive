use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage,
    PostThreadMessageW, RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage,
    HICON, IDI_APPLICATION, IMAGE_FLAGS, MF_CHECKED, MF_SEPARATOR, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_LBUTTONUP,
    WM_RBUTTONUP, WNDCLASSW,
};

use super::login::prompt_login;
use super::{send, state, Credentials, Ctx, Status, TrayEvent, TrayState, CTX};

const WM_TRAY: u32 = WM_APP + 1;
const WM_TRAY_REFRESH: u32 = WM_APP + 2;
const WM_TRAY_LOGIN: u32 = WM_APP + 3;
const CMD_BROWSE: usize = 1;
const CMD_LOGIN: usize = 2;
const CMD_LOGOUT: usize = 3;
const CMD_QUIT: usize = 4;
const CMD_AUTOSTART: usize = 5;

#[derive(Clone)]
pub struct Tray {
    state: Arc<Mutex<TrayState>>,
    thread: u32,
}

impl Tray {
    pub fn set_status(&self, status: Status) {
        {
            let mut state = self.state.lock().unwrap();
            if state.status == status {
                return;
            }
            state.status = status;
        }
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_TRAY_REFRESH, WPARAM(0), LPARAM(0));
        }
    }

    pub fn account(&self, creds: &Credentials) {
        let mut state = self.state.lock().unwrap();
        state.url = Some(creds.url.clone());
        state.user = creds.user.clone();
        state.storage = creds.storage.clone();
    }

    pub fn on_click(&self, handler: impl Fn() + Send + Sync + 'static) {
        self.state.lock().unwrap().on_click = Some(Arc::new(handler));
    }

    pub fn set_autostart(&self, enabled: bool) {
        self.state.lock().unwrap().autostart = enabled;
    }

    pub fn reset(&self) {
        self.state.lock().unwrap().on_click = None;
    }

    pub fn prompt_login(&self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_TRAY_LOGIN, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn spawn(
    state: Arc<Mutex<TrayState>>,
    events: tokio::sync::mpsc::UnboundedSender<TrayEvent>,
    data: PathBuf,
) -> std::io::Result<Tray> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let thread_state = state.clone();
    std::thread::Builder::new()
        .name("fdrive-tray".into())
        .spawn(move || {
            let _ = ready_tx.send(unsafe { GetCurrentThreadId() });
            if let Err(err) = tray_thread(thread_state, events, data) {
                log::error!("tray: {err}");
            }
        })?;
    let thread = ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| std::io::Error::other("tray thread did not start"))?;
    Ok(Tray { state, thread })
}

fn tray_thread(
    state: Arc<Mutex<TrayState>>,
    events: tokio::sync::mpsc::UnboundedSender<TrayEvent>,
    data: PathBuf,
) -> windows::core::Result<()> {
    CTX.with_borrow_mut(|ctx| {
        *ctx = Some(Ctx {
            state,
            events,
            data,
        })
    });
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(tray_wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("fdrive_tray"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("fdrive_tray"),
            w!("Filestash"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        let mut data = icon_data(hwnd);
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            log::error!("Shell_NotifyIconW failed; no tray icon");
        }
        log::info!("tray icon up");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if msg.hwnd.is_invalid() && msg.message == WM_TRAY_REFRESH {
                data = icon_data(hwnd);
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
                continue;
            }
            if msg.hwnd.is_invalid() && msg.message == WM_TRAY_LOGIN {
                prompt_login();
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
    Ok(())
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let status =
        state(|state| state.status);
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: status_icon(status),
        ..Default::default()
    };
    let tip: Vec<u16> = status.tip().encode_utf16().take(127).collect();
    data.szTip[..tip.len()].copy_from_slice(&tip);
    data
}

fn status_icon(status: Status) -> HICON {
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<Status, HICON>> =
            RefCell::new(std::collections::HashMap::new());
    }
    CACHE.with_borrow_mut(|cache| {
        *cache.entry(status).or_insert_with(|| {
            icon_from_ico(status.icon_bytes())
                .unwrap_or_else(|| unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() })
        })
    })
}

fn icon_from_ico(bytes: &[u8]) -> Option<HICON> {
    let count = u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]) as usize;
    let mut best: Option<(u32, usize, usize)> = None;
    for i in 0..count {
        let entry = bytes.get(6 + i * 16..6 + i * 16 + 16)?;
        let width = if entry[0] == 0 {
            256
        } else {
            u32::from(entry[0])
        };
        let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        let fit = width.abs_diff(32);
        if best.is_none_or(|(best_fit, ..)| fit < best_fit) {
            best = Some((fit, offset, size));
        }
    }
    let (_, offset, size) = best?;
    let frame = bytes.get(offset..offset + size)?;
    unsafe { CreateIconFromResourceEx(frame, true, 0x0003_0000, 32, 32, IMAGE_FLAGS(0)).ok() }
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => {
            let mouse = lparam.0 as u32;
            if mouse == WM_LBUTTONUP {
                let on_click = state(|state| state.on_click.clone());
                match on_click {
                    Some(on_click) => on_click(),
                    None => show_menu(hwnd),
                }
            } else if mouse == WM_RBUTTONUP {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let (logged_in, autostart_on) =
        state(|state| (state.status != Status::LoggedOut, state.autostart));
    let Ok(menu) = CreatePopupMenu() else { return };
    let autostart = if autostart_on {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    if logged_in {
        let _ = AppendMenuW(menu, MF_STRING, CMD_BROWSE, w!("Browse"));
        let _ = AppendMenuW(menu, autostart, CMD_AUTOSTART, w!("Autostart"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_LOGOUT, w!("Logout"));
    } else {
        let _ = AppendMenuW(menu, MF_STRING, CMD_LOGIN, w!("Login"));
        let _ = AppendMenuW(menu, autostart, CMD_AUTOSTART, w!("Autostart"));
    }
    let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit"));

    let mut point = Default::default();
    let _ = GetCursorPos(&mut point);
    let _ = SetForegroundWindow(hwnd);
    let picked = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN,
        point.x,
        point.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);

    match picked.0 as usize {
        CMD_BROWSE => send(TrayEvent::Browse),
        CMD_LOGIN => prompt_login(),
        CMD_LOGOUT => send(TrayEvent::Logout),
        CMD_AUTOSTART => send(TrayEvent::Autostart),
        CMD_QUIT => send(TrayEvent::Quit),
        _ => {}
    }
}

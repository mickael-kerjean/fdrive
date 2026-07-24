use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, COLOR_3DFACE, DEFAULT_GUI_FONT, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgItem, GetMessageW,
    IsDialogMessageW, LoadIconW, PeekMessageW, RegisterClassW, SendMessageW, SetForegroundWindow,
    TranslateMessage, BS_DEFPUSHBUTTON, CW_USEDEFAULT, ES_AUTOHSCROLL, HMENU, MSG, PM_REMOVE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

use super::{alert, get_text, set_text, Credentials, CTX};

const ID_SERVER: i32 = 101;
const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;

thread_local! {
    static DIALOG: RefCell<DialogState> = const { RefCell::new(DialogState::Closed) };
}

enum DialogState {
    Closed,
    Open,
    Submitted,
    Cancelled,
}

pub(super) fn prompt_login() {
    let prefill = CTX.with_borrow(|ctx| {
        let ctx = ctx.as_ref().expect("tray ctx");
        let state = ctx.state.lock().unwrap();
        Credentials {
            url: state.url.clone().unwrap_or_default(),
            ..Default::default()
        }
    });
    if let Some(credentials) = login_dialog(prefill) {
        CTX.with_borrow(|ctx| {
            let _ = ctx
                .as_ref()
                .expect("tray ctx")
                .events
                .send(super::TrayEvent::Login(credentials));
        });
    }
}

fn login_dialog(prefill: Credentials) -> Option<Credentials> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(login_wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("fdrive_login"),
            hIcon: LoadIconW(Some(instance.into()), PCWSTR(1 as _)).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_3DFACE.0 + 1) as _),
            ..Default::default()
        };
        RegisterClassW(&class);

        let hwnd = CreateWindowExW(
            Default::default(),
            w!("fdrive_login"),
            w!("Filestash — Login"),
            WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            360,
            135,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;

        let font = GetStockObject(DEFAULT_GUI_FONT);
        let child =
            |class: PCWSTR, text: PCWSTR, style: u32, x: i32, y: i32, w: i32, h: i32, id: i32| {
                if let Ok(ctl) = CreateWindowExW(
                    Default::default(),
                    class,
                    text,
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                    x,
                    y,
                    w,
                    h,
                    Some(hwnd),
                    Some(HMENU(id as _)),
                    Some(instance.into()),
                    None,
                ) {
                    SendMessageW(
                        ctl,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
            };
        child(w!("STATIC"), w!("Server"), 0, 12, 18, 80, 20, 0);
        child(
            w!("EDIT"),
            PCWSTR::null(),
            WS_BORDER.0 | WS_TABSTOP.0 | ES_AUTOHSCROLL as u32,
            100,
            15,
            230,
            22,
            ID_SERVER,
        );
        child(
            w!("BUTTON"),
            w!("Login"),
            WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32,
            150,
            55,
            85,
            26,
            ID_OK,
        );
        child(
            w!("BUTTON"),
            w!("Cancel"),
            WS_TABSTOP.0,
            245,
            55,
            85,
            26,
            ID_CANCEL,
        );
        set_text(hwnd, ID_SERVER, &prefill.url);
        let _ = SetForegroundWindow(hwnd);
        if let Ok(first) = GetDlgItem(Some(hwnd), ID_SERVER) {
            let _ = SetFocus(Some(first));
        }

        DIALOG.with_borrow_mut(|d| *d = DialogState::Open);
        let mut msg = MSG::default();
        loop {
            let submitted = DIALOG.with_borrow(|d| match d {
                DialogState::Open => None,
                DialogState::Submitted => Some(true),
                DialogState::Cancelled | DialogState::Closed => Some(false),
            });
            if let Some(submitted) = submitted {
                let raw = get_text(hwnd, ID_SERVER);
                DIALOG.with_borrow_mut(|d| *d = DialogState::Closed);
                let _ = DestroyWindow(hwnd);
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if !submitted || raw.trim().is_empty() {
                    return None;
                }
                let url = fdrive_core::sdk::normalize_server(&raw);
                if let Err(err) = fdrive_core::sdk::Sdk::builder(&url)
                    .insecure(prefill.insecure)
                    .probe_blocking()
                {
                    alert(&format!(
                        "{url} does not look like a Filestash server.\n\n{err}"
                    ));
                    return None;
                }
                let data = CTX.with_borrow(|ctx| {
                    ctx.as_ref()
                        .expect("tray ctx")
                        .log_path
                        .parent()
                        .expect("data dir")
                        .to_path_buf()
                });
                return match crate::webview::login(&url, prefill.insecure, &data) {
                    Ok(Some(token)) => Some(Credentials {
                        url,
                        token,
                        insecure: prefill.insecure,
                        ..Default::default()
                    }),
                    Ok(None) => None,
                    Err(err) => {
                        alert(&format!(
                            "{err}\n\nInstall the WebView2 runtime, or use --token / --user from the command line."
                        ));
                        None
                    }
                };
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                return None;
            }
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

unsafe extern "system" fn login_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match (wparam.0 & 0xffff) as i32 {
                ID_OK => DIALOG.with_borrow_mut(|d| *d = DialogState::Submitted),
                ID_CANCEL => DIALOG.with_borrow_mut(|d| *d = DialogState::Cancelled),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            DIALOG.with_borrow_mut(|d| *d = DialogState::Cancelled);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

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

use super::{alert, ctx, get_text, send, set_text, state, Credentials};

const ID_SERVER: i32 = 101;
const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;

thread_local! {
    static DIALOG: RefCell<Option<bool>> = const { RefCell::new(Some(false)) };
}

pub(super) fn prompt_login() {
    let prefill = state(|state| state.url.clone().unwrap_or_default());
    let Some(raw) = server_dialog(&prefill) else {
        return;
    };
    let url = fdrive_core::sdk::normalize_server(&raw);
    if let Err(err) = fdrive_core::sdk::Sdk::builder(&url)
        .insecure(false)
        .probe_blocking()
    {
        alert(&format!(
            "{url} does not look like a Filestash server.\n\n{err}"
        ));
        return;
    }
    let data = ctx(|ctx| ctx.data.clone());
    let token = match super::webview::login(&url, false, &data) {
        Ok(Some(token)) => token,
        Ok(None) => return,
        Err(err) => {
            alert(&format!(
                "{err}\n\nInstall the WebView2 runtime, or use --token / --user from the command line."
            ));
            return;
        }
    };
    send(super::TrayEvent::Login(Credentials {
        url,
        token,
        ..Default::default()
    }));
}

fn server_dialog(prefill: &str) -> Option<String> {
    unsafe {
        let hwnd = frame()?;
        controls(hwnd);
        set_text(hwnd, ID_SERVER, prefill);
        let _ = SetForegroundWindow(hwnd);
        if let Ok(first) = GetDlgItem(Some(hwnd), ID_SERVER) {
            let _ = SetFocus(Some(first));
        }
        pump(hwnd)
    }
}

unsafe fn frame() -> Option<HWND> {
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
    CreateWindowExW(
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
    .ok()
}

unsafe fn controls(hwnd: HWND) {
    let Ok(instance) = GetModuleHandleW(None) else {
        return;
    };
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
}

unsafe fn pump(hwnd: HWND) -> Option<String> {
    DIALOG.with_borrow_mut(|d| *d = None);
    let mut msg = MSG::default();
    loop {
        if let Some(submitted) = DIALOG.with_borrow(|d| *d) {
            let raw = get_text(hwnd, ID_SERVER);
            DIALOG.with_borrow_mut(|d| *d = Some(false));
            let _ = DestroyWindow(hwnd);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            return (submitted && !raw.trim().is_empty()).then_some(raw);
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

unsafe extern "system" fn login_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match (wparam.0 & 0xffff) as i32 {
                ID_OK => DIALOG.with_borrow_mut(|d| *d = Some(true)),
                ID_CANCEL => DIALOG.with_borrow_mut(|d| *d = Some(false)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            DIALOG.with_borrow_mut(|d| *d = Some(false));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

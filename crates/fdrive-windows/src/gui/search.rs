use std::cell::RefCell;
use std::path::PathBuf;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetStockObject, RedrawWindow, COLOR_3DFACE, DEFAULT_GUI_FONT, HBRUSH, RDW_ALLCHILDREN, RDW_INVALIDATE,
    RDW_UPDATENOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetDlgItem, GetParent, LoadIconW, RegisterClassW, SendMessageW,
    SetForegroundWindow, CW_USEDEFAULT, ES_AUTOHSCROLL, HMENU, LBN_DBLCLK, LBS_NOINTEGRALHEIGHT,
    LBS_NOTIFY, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, WINDOW_STYLE, WM_CHAR, WM_COMMAND, WM_DESTROY, WM_KEYDOWN,
    WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use super::{get_text, set_text, state};
use crate::utils::wstr;

const ID_QUERY: i32 = 301;
const ID_HITS: i32 = 302;
const ID_STATUS: i32 = 303;

thread_local! {
    static HITS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn open() {
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW {
            lpfnWndProc: Some(search_wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("fdrive_search"),
            hIcon: LoadIconW(Some(instance.into()), PCWSTR(1 as _)).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_3DFACE.0 + 1) as _),
            ..Default::default()
        };
        RegisterClassW(&class);
        let Ok(hwnd) = CreateWindowExW(
            Default::default(),
            w!("fdrive_search"),
            w!("Filestash — Search"),
            WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            360,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        let font = GetStockObject(DEFAULT_GUI_FONT);
        for (class, text, style, x, y, w, h, id) in [
            (w!("EDIT"), PCWSTR::null(), WS_BORDER.0 | WS_TABSTOP.0 | ES_AUTOHSCROLL as u32, 10, 10, 444, 24, ID_QUERY),
            (w!("LISTBOX"), PCWSTR::null(), WS_BORDER.0 | WS_VSCROLL.0 | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32, 10, 44, 444, 242, ID_HITS),
            (w!("STATIC"), PCWSTR::null(), 0, 10, 292, 444, 18, ID_STATUS),
        ] {
            if let Ok(ctl) = CreateWindowExW(Default::default(), class, text, WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style), x, y, w, h, Some(hwnd), Some(HMENU(id as _)), Some(instance.into()), None) {
                SendMessageW(ctl, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
                if id == ID_QUERY {
                    let _ = SetWindowSubclass(ctl, Some(query_proc), 0, 0);
                }
            }
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

unsafe fn search(hwnd: HWND) {
    let query = get_text(hwnd, ID_QUERY);
    let Some(on_search) = state(|state| state.on_search.clone()) else { return };
    set_text(hwnd, ID_STATUS, "Searching…");
    let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN);
    let hits = if query.trim().is_empty() { Vec::new() } else { on_search(query.trim()) };
    set_text(hwnd, ID_STATUS, &format!("{} results", hits.len()));
    let Ok(list) = GetDlgItem(Some(hwnd), ID_HITS) else { return };
    SendMessageW(list, LB_RESETCONTENT, None, None);
    for hit in &hits {
        let text = wstr(hit);
        SendMessageW(list, LB_ADDSTRING, None, Some(LPARAM(text.as_ptr() as isize)));
    }
    HITS.with_borrow_mut(|stored| *stored = hits);
}

unsafe fn reveal(hwnd: HWND) {
    let Ok(list) = GetDlgItem(Some(hwnd), ID_HITS) else { return };
    let index = SendMessageW(list, LB_GETCURSEL, None, None).0;
    if let Some(path) = HITS.with_borrow(|hits| hits.get(index as usize).cloned()) {
        super::reveal(&path);
    }
}

unsafe extern "system" fn search_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND if (wparam.0 & 0xffff) as i32 == ID_HITS && (wparam.0 >> 16) as u32 == LBN_DBLCLK => {
            reveal(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            HITS.with_borrow_mut(|hits| hits.clear());
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn query_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match (msg, wparam.0) {
        (WM_KEYDOWN, key) if key == VK_RETURN.0 as usize => {
            if let Ok(window) = GetParent(hwnd) {
                search(window);
            }
            LRESULT(0)
        }
        (WM_CHAR, 0x0d) => LRESULT(0),
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

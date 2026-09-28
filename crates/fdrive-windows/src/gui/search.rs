use std::cell::RefCell;
use std::path::PathBuf;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, COLOR_3DFACE, DEFAULT_GUI_FONT, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetDlgItem, LoadIconW, RegisterClassW, SendMessageW,
    SetForegroundWindow, CW_USEDEFAULT, ES_AUTOHSCROLL, HMENU, LBN_DBLCLK, LBS_NOINTEGRALHEIGHT,
    LBS_NOTIFY, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, WINDOW_STYLE, WM_COMMAND, WM_DESTROY,
    WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use super::{get_text, state};
use crate::utils::wstr;

const ID_QUERY: i32 = 301;
const ID_GO: i32 = 302;
const ID_HITS: i32 = 303;

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
            (w!("EDIT"), PCWSTR::null(), WS_BORDER.0 | WS_TABSTOP.0 | ES_AUTOHSCROLL as u32, 10, 10, 360, 24, ID_QUERY),
            (w!("BUTTON"), w!("Search"), WS_TABSTOP.0, 380, 9, 74, 26, ID_GO),
            (w!("LISTBOX"), PCWSTR::null(), WS_BORDER.0 | WS_VSCROLL.0 | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32, 10, 44, 444, 266, ID_HITS),
        ] {
            if let Ok(ctl) = CreateWindowExW(Default::default(), class, text, WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style), x, y, w, h, Some(hwnd), Some(HMENU(id as _)), Some(instance.into()), None) {
                SendMessageW(ctl, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            }
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

unsafe fn search(hwnd: HWND) {
    let query = get_text(hwnd, ID_QUERY);
    let Some(on_search) = state(|state| state.on_search.clone()) else { return };
    let hits = if query.trim().is_empty() { Vec::new() } else { on_search(query.trim()) };
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
        let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn();
    }
}

unsafe extern "system" fn search_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match ((wparam.0 & 0xffff) as i32, (wparam.0 >> 16) as u32) {
                (ID_GO, _) => search(hwnd),
                (ID_HITS, LBN_DBLCLK) => reveal(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            HITS.with_borrow_mut(|hits| hits.clear());
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

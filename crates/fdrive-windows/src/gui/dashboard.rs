use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use fdrive_core::activity::Activity;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, GetStockObject, GetSysColor, GetSysColorBrush,
    ScreenToClient, SetBkColor, SetTextColor, COLOR_GRAYTEXT, COLOR_WINDOW, DEFAULT_GUI_FONT,
    HBRUSH, HDC, HGDIOBJ, LOGFONTW,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, CCS_BOTTOM, CCS_NOPARENTALIGN, CCS_NORESIZE, ICC_BAR_CLASSES,
    ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVCFMT_LEFT, LVCF_FMT, LVCF_TEXT, LVCF_WIDTH,
    LVCOLUMNW, LVHITTESTINFO, LVIF_TEXT, LVITEMW, LVM_DELETEITEM, LVM_GETITEMCOUNT,
    LVM_HITTEST, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETCOLUMNWIDTH,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMTEXTW, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT,
    NMHDR, NMITEMACTIVATE, NM_DBLCLK,
    LVS_EX_LABELTIP, LVS_NOCOLUMNHEADER, LVS_REPORT, LVS_SHOWSELALWAYS,
    LVS_SINGLESEL, SB_SETPARTS, SB_SETTEXTW, STATUSCLASSNAMEW, WC_LISTVIEWW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetClientRect, GetCursorPos, GetDlgItem, GetWindowRect, KillTimer, LoadIconW, MoveWindow,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetTimer, ShowWindow, SystemParametersInfoW,
    TrackPopupMenu, HMENU, MF_STRING, SPI_GETWORKAREA, SW_HIDE, SW_SHOW,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, TPM_NONOTIFY, TPM_RETURNCMD, WINDOW_STYLE, WM_CLOSE,
    WM_CONTEXTMENU, WM_CTLCOLORSTATIC, WM_DESTROY, WM_GETFONT, WM_NOTIFY, WM_SETFONT, WM_SIZE,
    WM_TIMER,
    WNDCLASSW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_THICKFRAME, WS_VISIBLE,
};

use crate::utils::wstr;

const ID_STATS_STATUS: i32 = 201;
const ID_STATS_LIST: i32 = 202;
const ID_STATS_EMPTY_ICON: i32 = 203;
const ID_STATS_EMPTY_TEXT: i32 = 204;
const SS_CENTER: u32 = 0x1;
const CMD_COPY: usize = 1;
const CMD_CLEAR: usize = 2;
const CMD_REVEAL: usize = 3;
const STATS_TIMER: usize = 1;
const STATS_W: i32 = 350;
const STATS_H: i32 = 400;

struct Stats {
    hwnd: HWND,
    shown: u64,
    cleared: u64,
    activity: Arc<Activity>,
    root: PathBuf,
}

thread_local! {
    static STATS: RefCell<Option<Stats>> = const { RefCell::new(None) };
}

pub fn toggle(activity: Arc<Activity>, root: PathBuf) {
    match STATS.with_borrow(|stats| stats.as_ref().map(|stats| stats.hwnd)) {
        Some(hwnd) => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        None => unsafe { open(activity, root) },
    }
}

pub(crate) fn close() -> bool {
    match STATS.with_borrow(|stats| stats.as_ref().map(|stats| stats.hwnd)) {
        Some(hwnd) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            true
        }
        None => false,
    }
}

unsafe fn open(activity: Arc<Activity>, root: PathBuf) {
    let Some(hwnd) = frame() else { return };
    let font = GetStockObject(DEFAULT_GUI_FONT);
    if list(hwnd, font).is_none() || status_bar(hwnd, font).is_none() || empty_state(hwnd, font).is_none() {
        let _ = DestroyWindow(hwnd);
        return;
    }
    STATS.with_borrow_mut(|stats| {
        *stats = Some(Stats {
            hwnd,
            shown: u64::MAX,
            cleared: 0,
            activity,
            root,
        })
    });
    layout(hwnd);
    refresh_stats(hwnd);
    SetTimer(Some(hwnd), STATS_TIMER, 300, None);
    let _ = SetForegroundWindow(hwnd);
}

unsafe fn frame() -> Option<HWND> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES,
    };
    if !InitCommonControlsEx(&controls).as_bool() {
        return None;
    }
    let instance = GetModuleHandleW(None).ok()?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(stats_wndproc),
        hInstance: instance.into(),
        lpszClassName: w!("fdrive_stats"),
        hIcon: LoadIconW(Some(instance.into()), PCWSTR(1 as _)).unwrap_or_default(),
        hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as _),
        ..Default::default()
    };
    RegisterClassW(&class);
    let (x, y) = flyout_position(STATS_W, STATS_H);
    CreateWindowExW(
        Default::default(),
        w!("fdrive_stats"),
        w!("Filestash — Transfers"),
        WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_VISIBLE,
        x,
        y,
        STATS_W,
        STATS_H,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .ok()
}

unsafe fn list(hwnd: HWND, font: HGDIOBJ) -> Option<HWND> {
    let style = WS_CHILD
        | WS_VISIBLE
        | WS_TABSTOP
        | WINDOW_STYLE(LVS_REPORT | LVS_SHOWSELALWAYS | LVS_SINGLESEL | LVS_NOCOLUMNHEADER);
    let list = CreateWindowExW(
        Default::default(),
        WC_LISTVIEWW,
        PCWSTR::null(),
        style,
        0,
        0,
        0,
        0,
        Some(hwnd),
        Some(HMENU(ID_STATS_LIST as _)),
        Some(GetModuleHandleW(None).ok()?.into()),
        None,
    )
    .ok()?;
    set_font(list, font);
    let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP;
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(ex as usize)),
        Some(LPARAM(ex as isize)),
    );
    insert_column(list, 0, "Name", 300, LVCFMT_LEFT);
    Some(list)
}

unsafe fn status_bar(hwnd: HWND, font: HGDIOBJ) -> Option<HWND> {
    let status = CreateWindowExW(
        Default::default(),
        STATUSCLASSNAMEW,
        PCWSTR::null(),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE((CCS_BOTTOM | CCS_NORESIZE | CCS_NOPARENTALIGN) as u32),
        0,
        0,
        0,
        0,
        Some(hwnd),
        Some(HMENU(ID_STATS_STATUS as _)),
        Some(GetModuleHandleW(None).ok()?.into()),
        None,
    )
    .ok()?;
    set_font(status, font);
    Some(status)
}

unsafe fn empty_state(hwnd: HWND, font: HGDIOBJ) -> Option<()> {
    let instance = GetModuleHandleW(None).ok()?;
    let icon = CreateWindowExW(
        Default::default(),
        w!("STATIC"),
        w!("☁"),
        WS_CHILD | WINDOW_STYLE(SS_CENTER),
        0,
        0,
        0,
        0,
        Some(hwnd),
        Some(HMENU(ID_STATS_EMPTY_ICON as _)),
        Some(instance.into()),
        None,
    )
    .ok()?;
    let mut logfont = LOGFONTW {
        lfHeight: -28,
        ..Default::default()
    };
    for (dst, src) in logfont.lfFaceName.iter_mut().zip("Segoe UI Symbol".encode_utf16()) {
        *dst = src;
    }
    set_font(icon, CreateFontIndirectW(&logfont).into());
    let caption = CreateWindowExW(
        Default::default(),
        w!("STATIC"),
        w!("No Transfer"),
        WS_CHILD | WINDOW_STYLE(SS_CENTER),
        0,
        0,
        0,
        0,
        Some(hwnd),
        Some(HMENU(ID_STATS_EMPTY_TEXT as _)),
        Some(instance.into()),
        None,
    )
    .ok()?;
    set_font(caption, font);
    Some(())
}

unsafe fn set_font(hwnd: HWND, font: HGDIOBJ) {
    SendMessageW(
        hwnd,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
}

unsafe fn insert_column(
    list: HWND,
    index: usize,
    title: &str,
    width: i32,
    format: windows::Win32::UI::Controls::LVCOLUMNW_FORMAT,
) {
    let mut title = wstr(title);
    let column = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
        fmt: format,
        cx: width,
        pszText: PWSTR(title.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_INSERTCOLUMNW,
        Some(WPARAM(index)),
        Some(LPARAM((&column as *const LVCOLUMNW) as isize)),
    );
}

unsafe extern "system" fn stats_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            refresh_stats(hwnd);
            LRESULT(0)
        }
        WM_SIZE => {
            layout(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_NOTIFY => {
            let hdr = &*(lparam.0 as *const NMHDR);
            if hdr.idFrom == ID_STATS_LIST as usize && hdr.code == NM_DBLCLK {
                let item = (*(lparam.0 as *const NMITEMACTIVATE)).iItem;
                if item >= 0 {
                    reveal(item as usize);
                }
            }
            LRESULT(0)
        }
        WM_CONTEXTMENU => {
            context_menu(hwnd);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as _);
            SetTextColor(hdc, COLORREF(GetSysColor(COLOR_GRAYTEXT)));
            SetBkColor(hdc, COLORREF(GetSysColor(COLOR_WINDOW)));
            LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
        }
        WM_DESTROY => {
            if let Ok(icon) = GetDlgItem(Some(hwnd), ID_STATS_EMPTY_ICON) {
                let font = SendMessageW(icon, WM_GETFONT, None, None);
                let _ = DeleteObject(HGDIOBJ(font.0 as _));
            }
            let _ = KillTimer(Some(hwnd), STATS_TIMER);
            STATS.with_borrow_mut(|stats| *stats = None);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn layout(hwnd: HWND) {
    let mut client = RECT::default();
    if GetClientRect(hwnd, &mut client).is_err() {
        return;
    }
    let width = (client.right - client.left).max(1);
    let height = (client.bottom - client.top).max(1);
    let mut status_height = 22;
    if let Ok(status) = GetDlgItem(Some(hwnd), ID_STATS_STATUS) {
        SendMessageW(status, WM_SIZE, Some(WPARAM(0)), Some(LPARAM(0)));
        let mut status_rect = RECT::default();
        if GetWindowRect(status, &mut status_rect).is_ok() {
            status_height = (status_rect.bottom - status_rect.top).max(status_height);
        }
        let parts = [-1];
        SendMessageW(
            status,
            SB_SETPARTS,
            Some(WPARAM(parts.len())),
            Some(LPARAM(parts.as_ptr() as isize)),
        );
        let _ = MoveWindow(status, 0, height - status_height, width, status_height, true);
    }
    let body = (height - status_height).max(1);
    if let Ok(list) = GetDlgItem(Some(hwnd), ID_STATS_LIST) {
        let _ = MoveWindow(list, 0, 0, width, body, true);
        fit_columns(list);
    }
    for (id, top, h) in [
        (ID_STATS_EMPTY_ICON, body / 2 - 32, 32),
        (ID_STATS_EMPTY_TEXT, body / 2 + 2, 18),
    ] {
        if let Ok(ctl) = GetDlgItem(Some(hwnd), id) {
            let _ = MoveWindow(ctl, 0, top, width, h, true);
        }
    }
}

unsafe fn fit_columns(list: HWND) {
    let mut client = RECT::default();
    if GetClientRect(list, &mut client).is_err() {
        return;
    }
    set_column_width(list, 0, (client.right - client.left).max(80));
}

unsafe fn set_column_width(list: HWND, column: usize, width: i32) {
    SendMessageW(
        list,
        LVM_SETCOLUMNWIDTH,
        Some(WPARAM(column)),
        Some(LPARAM(width as isize)),
    );
}

unsafe fn context_menu(hwnd: HWND) {
    let Ok(list) = GetDlgItem(Some(hwnd), ID_STATS_LIST) else { return };
    let mut cursor = POINT::default();
    let _ = GetCursorPos(&mut cursor);
    let mut hit = LVHITTESTINFO {
        pt: cursor,
        ..Default::default()
    };
    let _ = ScreenToClient(list, &mut hit.pt);
    let item = SendMessageW(
        list,
        LVM_HITTEST,
        None,
        Some(LPARAM((&mut hit as *mut LVHITTESTINFO) as isize)),
    )
    .0;
    let Ok(menu) = CreatePopupMenu() else { return };
    if item >= 0 {
        let _ = AppendMenuW(menu, MF_STRING, CMD_REVEAL, w!("Open"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_COPY, w!("Copy"));
    }
    let _ = AppendMenuW(menu, MF_STRING, CMD_CLEAR, w!("Clear"));
    let picked = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        cursor.x,
        cursor.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    match picked.0 as usize {
        CMD_REVEAL => reveal(item as usize),
        CMD_COPY => copy_path(hwnd, item as usize),
        CMD_CLEAR => clear_transfers(hwnd),
        _ => {}
    }
}

fn transfer_at(index: usize) -> Option<(fdrive_core::activity::Transfer, PathBuf)> {
    let state = STATS.with_borrow(|stats| {
        stats.as_ref().map(|s| (s.cleared, s.activity.clone(), s.root.clone()))
    });
    let (cleared, activity, root) = state?;
    let snap = activity.snapshot();
    let transfer = snap.transfers.iter().filter(|t| t.id > cleared).nth(index)?.clone();
    Some((transfer, root))
}

fn reveal(index: usize) {
    let Some((transfer, root)) = transfer_at(index) else { return };
    let local = root.join(transfer.path.trim_start_matches('/').replace('/', "\\"));
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", local.display()))
        .spawn();
}

unsafe fn copy_path(hwnd: HWND, index: usize) {
    let Some((transfer, _)) = transfer_at(index) else { return };
    let wide = wstr(&transfer.path);
    if OpenClipboard(Some(hwnd)).is_err() {
        return;
    }
    let _ = EmptyClipboard();
    if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
        let dst = GlobalLock(mem);
        if !dst.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst as *mut u16, wide.len());
            let _ = GlobalUnlock(mem);
            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0)));
        }
    }
    let _ = CloseClipboard();
}

fn clear_transfers(hwnd: HWND) {
    STATS.with_borrow_mut(|stats| {
        if let Some(stats) = stats {
            let snap = stats.activity.snapshot();
            stats.cleared = snap.transfers.iter().map(|transfer| transfer.id).max().unwrap_or(stats.cleared);
            stats.shown = u64::MAX;
        }
    });
    refresh_stats(hwnd);
}

fn refresh_stats(hwnd: HWND) {
    let state = STATS
        .with_borrow(|stats| stats.as_ref().map(|s| (s.cleared, s.activity.clone())));
    let Some((cleared, activity)) = state else { return };
    let snap = activity.snapshot();
    set_rates(hwnd, &snap);
    let stale = STATS.with_borrow_mut(|stats| match stats {
        Some(stats) if stats.shown != snap.version => {
            stats.shown = snap.version;
            true
        }
        _ => false,
    });
    if stale {
        unsafe {
            render_transfers(hwnd, &snap, cleared);
        }
    }
}

fn set_rates(hwnd: HWND, snap: &fdrive_core::activity::Snapshot) {
    let Ok(status) = (unsafe { GetDlgItem(Some(hwnd), ID_STATS_STATUS) }) else {
        return;
    };
    let (up, down) = fdrive_core::activity::mean_rate(snap);
    let down = format!("➘{}/s", fdrive_core::activity::fmt_compact(down));
    let up = format!("➚{}/s", fdrive_core::activity::fmt_compact(up));
    let traffic = fdrive_core::activity::sparkline(snap, 24);
    unsafe {
        set_status_text(status, 0, &format!("{traffic}\t\t{down:>10}  {up:>10} "));
    }
}

unsafe fn set_status_text(status: HWND, part: usize, text: &str) {
    let wide = wstr(text);
    SendMessageW(
        status,
        SB_SETTEXTW,
        Some(WPARAM(part)),
        Some(LPARAM(wide.as_ptr() as isize)),
    );
}

unsafe fn render_transfers(hwnd: HWND, snap: &fdrive_core::activity::Snapshot, cleared: u64) {
    let Ok(list) = GetDlgItem(Some(hwnd), ID_STATS_LIST) else {
        return;
    };
    let transfers: Vec<_> = snap.transfers.iter().filter(|t| t.id > cleared).collect();
    let empty = transfers.is_empty();
    for id in [ID_STATS_EMPTY_ICON, ID_STATS_EMPTY_TEXT] {
        if let Ok(ctl) = GetDlgItem(Some(hwnd), id) {
            let _ = ShowWindow(ctl, if empty { SW_SHOW } else { SW_HIDE });
        }
    }
    let _ = ShowWindow(list, if empty { SW_HIDE } else { SW_SHOW });
    let rows: Vec<String> = transfers.iter().map(|transfer| transfer_line(transfer)).collect();
    let shown = SendMessageW(list, LVM_GETITEMCOUNT, None, None).0 as usize;
    for (index, row) in rows.iter().enumerate() {
        match index < shown {
            true => set_row(list, index, row),
            false => insert_row(list, index, row),
        }
    }
    for index in (rows.len()..shown).rev() {
        SendMessageW(list, LVM_DELETEITEM, Some(WPARAM(index)), None);
    }
    fit_columns(list);
}

unsafe fn insert_row(list: HWND, index: usize, text: &str) {
    let mut text = wstr(text);
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: index as i32,
        pszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_INSERTITEMW,
        Some(WPARAM(0)),
        Some(LPARAM((&item as *const LVITEMW) as isize)),
    );
}

unsafe fn set_row(list: HWND, index: usize, text: &str) {
    let mut text = wstr(text);
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: index as i32,
        iSubItem: 0,
        pszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMTEXTW,
        Some(WPARAM(index)),
        Some(LPARAM((&item as *const LVITEMW) as isize)),
    );
}

fn transfer_line(transfer: &fdrive_core::activity::Transfer) -> String {
    use fdrive_core::activity::{fmt_compact, Direction, Mode, Outcome};
    let name = transfer.path.trim_start_matches('/');
    if let Outcome::Failed(err) = &transfer.outcome {
        return format!("✗ {name} ({err})");
    }
    let icon = match transfer.direction {
        Direction::Down => "➘",
        Direction::Up => "➚",
    };
    let detail = match (transfer.mode, &transfer.outcome) {
        (Mode::Delta, _) => format!(
            "Δ{} / {}",
            fmt_compact(transfer.wire),
            fmt_compact(transfer.size)
        ),
        (Mode::Full, Outcome::Running) if transfer.progress > 0 => format!(
            "{} / {}",
            fmt_compact(transfer.progress),
            fmt_compact(transfer.size)
        ),
        _ => fmt_compact(transfer.size),
    };
    format!("{icon} {name} ({detail})")
}

fn flyout_position(w: i32, h: i32) -> (i32, i32) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let mut area = RECT {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 1024,
        };
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut area as *mut RECT as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let x = (cursor.x - w / 2).clamp(area.left + 8, (area.right - w - 8).max(area.left + 8));
        let y = if cursor.y > (area.top + area.bottom) / 2 {
            (area.bottom - h - 8).max(area.top + 8)
        } else {
            area.top + 8
        };
        (x, y)
    }
}

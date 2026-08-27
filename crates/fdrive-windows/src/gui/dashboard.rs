use std::cell::RefCell;
use std::sync::Arc;

use fdrive_core::activity::Activity;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, COLOR_WINDOW, DEFAULT_GUI_FONT, HBRUSH, HGDIOBJ};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, CCS_BOTTOM, CCS_NOPARENTALIGN, CCS_NORESIZE, ICC_BAR_CLASSES,
    ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVCFMT_LEFT, LVCF_FMT, LVCF_TEXT, LVCF_WIDTH,
    LVCOLUMNW, LVIF_TEXT, LVITEMW, LVM_DELETEITEM, LVM_GETCOLUMNWIDTH, LVM_GETITEMCOUNT,
    LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMTEXTW, LVSCW_AUTOSIZE_USEHEADER, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT,
    LVS_EX_GRIDLINES, LVS_EX_LABELTIP, LVS_NOCOLUMNHEADER, LVS_REPORT, LVS_SHOWSELALWAYS,
    LVS_SINGLESEL, SB_SETPARTS, SB_SETTEXTW, STATUSCLASSNAMEW, WC_LISTVIEWW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, GetDlgItem,
    GetWindowRect, KillTimer, LoadIconW, MoveWindow, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetTimer, SystemParametersInfoW, HMENU, SPI_GETWORKAREA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_SETFONT, WM_SIZE,
    WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_THICKFRAME, WS_VISIBLE,
};

use crate::utils::wstr;

const ID_STATS_STATUS: i32 = 201;
const ID_STATS_LIST: i32 = 202;
const STATS_TIMER: usize = 1;
const STATS_W: i32 = 350;
const STATS_H: i32 = 400;

thread_local! {
    static STATS: RefCell<Option<(HWND, u64, Arc<Activity>)>> = const { RefCell::new(None) };
}

pub fn toggle(activity: Arc<Activity>) {
    match STATS.with_borrow(|stats| stats.as_ref().map(|(hwnd, ..)| *hwnd)) {
        Some(hwnd) => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        None => unsafe { open(activity) },
    }
}

unsafe fn open(activity: Arc<Activity>) {
    let Some(hwnd) = frame() else { return };
    let font = GetStockObject(DEFAULT_GUI_FONT);
    if list(hwnd, font).is_none() || status_bar(hwnd, font).is_none() {
        let _ = DestroyWindow(hwnd);
        return;
    }
    STATS.with_borrow_mut(|stats| *stats = Some((hwnd, u64::MAX, activity)));
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
    let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP | LVS_EX_GRIDLINES;
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(ex as usize)),
        Some(LPARAM(ex as isize)),
    );
    insert_column(list, 0, "Name", 300, LVCFMT_LEFT);
    insert_column(list, 1, "Status", 200, LVCFMT_LEFT);
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
        WM_DESTROY => {
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
        let parts = [width * 45 / 100, width * 73 / 100, -1];
        SendMessageW(
            status,
            SB_SETPARTS,
            Some(WPARAM(parts.len())),
            Some(LPARAM(parts.as_ptr() as isize)),
        );
        let _ = MoveWindow(status, 0, height - status_height, width, status_height, true);
    }
    if let Ok(list) = GetDlgItem(Some(hwnd), ID_STATS_LIST) {
        let _ = MoveWindow(list, 0, 0, width, (height - status_height).max(1), true);
        fit_columns(list);
    }
}

unsafe fn fit_columns(list: HWND) {
    let mut client = RECT::default();
    if GetClientRect(list, &mut client).is_err() {
        return;
    }
    set_column_width(list, 1, LVSCW_AUTOSIZE_USEHEADER);
    let status_width = SendMessageW(list, LVM_GETCOLUMNWIDTH, Some(WPARAM(1)), None).0 as i32;
    let name_width = (client.right - client.left - status_width).max(80);
    set_column_width(list, 0, name_width);
}

unsafe fn set_column_width(list: HWND, column: usize, width: i32) {
    SendMessageW(
        list,
        LVM_SETCOLUMNWIDTH,
        Some(WPARAM(column)),
        Some(LPARAM(width as isize)),
    );
}

fn refresh_stats(hwnd: HWND) {
    let activity = STATS.with_borrow(|stats| stats.as_ref().map(|(.., activity)| activity.clone()));
    let Some(activity) = activity else { return };
    let snap = activity.snapshot();
    set_rates(hwnd, &snap);
    let stale = STATS.with_borrow_mut(|stats| match stats {
        Some((_, shown, _)) if *shown != snap.version => {
            *shown = snap.version;
            true
        }
        _ => false,
    });
    if stale {
        unsafe {
            render_transfers(hwnd, &snap);
        }
    }
}

fn set_rates(hwnd: HWND, snap: &fdrive_core::activity::Snapshot) {
    let Ok(status) = (unsafe { GetDlgItem(Some(hwnd), ID_STATS_STATUS) }) else {
        return;
    };
    let (up, down) = fdrive_core::activity::mean_rate(snap);
    let down = format!("  ↓  {}/s", fdrive_core::activity::fmt_compact(down));
    let up = format!("  ↑  {}/s", fdrive_core::activity::fmt_compact(up));
    let traffic = fdrive_core::activity::sparkline(snap, 24);
    unsafe {
        set_status_text(status, 0, &traffic);
        set_status_text(status, 1, &down);
        set_status_text(status, 2, &up);
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

unsafe fn render_transfers(hwnd: HWND, snap: &fdrive_core::activity::Snapshot) {
    let Ok(list) = GetDlgItem(Some(hwnd), ID_STATS_LIST) else {
        return;
    };
    let rows: Vec<[String; 2]> = match snap.transfers.is_empty() {
        true => vec![["No recent transfers".into(), String::new()]],
        false => snap
            .transfers
            .iter()
            .map(|transfer| {
                [
                    transfer.path.trim_start_matches('/').to_string(),
                    transfer_detail(transfer),
                ]
            })
            .collect(),
    };
    let shown = SendMessageW(list, LVM_GETITEMCOUNT, None, None).0 as usize;
    for (index, row) in rows.iter().enumerate() {
        let columns = [row[0].as_str(), row[1].as_str()];
        match index < shown {
            true => set_row(list, index, columns),
            false => insert_row(list, index, columns),
        }
    }
    for index in (rows.len()..shown).rev() {
        SendMessageW(list, LVM_DELETEITEM, Some(WPARAM(index)), None);
    }
    fit_columns(list);
}

unsafe fn insert_row(list: HWND, index: usize, columns: [&str; 2]) {
    let mut blank = wstr("");
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: index as i32,
        pszText: PWSTR(blank.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_INSERTITEMW,
        Some(WPARAM(0)),
        Some(LPARAM((&item as *const LVITEMW) as isize)),
    );
    set_row(list, index, columns);
}

unsafe fn set_row(list: HWND, index: usize, columns: [&str; 2]) {
    for (subitem, text) in columns.iter().enumerate() {
        let mut text = wstr(text);
        let item = LVITEMW {
            mask: LVIF_TEXT,
            iItem: index as i32,
            iSubItem: subitem as i32,
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
}

fn transfer_detail(transfer: &fdrive_core::activity::Transfer) -> String {
    use fdrive_core::activity::{fmt_compact, Direction, Mode, Outcome};
    if let Outcome::Failed(err) = &transfer.outcome {
        return format!("Failed · {err}");
    }
    let status = match (&transfer.outcome, transfer.direction) {
        (Outcome::Running, Direction::Down) => "Downloading",
        (Outcome::Running, Direction::Up) => "Uploading",
        (_, Direction::Down) => "Downloaded",
        (_, Direction::Up) => "Uploaded",
    };
    let bytes = match (transfer.mode, &transfer.outcome) {
        (Mode::Delta, _) => format!(
            "Δ{} of {}",
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
    format!("{status} · {bytes}")
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

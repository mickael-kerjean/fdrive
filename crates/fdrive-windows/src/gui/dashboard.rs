use std::cell::RefCell;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, COLOR_WINDOW, DEFAULT_GUI_FONT, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVCFMT_LEFT,
    LVCFMT_RIGHT, LVCF_FMT, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW,
    LVM_DELETEALLITEMS, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETCOLUMNWIDTH,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMTEXTW, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT,
    LVS_EX_LABELTIP, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, SBARS_SIZEGRIP, SB_SETPARTS,
    SB_SETTEXTW, STATUSCLASSNAMEW, WC_LISTVIEWW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, GetDlgItem,
    GetWindowRect, KillTimer, LoadIconW, MoveWindow, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetTimer, SystemParametersInfoW, HMENU, SPI_GETWORKAREA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_SETFONT, WM_SIZE,
    WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_SYSMENU, WS_TABSTOP,
    WS_THICKFRAME, WS_VISIBLE,
};

use super::CTX;

const ID_STATS_STATUS: i32 = 201;
const ID_STATS_LIST: i32 = 202;
const STATS_TIMER: usize = 1;
const STATS_W: i32 = 560;
const STATS_H: i32 = 430;
const MARGIN: i32 = 8;

thread_local! {
    static STATS: RefCell<Option<(HWND, u64)>> = const { RefCell::new(None) };
}

pub(super) fn show_stats() -> bool {
    let attached = CTX.with_borrow(|ctx| {
        let ctx = ctx.as_ref().expect("tray ctx");
        let state = ctx.state.lock().unwrap();
        state.activity.is_some()
    });
    if !attached {
        return false;
    }
    if let Some((hwnd, _)) = STATS.with_borrow(|stats| *stats) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return true;
    }
    unsafe {
        let controls = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES,
        };
        if !InitCommonControlsEx(&controls).as_bool() {
            return false;
        }
        let Ok(instance) = GetModuleHandleW(None) else {
            return false;
        };
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
        let Ok(hwnd) = CreateWindowExW(
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
        ) else {
            return false;
        };
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let list_style = WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WINDOW_STYLE(LVS_REPORT | LVS_SHOWSELALWAYS | LVS_SINGLESEL);
        let Ok(list) = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            WC_LISTVIEWW,
            PCWSTR::null(),
            list_style,
            0,
            0,
            0,
            0,
            Some(hwnd),
            Some(HMENU(ID_STATS_LIST as _)),
            Some(instance.into()),
            None,
        ) else {
            let _ = DestroyWindow(hwnd);
            return false;
        };
        SendMessageW(
            list,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        let list_ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP;
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(list_ex as usize)),
            Some(LPARAM(list_ex as isize)),
        );
        insert_column(list, 0, "Name", 300, LVCFMT_LEFT);
        insert_column(list, 1, "Status", 130, LVCFMT_LEFT);
        insert_column(list, 2, "Size", 90, LVCFMT_RIGHT);
        let Ok(status) = CreateWindowExW(
            Default::default(),
            STATUSCLASSNAMEW,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SBARS_SIZEGRIP),
            0,
            0,
            0,
            0,
            Some(hwnd),
            Some(HMENU(ID_STATS_STATUS as _)),
            Some(instance.into()),
            None,
        ) else {
            let _ = DestroyWindow(hwnd);
            return false;
        };
        SendMessageW(
            status,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );

        STATS.with_borrow_mut(|stats| *stats = Some((hwnd, u64::MAX)));
        layout(hwnd);
        refresh_stats(hwnd);
        SetTimer(Some(hwnd), STATS_TIMER, 300, None);
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

unsafe fn insert_column(
    list: HWND,
    index: usize,
    title: &str,
    width: i32,
    format: windows::Win32::UI::Controls::LVCOLUMNW_FORMAT,
) {
    let mut title: Vec<u16> = title.encode_utf16().chain([0]).collect();
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
    }
    if let Ok(list) = GetDlgItem(Some(hwnd), ID_STATS_LIST) {
        let list_width = (width - MARGIN * 2).max(1);
        let _ = MoveWindow(
            list,
            MARGIN,
            MARGIN,
            list_width,
            (height - status_height - MARGIN * 2).max(1),
            true,
        );
        let activity_width = 120;
        let size_width = 90;
        let name_width = (list_width - activity_width - size_width - 6).max(120);
        set_column_width(list, 0, name_width);
        set_column_width(list, 1, activity_width);
        set_column_width(list, 2, size_width);
    }
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
    let activity = CTX.with_borrow(|ctx| {
        let ctx = ctx.as_ref().expect("tray ctx");
        let state = ctx.state.lock().unwrap();
        state.activity.clone()
    });
    let Some(activity) = activity else { return };
    let snap = activity.snapshot();
    set_rates(hwnd, &snap);
    let stale = STATS.with_borrow_mut(|stats| match stats {
        Some((_, shown)) if *shown != snap.version => {
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
    let n = 5.min(snap.meter.len()).max(1);
    let (up, down) = snap.meter[snap.meter.len() - n..]
        .iter()
        .fold((0, 0), |(up, down), (bucket_up, bucket_down)| {
            (up + bucket_up, down + bucket_down)
        });
    let down = format!(
        "  Download  {}/s",
        fdrive_core::activity::fmt_compact(down / n as u64)
    );
    let up = format!(
        "  Upload  {}/s",
        fdrive_core::activity::fmt_compact(up / n as u64)
    );
    let traffic = format!("  Traffic  {}", fdrive_core::activity::sparkline(snap, 24));
    unsafe {
        set_status_text(status, 0, &traffic);
        set_status_text(status, 1, &down);
        set_status_text(status, 2, &up);
    }
}

unsafe fn set_status_text(status: HWND, part: usize, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
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
    SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    if snap.transfers.is_empty() {
        insert_row(list, 0, ["No recent transfers", "Idle", ""]);
        return;
    }
    for (index, transfer) in snap.transfers.iter().enumerate() {
        let (activity, size) = transfer_detail(transfer);
        insert_row(
            list,
            index,
            [transfer.path.trim_start_matches('/'), &activity, &size],
        );
    }
}

unsafe fn insert_row(list: HWND, index: usize, columns: [&str; 3]) {
    let mut name: Vec<u16> = columns[0].encode_utf16().chain([0]).collect();
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: index as i32,
        pszText: PWSTR(name.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_INSERTITEMW,
        Some(WPARAM(0)),
        Some(LPARAM((&item as *const LVITEMW) as isize)),
    );
    for (subitem, text) in columns.iter().enumerate().skip(1) {
        let mut text: Vec<u16> = text.encode_utf16().chain([0]).collect();
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

fn transfer_detail(transfer: &fdrive_core::activity::Transfer) -> (String, String) {
    use fdrive_core::activity::{fmt_compact, Direction, Mode, Outcome};
    let verb = match (&transfer.outcome, transfer.direction) {
        (Outcome::Running, Direction::Down) => "Downloading",
        (Outcome::Running, Direction::Up) => "Uploading",
        (Outcome::Done, Direction::Down) => "Downloaded",
        (Outcome::Done, Direction::Up) => "Uploaded",
        (Outcome::Failed(_), _) => "Failed",
    };
    let activity = if transfer.mode == Mode::Delta {
        format!("{verb} (delta)")
    } else {
        verb.to_string()
    };
    let size = match transfer.outcome {
        Outcome::Running if transfer.progress > 0 => format!(
            "{} / {}",
            fmt_compact(transfer.progress),
            fmt_compact(transfer.size)
        ),
        _ => fmt_compact(transfer.size),
    };
    (activity, size)
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

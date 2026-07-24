use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, DEFAULT_CHARSET, DEFAULT_QUALITY, FIXED_PITCH,
    FW_NORMAL, HBRUSH, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, KillTimer, LoadIconW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetTimer, SystemParametersInfoW, HMENU,
    SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_CLOSE, WM_DESTROY, WM_SETFONT,
    WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_VISIBLE,
};

use super::{set_text, CTX};

const ID_STATS_HEADER: i32 = 201;
const ID_STATS_LIST: i32 = 202;
const STATS_TIMER: usize = 1;
const STATS_COLS: usize = 46;
const STATS_LINES: usize = 20;
const STATS_W: i32 = 380;
const STATS_H: i32 = 420;

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
            w!("Filestash"),
            WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
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
        let font = CreateFontW(
            -14,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            FIXED_PITCH.0 as u32,
            w!("Consolas"),
        );
        let child = |text: PCWSTR, x: i32, y: i32, w: i32, h: i32, id: i32| {
            if let Ok(ctl) = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                text,
                WS_CHILD | WS_VISIBLE,
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
        child(PCWSTR::null(), 10, 10, STATS_W - 28, 20, ID_STATS_HEADER);
        child(PCWSTR::null(), 10, 38, STATS_W - 28, STATS_H - 80, ID_STATS_LIST);
        SetTimer(Some(hwnd), STATS_TIMER, 300, None);
        STATS.with_borrow_mut(|stats| *stats = Some((hwnd, 0)));
        refresh_stats(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
    true
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

fn refresh_stats(hwnd: HWND) {
    let activity = CTX.with_borrow(|ctx| {
        let ctx = ctx.as_ref().expect("tray ctx");
        let state = ctx.state.lock().unwrap();
        state.activity.clone()
    });
    let Some(activity) = activity else { return };
    let snap = activity.snapshot();
    set_text(hwnd, ID_STATS_HEADER, &stats_header(&snap));
    let stale = STATS.with_borrow_mut(|stats| match stats {
        Some((_, shown)) if *shown != snap.version => {
            *shown = snap.version;
            true
        }
        _ => false,
    });
    if stale {
        set_text(hwnd, ID_STATS_LIST, &stats_list(&snap));
    }
}

fn stats_header(snap: &fdrive_core::activity::Snapshot) -> String {
    let spark = fdrive_core::activity::sparkline(snap, 24);
    let rate = fdrive_core::activity::rate_line(snap);
    let pad = STATS_COLS.saturating_sub(24 + rate.chars().count());
    format!("{spark}{}{rate}", " ".repeat(pad))
}

fn stats_list(snap: &fdrive_core::activity::Snapshot) -> String {
    use fdrive_core::activity::{fmt_compact, Direction, Mode, Outcome};
    if snap.transfers.is_empty() {
        return format!("\r\n\r\n\r\n{:^width$}", "⊘", width = STATS_COLS);
    }
    snap.transfers
        .iter()
        .take(STATS_LINES)
        .map(|t| {
            let arrow = match t.direction {
                Direction::Down => '↓',
                Direction::Up => '↑',
            };
            let extra = match &t.outcome {
                Outcome::Done if t.mode == Mode::Delta => format!(" (⇄{})", fmt_compact(t.wire)),
                _ => String::new(),
            };
            let detail = match &t.outcome {
                Outcome::Running => String::new(),
                Outcome::Failed(_) => "✕".to_string(),
                Outcome::Done => fmt_compact(t.size),
            };
            let avail = STATS_COLS
                .saturating_sub(3 + extra.chars().count() + detail.chars().count() + 1);
            let name = truncate_middle(t.path.trim_start_matches('/'), avail);
            let left = format!("{arrow} {name}{extra}");
            let pad = STATS_COLS.saturating_sub(left.chars().count() + detail.chars().count());
            format!("{left}{}{detail}", " ".repeat(pad))
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn truncate_middle(name: &str, max: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max {
        return name.to_string();
    }
    let keep = max.saturating_sub(1);
    let head = keep / 2 + keep % 2;
    let tail = keep / 2;
    chars[..head]
        .iter()
        .chain(std::iter::once(&'…'))
        .chain(chars[chars.len() - tail..].iter())
        .collect()
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

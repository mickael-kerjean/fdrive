use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DialogBoxParamW, EndDialog, GetDlgItem, GetDlgItemTextW, GetWindowLongPtrW, LoadCursorW, MessageBoxW,
    SendDlgItemMessageW, SetCursor, SetWindowLongPtrW, SetWindowTextW, GWLP_USERDATA, IDCANCEL, IDC_WAIT, IDOK, LBN_DBLCLK,
    LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, MB_ICONERROR, MB_OK, SW_SHOWNORMAL, WM_COMMAND,
    WM_INITDIALOG,
};

const DIALOG: usize = 100;
const QUERY: i32 = 101;
const RESULTS: i32 = 102;

pub enum Event {
    Search,
    Open,
    Close,
}

pub trait Handler {
    fn handle(&mut self, dialog: Dialog, event: Event);
}

pub fn show_modal<H: Handler>(handler: &mut H) -> Result<(), String> {
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|err| err.to_string())?;
        let handler = handler as *mut H as isize;
        DialogBoxParamW(Some(instance.into()), PCWSTR(DIALOG as *const u16), None, Some(proc::<H>), LPARAM(handler));
    }
    Ok(())
}

pub fn error(message: &str) {
    unsafe { MessageBoxW(None, &HSTRING::from(message), w!("Filestash Search"), MB_OK | MB_ICONERROR) };
}

#[derive(Clone, Copy)]
pub struct Dialog(HWND);

impl Dialog {
    pub fn query(self) -> String {
        let mut buffer = [0u16; 512];
        let length = unsafe { GetDlgItemTextW(self.0, QUERY, &mut buffer) } as usize;
        String::from_utf16_lossy(&buffer[..length]).trim().to_owned()
    }

    pub fn show<'a>(self, labels: impl IntoIterator<Item = &'a str>) {
        unsafe { SendDlgItemMessageW(self.0, RESULTS, LB_RESETCONTENT, WPARAM(0), LPARAM(0)) };
        for label in labels {
            let label: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
            unsafe { SendDlgItemMessageW(self.0, RESULTS, LB_ADDSTRING, WPARAM(0), LPARAM(label.as_ptr() as isize)) };
        }
    }

    pub fn selected(self) -> Option<usize> {
        let index = unsafe { SendDlgItemMessageW(self.0, RESULTS, LB_GETCURSEL, WPARAM(0), LPARAM(0)) }.0;
        usize::try_from(index).ok()
    }

    pub fn title(self, title: &str) {
        let _ = unsafe { SetWindowTextW(self.0, &HSTRING::from(title)) };
    }

    pub fn open(self, url: &str) {
        unsafe { ShellExecuteW(Some(self.0), w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    }

    pub fn busy<T>(self, title: &str, work: impl FnOnce() -> T) -> T {
        self.title(title);
        self.show([]);
        let previous = unsafe {
            let _ = UpdateWindow(self.0);
            SetCursor(LoadCursorW(None, IDC_WAIT).ok())
        };
        let result = work();
        unsafe { SetCursor(Some(previous)) };
        result
    }

    pub fn close(self) {
        let _ = unsafe { EndDialog(self.0, 0) };
    }

    fn event(self, wparam: WPARAM) -> Option<Event> {
        let (id, code) = ((wparam.0 & 0xffff) as i32, (wparam.0 >> 16) as u32);
        let in_results = unsafe { GetDlgItem(Some(self.0), RESULTS) }.is_ok_and(|list| list == unsafe { GetFocus() });
        match id {
            _ if id == IDOK.0 && in_results => Some(Event::Open),
            _ if id == IDOK.0 => Some(Event::Search),
            _ if id == IDCANCEL.0 => Some(Event::Close),
            RESULTS if code == LBN_DBLCLK => Some(Event::Open),
            _ => None,
        }
    }
}

unsafe extern "system" fn proc<H: Handler>(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    if msg == WM_INITDIALOG {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam.0);
        return 1;
    }
    let handler = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut H;
    if msg != WM_COMMAND || handler.is_null() {
        return 0;
    }
    let dialog = Dialog(hwnd);
    match dialog.event(wparam) {
        Some(event) => {
            (*handler).handle(dialog, event);
            1
        }
        None => 0,
    }
}

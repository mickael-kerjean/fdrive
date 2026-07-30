use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MESSAGEBOX_STYLE,
};

pub fn alert(message: &str) {
    message_box(message, MB_ICONERROR);
}

pub fn info(message: &str) {
    message_box(message, MB_ICONINFORMATION);
}

fn message_box(message: &str, icon: MESSAGEBOX_STYLE) {
    let text = wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), w!("Filestash"), MB_OK | icon);
    }
}

fn wide(message: &str) -> Vec<u16> {
    message.encode_utf16().chain(std::iter::once(0)).collect()
}

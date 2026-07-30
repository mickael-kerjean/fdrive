use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK,
    MB_YESNO, MESSAGEBOX_STYLE,
};

pub fn alert(message: &str) {
    message_box(message, MB_ICONERROR);
}

pub fn info(message: &str) {
    message_box(message, MB_ICONINFORMATION);
}

pub fn confirm_deletions(held: usize) -> bool {
    let plural = if held == 1 { "" } else { "s" };
    let text = wide(&format!(
        "Filestash blocked {held} deletion{plural} from reaching the server.\n\
         This many at once is unusual and may be a bug.\n\n\
         Delete them on the server too?\n\n\
         Yes — delete on the server\n\
         No — keep everything, restore here"
    ));
    let picked = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("Filestash"),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };
    picked == IDYES
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

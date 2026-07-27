use gtk::prelude::*;

use super::{normalize_server, Credentials};

pub(super) fn show_login(prefill: Credentials) -> Option<Credentials> {
    let dialog = gtk::Dialog::new();
    dialog.set_title("Filestash");
    dialog.set_default_size(320, -1);
    dialog.set_border_width(12);
    dialog.add_button("Login", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(8);
    grid.set_margin_bottom(12);
    let label = gtk::Label::new(Some("Server"));
    label.set_halign(gtk::Align::Start);
    let server = gtk::Entry::new();
    server.set_hexpand(true);
    server.set_activates_default(true);
    server.set_text(&prefill.url);
    grid.attach(&label, 0, 0, 1, 1);
    grid.attach(&server, 1, 0, 1, 1);
    server.grab_focus();
    dialog.content_area().add(&grid);
    dialog.show_all();

    let accepted = dialog.run() == gtk::ResponseType::Accept;
    let raw = server.text();
    unsafe {
        dialog.destroy();
    }
    if !accepted || raw.trim().is_empty() {
        return None;
    }
    let url = normalize_server(&raw);
    if let Err(err) = fdrive_core::sdk::Sdk::builder(&url)
        .insecure(prefill.insecure)
        .probe_blocking()
    {
        alert(&format!(
            "{url} does not look like a Filestash server.\n\n{err}"
        ));
        return None;
    }
    match crate::webview::login(&url, prefill.insecure) {
        Ok(Some(token)) => Some(Credentials {
            url,
            token,
            insecure: prefill.insecure,
            ..Default::default()
        }),
        Ok(None) => None,
        Err(err) => {
            alert(&format!(
                "{err}\n\nInstall webkit2gtk, or use --token / --user from the command line."
            ));
            None
        }
    }
}

fn alert(message: &str) {
    let dialog = gtk::MessageDialog::new(
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Close,
        message,
    );
    dialog.set_title("Filestash");
    dialog.run();
    unsafe {
        dialog.destroy();
    }
}

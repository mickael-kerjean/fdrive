use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use fdrive_core::activity::{fmt_compact, rate_line, sparkline, Activity, Direction, Mode, Outcome, Snapshot, Transfer};
use gtk::prelude::*;

thread_local! {
    static OPEN: gtk::glib::WeakRef<gtk::Window> = gtk::glib::WeakRef::new();
}

pub(super) fn show_stats(activity: Arc<Activity>, near: Option<(i32, i32)>) {
    if let Some(existing) = OPEN.with(|open| open.upgrade()) {
        unsafe {
            existing.destroy();
        }
        OPEN.with(|open| open.set(None));
        return;
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Filestash — activity");
    window.set_default_size(380, 400);
    window.set_border_width(8);
    window.set_type_hint(gtk::gdk::WindowTypeHint::Dialog);

    let spark = gtk::Label::new(None);
    spark.set_halign(gtk::Align::Start);
    let rate = gtk::Label::new(None);
    rate.set_halign(gtk::Align::End);
    rate.set_width_chars(22);
    rate.set_xalign(1.0);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_border_width(4);
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    let viewport = gtk::Viewport::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    viewport.set_shadow_type(gtk::ShadowType::None);
    viewport.add(&list);
    scroll.add(&viewport);
    let style = gtk::CssProvider::new();
    style
        .load_from_data(b"scrolledwindow { background-color: @theme_base_color; border-radius: 10px; } viewport, list { background-color: transparent; }")
        .expect("valid activity container CSS");
    for context in [scroll.style_context(), viewport.style_context(), list.style_context()] {
        context.add_provider(&style, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.pack_start(&spark, false, false, 0);
    header.pack_end(&rate, false, false, 0);

    let menu = gtk::Menu::new();
    let paths = Rc::new(RefCell::new(Vec::<String>::new()));
    let selected = Rc::new(RefCell::new(None::<String>));
    let copy = gtk::MenuItem::with_label("Copy");
    {
        let selected = selected.clone();
        copy.connect_activate(move |_| {
            if let Some(path) = selected.borrow().as_ref() {
                gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).set_text(path.trim_start_matches('/'));
            }
        });
    }
    menu.append(&copy);
    let clear = gtk::MenuItem::with_label("Clear");
    menu.append(&clear);
    menu.show_all();

    {
        let paths = paths.clone();
        list.connect_button_press_event(move |list, event| {
            if !event.triggers_context_menu() {
                return gtk::Inhibit(false);
            }
            let path = list
                .row_at_y(event.position().1 as i32)
                .and_then(|row| paths.borrow().get(row.index() as usize).cloned());
            copy.set_sensitive(path.is_some());
            *selected.borrow_mut() = path;
            menu.popup_easy(event.button(), event.time());
            gtk::Inhibit(true)
        });
    }

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
    vbox.pack_start(&header, false, false, 0);
    vbox.pack_start(&scroll, true, true, 0);
    window.add(&vbox);

    let shown = Cell::new(u64::MAX);
    let list = list.downgrade();
    let refresh = Rc::new({
        let activity = activity.clone();
        move || {
            let Some(list) = list.upgrade() else { return };
            let snap = activity.snapshot();
            spark.set_markup(&format!("<tt>{}</tt>", sparkline(&snap, 24)));
            rate.set_markup(&format!("<tt>{}</tt>", rate_line(&snap)));
            if shown.replace(snap.version) != snap.version {
                *paths.borrow_mut() = rebuild_rows(&list, &snap);
            }
        }
    });
    refresh();
    {
        let refresh = refresh.clone();
        clear.connect_activate(move |_| {
            activity.clear();
            refresh();
        });
    }
    {
        let window = window.downgrade();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let Some(_alive) = window.upgrade() else {
                return gtk::glib::Continue(false);
            };
            refresh();
            gtk::glib::Continue(true)
        });
    }
    if let Some((center, y)) = near {
        window.move_(center - 190, y);
    }
    OPEN.with(|open| open.set(Some(&window)));
    window.show_all();
    let (width, height) = window.size();
    window.resize(width, height);
    if let Some((center, y)) = near {
        let right = window.screen().map(|s| s.width() - width - 8).unwrap_or(i32::MAX);
        window.move_((center - width / 2).clamp(8, right.max(8)), y);
    }
}

fn rebuild_rows(list: &gtk::ListBox, snap: &Snapshot) -> Vec<String> {
    for child in list.children() {
        list.remove(&child);
    }
    let mut transfers = snap.transfers.iter().collect::<Vec<_>>();
    if transfers.is_empty() {
        let empty = gtk::Label::new(None);
        empty.set_markup("<span size=\"xx-large\" alpha=\"35%\">⊘</span>");
        empty.set_margin_top(64);
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.set_selectable(false);
        row.add(&empty);
        list.add(&row);
        list.show_all();
        return Vec::new();
    }
    transfers.sort_by_key(|transfer| match &transfer.outcome {
        Outcome::Failed(_) => 0,
        Outcome::Running => 1,
        Outcome::Done => 2,
    });
    for (index, t) in transfers.iter().enumerate() {
        let detail = transfer_detail(t);
        let icon = transfer_icon(t.direction, &list.style_context());
        let name = gtk::Label::new(None);
        name.set_markup(&format!(
            "<tt>{}</tt>",
            gtk::glib::markup_escape_text(t.path.trim_start_matches('/')),
        ));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let subtitle = gtk::Label::new(None);
        subtitle.set_markup(&format!(
            "<span font_family=\"monospace\" size=\"small\" alpha=\"65%\">{}</span>",
            gtk::glib::markup_escape_text(&detail),
        ));
        subtitle.set_xalign(0.0);
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.pack_start(&name, false, false, 0);
        text.pack_start(&subtitle, false, false, 0);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        row.set_margin_start(9);
        row.set_margin_end(9);
        row.set_margin_top(if index == 0 { 9 } else { 3 });
        row.set_margin_bottom(if index + 1 == transfers.len() { 9 } else { 3 });
        row.pack_start(&icon, false, false, 0);
        row.pack_start(&text, true, true, 0);
        let target = gtk::ListBoxRow::new();
        target.set_activatable(false);
        target.add(&row);
        list.add(&target);
    }
    list.show_all();
    transfers.iter().map(|t| t.path.clone()).collect()
}

fn transfer_detail(t: &Transfer) -> String {
    let status = match (&t.outcome, t.direction) {
        (Outcome::Failed(why), _) => return format!("Failed · {why}"),
        (Outcome::Running, Direction::Down) => "Downloading",
        (Outcome::Running, Direction::Up) => "Uploading",
        (Outcome::Done, Direction::Down) => "Downloaded",
        (Outcome::Done, Direction::Up) => "Uploaded",
    };
    let size = fmt_compact(t.size);
    let bytes = match (&t.outcome, t.mode) {
        (_, Mode::Delta) => format!("Δ{} of {size}", fmt_compact(t.wire)),
        (Outcome::Running, _) if t.progress > 0 => format!("{} / {size}", fmt_compact(t.progress)),
        _ => size,
    };
    format!("{status} · {bytes}")
}

fn transfer_icon(direction: Direction, style: &gtk::StyleContext) -> gtk::Image {
    let svg = match direction {
        Direction::Up => include_str!("../../assets/document-upload.svg"),
        Direction::Down => include_str!("../../assets/document-download.svg"),
    };
    let color = style.color(gtk::StateFlags::NORMAL).to_string();
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.set_size(19, 24);
    loader
        .write(svg.replace("fill=\"black\"", &format!("fill=\"{color}\"")).as_bytes())
        .expect("valid transfer SVG");
    loader.close().expect("complete transfer SVG");
    gtk::Image::from_pixbuf(loader.pixbuf().as_ref())
}

use std::sync::Arc;

use fdrive_core::activity::{fmt_compact, rate_line, sparkline, Activity, Direction, Mode, Outcome, Snapshot};
use gtk::prelude::*;

thread_local! {
    static OPEN: std::cell::RefCell<Option<gtk::glib::WeakRef<gtk::Window>>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn show_stats(activity: Arc<Activity>, near: Option<(i32, i32)>) {
    use std::cell::Cell;
    use std::rc::Rc;

    if let Some(existing) = OPEN.with_borrow(|open| open.as_ref().and_then(|w| w.upgrade())) {
        unsafe {
            existing.destroy();
        }
        OPEN.with_borrow_mut(|open| *open = None);
        return;
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Filestash — activity");
    window.set_default_size(320, 400);
    window.set_border_width(8);
    window.set_type_hint(gtk::gdk::WindowTypeHint::Dialog);

    let spark = gtk::Label::new(None);
    spark.set_halign(gtk::Align::Start);
    let rate = gtk::Label::new(None);
    rate.set_halign(gtk::Align::End);
    rate.set_width_chars(22);
    rate.set_xalign(1.0);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_vexpand(true);
    scroll.add(&list);
    if let Some(viewport) = scroll.child() {
        viewport.style_context().add_class("view");
    }

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.pack_start(&spark, false, false, 0);
    header.pack_end(&rate, false, false, 0);

    let menu = gtk::Menu::new();
    let clear = gtk::MenuItem::with_label("Clear");
    menu.append(&clear);
    menu.show_all();

    let activity_area = gtk::EventBox::new();
    activity_area.add(&scroll);
    activity_area.add_events(gtk::gdk::EventMask::BUTTON_PRESS_MASK);
    activity_area.connect_button_press_event(move |_, event| {
        let context_menu = event.triggers_context_menu();
        if context_menu {
            menu.popup_easy(event.button(), event.time());
        }
        gtk::Inhibit(context_menu)
    });

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
    vbox.pack_start(&header, false, false, 0);
    vbox.pack_start(&activity_area, true, true, 0);
    window.add(&vbox);

    let snap = activity.snapshot();
    spark.set_markup(&format!("<tt>{}</tt>", sparkline(&snap, 24)));
    rate.set_markup(&format!("<tt>{}</tt>", rate_line(&snap)));
    rebuild_rows(&list, &snap, 0);

    let cleared = Rc::new(Cell::new(0u64));
    {
        let activity = activity.clone();
        let list = list.clone();
        let cleared = cleared.clone();
        clear.connect_activate(move |_| {
            let snap = activity.snapshot();
            let latest = snap.transfers.iter().map(|t| t.id).max();
            cleared.set(latest.unwrap_or(cleared.get()));
            rebuild_rows(&list, &snap, cleared.get());
        });
    }
    {
        let window = window.downgrade();
        let mut shown = snap.version;
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let Some(_alive) = window.upgrade() else {
                return gtk::glib::Continue(false);
            };
            let snap = activity.snapshot();
            spark.set_markup(&format!("<tt>{}</tt>", sparkline(&snap, 24)));
            rate.set_markup(&format!("<tt>{}</tt>", rate_line(&snap)));
            if snap.version != shown {
                shown = snap.version;
                rebuild_rows(&list, &snap, cleared.get());
            }
            gtk::glib::Continue(true)
        });
    }
    if let Some((center, y)) = near {
        window.move_(center - 190, y);
    }
    OPEN.with_borrow_mut(|open| *open = Some(window.downgrade()));
    window.show_all();
    let (width, height) = window.size();
    window.resize(width, height);
    if let Some((center, y)) = near {
        let right = window.screen().map(|s| s.width() - width - 8).unwrap_or(i32::MAX);
        window.move_((center - width / 2).clamp(8, right.max(8)), y);
    }
}

fn rebuild_rows(list: &gtk::Box, snap: &Snapshot, cleared: u64) {
    for child in list.children() {
        list.remove(&child);
    }
    let mut transfers = snap
        .transfers
        .iter()
        .filter(|transfer| transfer.id > cleared)
        .collect::<Vec<_>>();
    if transfers.is_empty() {
        let empty = gtk::Label::new(None);
        empty.set_markup("<span size=\"xx-large\" alpha=\"35%\">⊘</span>");
        empty.set_margin_top(64);
        list.add(&empty);
        list.show_all();
        return;
    }
    transfers.sort_by_key(|transfer| match &transfer.outcome {
        Outcome::Failed(_) => 0,
        Outcome::Running => 1,
        Outcome::Done => 2,
    });
    for t in transfers {
        let detail = match &t.outcome {
            Outcome::Running => None,
            Outcome::Failed(_) => Some("✕".to_string()),
            Outcome::Done => Some(fmt_compact(t.size)),
        };
        let extra = match &t.outcome {
            Outcome::Done if t.mode == Mode::Delta => Some(format!("⇄{}", fmt_compact(t.wire))),
            _ => None,
        };
        let arrow = gtk::Label::new(Some(match t.direction {
            Direction::Down => "↓",
            Direction::Up => "↑",
        }));
        let name = gtk::Label::new(Some(t.path.trim_start_matches('/')));
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_border_width(4);
        row.pack_start(&arrow, false, false, 0);
        row.pack_start(&name, false, true, 0);
        if let Some(extra) = extra {
            let extra_label = gtk::Label::new(None);
            extra_label.set_markup(&format!("<span alpha=\"60%\">({extra})</span>"));
            row.pack_start(&extra_label, false, false, 0);
        }
        if let Some(detail) = detail {
            let detail = gtk::Label::new(Some(&detail));
            row.pack_end(&detail, false, false, 0);
        }
        if let Outcome::Failed(why) = &t.outcome {
            row.set_tooltip_text(Some(why));
        }
        list.add(&row);
    }
    list.show_all();
}

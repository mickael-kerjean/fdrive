use std::cell::RefCell;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fdrive_core::activity::Activity;
use gtk::prelude::*;
use libayatana_appindicator::{AppIndicator, AppIndicatorStatus};
use tokio::sync::mpsc::UnboundedSender;

use super::dashboard::show_stats;
use super::login::show_login;
use super::{Credentials, Status, TrayEvent};

struct Ctx {
    events: UnboundedSender<TrayEvent>,
    mount: PathBuf,
}

enum TrayMsg {
    Set(Status, bool),
    Attach(Arc<Activity>),
    Login(
        Credentials,
        tokio::sync::oneshot::Sender<Option<Credentials>>,
    ),
    Quit,
}

pub struct Tray {
    tx: gtk::glib::Sender<TrayMsg>,
}

impl Tray {
    pub async fn spawn(
        events: UnboundedSender<TrayEvent>,
        data_dir: PathBuf,
        mount: PathBuf,
    ) -> std::io::Result<Self> {
        let icon_dir = data_dir.join("icons");
        ensure_icons(&icon_dir)?;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("fdrive-tray".into())
            .spawn(move || tray_thread(ready_tx, Ctx { events, mount }, icon_dir))?;
        match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(tx)) => Ok(Self { tx }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(std::io::Error::other("tray thread did not start")),
        }
    }

    pub async fn set(&self, status: Status, signed_in: bool) {
        let _ = self.tx.send(TrayMsg::Set(status, signed_in));
    }

    pub async fn attach(&self, activity: Arc<Activity>) {
        let _ = self.tx.send(TrayMsg::Attach(activity));
    }

    pub async fn login(&self, prefill: Credentials) -> Option<Credentials> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if self.tx.send(TrayMsg::Login(prefill, reply_tx)).is_err() {
            return None;
        }
        reply_rx.await.ok().flatten()
    }

    pub async fn shutdown(self) {
        let _ = self.tx.send(TrayMsg::Quit);
    }
}

struct Ui {
    ctx: Ctx,
    icon_dir: PathBuf,
    signed_in: bool,
    activity: Option<Arc<Activity>>,
    menu: Option<gtk::Menu>,
    stats_in_menu: bool,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    UI.with_borrow_mut(|ui| f(ui.as_mut().expect("tray ui")))
}

enum Backend {
    Indicator(AppIndicator),
    Xembed(*mut gtk::ffi::GtkStatusIcon),
}

type Ready = Result<gtk::glib::Sender<TrayMsg>, std::io::Error>;

fn tray_thread(ready: std::sync::mpsc::Sender<Ready>, ctx: Ctx, icon_dir: PathBuf) {
    if gtk::init().is_err() {
        let _ = ready.send(Err(std::io::Error::other(
            "could not connect GTK to the desktop session",
        )));
        return;
    }
    gtk::glib::log_set_writer_func(|level, fields| {
        let field = |key| {
            fields
                .iter()
                .find(|f| f.key() == key)
                .and_then(|f| f.value_str())
        };
        let benign = field("GLIB_DOMAIN") == Some("libayatana-appindicator")
            || field("MESSAGE").is_some_and(|m| m.contains("thaw_toplevel_updates"));
        if benign {
            gtk::glib::LogWriterOutput::Handled
        } else {
            gtk::glib::log_writer_default(level, fields)
        }
    });
    let (tx, rx) = gtk::glib::MainContext::channel(gtk::glib::PRIORITY_DEFAULT);
    UI.with_borrow_mut(|ui| {
        *ui = Some(Ui {
            ctx,
            icon_dir: icon_dir.clone(),
            signed_in: false,
            activity: None,
            menu: None,
            stats_in_menu: true,
        })
    });
    let mut backend = if sni_host_running() {
        let mut indicator = AppIndicator::with_path(
            "filestash",
            Status::LoggedOut.icon_name(),
            icon_dir.to_str().unwrap_or("."),
        );
        indicator.set_title(Status::LoggedOut.tip());
        indicator.set_status(AppIndicatorStatus::Active);
        let mut menu = with_ui(build_menu);
        indicator.set_menu(&mut menu);
        Backend::Indicator(indicator)
    } else {
        with_ui(|ui| ui.stats_in_menu = false);
        Backend::Xembed(xembed_icon(&icon_dir))
    };
    let _ = ready.send(Ok(tx));

    rx.attach(None, move |msg| match msg {
        TrayMsg::Set(status, signed) => {
            match &mut backend {
                Backend::Indicator(indicator) => {
                    indicator.set_icon(status.icon_name());
                    indicator.set_title(status.tip());
                }
                Backend::Xembed(icon) => xembed_set(*icon, status),
            }
            let changed = with_ui(|ui| {
                let changed = ui.signed_in != signed;
                ui.signed_in = signed;
                changed
            });
            if changed {
                if let Backend::Indicator(indicator) = &mut backend {
                    let mut menu = with_ui(build_menu);
                    indicator.set_menu(&mut menu);
                }
            }
            gtk::glib::Continue(true)
        }
        TrayMsg::Attach(handle) => {
            with_ui(|ui| ui.activity = Some(handle));
            if let Backend::Indicator(indicator) = &mut backend {
                let mut menu = with_ui(build_menu);
                indicator.set_menu(&mut menu);
            }
            gtk::glib::Continue(true)
        }
        TrayMsg::Login(prefill, reply) => {
            let _ = reply.send(show_login(prefill));
            gtk::glib::Continue(true)
        }
        TrayMsg::Quit => {
            gtk::main_quit();
            gtk::glib::Continue(false)
        }
    });
    gtk::main();
}

fn sni_host_running() -> bool {
    use gtk::gio;
    use gtk::glib::ToVariant;
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return false;
    };
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&("org.kde.StatusNotifierWatcher",).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        1000,
        None::<&gio::Cancellable>,
    )
    .ok()
    .and_then(|reply| reply.child_value(0).get::<bool>())
    .unwrap_or(false)
}

fn xembed_icon(icon_dir: &Path) -> *mut gtk::ffi::GtkStatusIcon {
    let path = svg_path(icon_dir, Status::LoggedOut);
    unsafe {
        let icon = gtk::ffi::gtk_status_icon_new_from_file(path.as_ptr());
        let tip = CString::new(Status::LoggedOut.tip()).unwrap_or_default();
        gtk::ffi::gtk_status_icon_set_tooltip_text(icon, tip.as_ptr());
        gtk::glib::signal::connect_raw::<()>(
            icon as *mut gtk::glib::gobject_ffi::GObject,
            c"activate".as_ptr(),
            Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(
                on_activate as *const (),
            )),
            std::ptr::null_mut(),
        );
        gtk::glib::signal::connect_raw::<()>(
            icon as *mut gtk::glib::gobject_ffi::GObject,
            c"popup-menu".as_ptr(),
            Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(
                on_popup as *const (),
            )),
            std::ptr::null_mut(),
        );
        icon
    }
}

fn svg_path(icon_dir: &Path, status: Status) -> CString {
    let path = icon_dir.join(format!("{}.svg", status.icon_name()));
    CString::new(path.to_str().unwrap_or_default()).unwrap_or_default()
}

fn xembed_set(icon: *mut gtk::ffi::GtkStatusIcon, status: Status) {
    let path = with_ui(|ui| svg_path(&ui.icon_dir, status));
    let tip = CString::new(status.tip()).unwrap_or_default();
    unsafe {
        gtk::ffi::gtk_status_icon_set_from_file(icon, path.as_ptr());
        gtk::ffi::gtk_status_icon_set_tooltip_text(icon, tip.as_ptr());
    }
}

unsafe extern "C" fn on_activate(
    icon: *mut gtk::ffi::GtkStatusIcon,
    _data: gtk::glib::ffi::gpointer,
) {
    match with_ui(|ui| ui.activity.clone()) {
        Some(activity) => show_stats(activity, stats_position(icon)),
        None => popup_menu(0, gtk::current_event_time()),
    }
}

unsafe extern "C" fn on_popup(
    _icon: *mut gtk::ffi::GtkStatusIcon,
    button: std::os::raw::c_uint,
    time: std::os::raw::c_uint,
    _data: gtk::glib::ffi::gpointer,
) {
    popup_menu(button, time);
}

fn popup_menu(button: u32, time: u32) {
    let menu = with_ui(build_menu);
    menu.popup_easy(button, time);
    with_ui(|ui| ui.menu = Some(menu));
}

fn stats_position(icon: *mut gtk::ffi::GtkStatusIcon) -> Option<(i32, i32)> {
    unsafe {
        let mut screen: *mut gtk::gdk::ffi::GdkScreen = std::ptr::null_mut();
        let mut rect: gtk::gdk::ffi::GdkRectangle = std::mem::zeroed();
        let mut orientation: gtk::ffi::GtkOrientation = 0;
        if gtk::ffi::gtk_status_icon_get_geometry(icon, &mut screen, &mut rect, &mut orientation)
            == 0
        {
            return None;
        }
        let screen_h = gtk::gdk::ffi::gdk_screen_get_height(screen);
        let center = rect.x + rect.width / 2;
        let y = if rect.y > screen_h / 2 {
            (rect.y - 456).max(8)
        } else {
            rect.y + rect.height + 8
        };
        Some((center, y))
    }
}

fn build_menu(ui: &mut Ui) -> gtk::Menu {
    let menu = gtk::Menu::new();
    let item = |label: &str, event: TrayEvent| {
        let item = gtk::MenuItem::with_label(label);
        let events = ui.ctx.events.clone();
        item.connect_activate(move |_| {
            let _ = events.send(event.clone());
        });
        item
    };
    if ui.signed_in {
        if file_manager().is_some() {
            let browse = gtk::MenuItem::with_label("Browse");
            let mount = ui.ctx.mount.clone();
            browse.connect_activate(move |_| open_folder(&mount));
            menu.append(&browse);
        }
        if ui.stats_in_menu {
            if let Some(activity) = &ui.activity {
                let stats = gtk::MenuItem::with_label("Statistics");
                let activity = activity.clone();
                stats.connect_activate(move |_| show_stats(activity.clone(), None));
                menu.append(&stats);
            }
        }
        menu.append(&item("Logout", TrayEvent::Logout));
        menu.append(&item("Restart", TrayEvent::Restart));
    } else {
        menu.append(&item("Login…", TrayEvent::Login));
    }
    menu.append(&gtk::SeparatorMenuItem::new());
    menu.append(&item("Quit", TrayEvent::Quit));
    menu.show_all();
    menu
}

fn ensure_icons(dir: &Path) -> std::io::Result<()> {
    const ICONS: [(&str, &str); 4] = [
        (
            "icon-base.svg",
            include_str!("../../../fdrive-core/icons/icon-base.svg"),
        ),
        (
            "icon-ok.svg",
            include_str!("../../../fdrive-core/icons/icon-ok.svg"),
        ),
        (
            "icon-sync.svg",
            include_str!("../../../fdrive-core/icons/icon-sync.svg"),
        ),
        (
            "icon-error.svg",
            include_str!("../../../fdrive-core/icons/icon-error.svg"),
        ),
    ];
    std::fs::create_dir_all(dir)?;
    for (name, svg) in ICONS {
        std::fs::write(dir.join(name), update_svg(svg))?;
    }
    Ok(())
}

fn update_svg(svg: &str) -> String {
    use std::sync::LazyLock;
    static PAINT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(stroke|fill):#([0-9a-fA-F]{3,6})").unwrap());
    static WIDTH: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"stroke-width="([0-9.]+)"#).unwrap());

    let is_white = |hex: &str| matches!(hex.to_lowercase().as_str(), "fff" | "ffffff");
    let out = PAINT.replace_all(svg, |caps: &regex::Captures| {
        match (&caps[1], is_white(&caps[2])) {
            ("stroke", _) => "stroke:#ffffff".to_string(),
            ("fill", true) => caps[0].to_string(),
            ("fill", _) => "fill:none".to_string(),
            _ => unreachable!(),
        }
    });
    WIDTH
        .replace_all(&out, |caps: &regex::Captures| {
            let width: f32 = caps[1].parse().unwrap_or(0.0);
            format!(r#"stroke-width="{:.3}"#, width * 0.6)
        })
        .into_owned()
}

fn file_manager() -> Option<gtk::gio::AppInfo> {
    use gtk::gio;
    use gtk::gio::prelude::AppInfoExt;
    let is_manager = |app: &gio::AppInfo| {
        app.id()
            .and_then(|id| gio::DesktopAppInfo::new(&id))
            .and_then(|info| info.categories())
            .is_some_and(|categories| categories.split(';').any(|c| c == "FileManager"))
    };
    gio::AppInfo::default_for_type("inode/directory", false)
        .filter(is_manager)
        .or_else(|| {
            gio::AppInfo::all_for_type("inode/directory")
                .into_iter()
                .find(is_manager)
        })
}

fn open_folder(path: &Path) {
    use gtk::gio;
    use gtk::gio::prelude::AppInfoExt;
    if let Some(app) = file_manager() {
        let _ = app.launch(&[gio::File::for_path(path)], None::<&gio::AppLaunchContext>);
    }
}

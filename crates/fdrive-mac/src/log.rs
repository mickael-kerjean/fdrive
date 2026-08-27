pub fn init() {
    let _ = oslog::OsLogger::new("app.filestash.mac.fileprovider")
        .level_filter(log::LevelFilter::Debug)
        .init();
}

#![windows_subsystem = "windows"]

mod ui;
mod win32;

use std::path::PathBuf;

use fdrive_core::remote::{FileType, Remote};

struct Hit {
    label: String,
    url: String,
}

fn main() {
    if let Err(err) = run() {
        ui::error(&err);
    }
}

fn run() -> Result<(), String> {
    let data = PathBuf::from(std::env::var("LOCALAPPDATA").map_err(|_| "%LOCALAPPDATA% is not set")?).join("Filestash");
    let session = fdrive_core::config::load(&data);
    if !session.ok() {
        return Err("Sign in with Filestash Drive first, search uses the same account.".into());
    }
    let base = url::Url::parse(&session.url).map_err(|err| err.to_string())?;
    let remote = Remote::builder(&data).build().map_err(|err| err.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|err| err.to_string())?;

    ui::run(|term| {
        let files = runtime.block_on(remote.search("/", term)).map_err(|err| err.to_string())?;
        Ok(files
            .iter()
            .map(|(path, info)| Hit {
                label: path.clone(),
                url: link(&base, path, info.kind == FileType::Directory),
            })
            .collect())
    })
}

fn link(base: &url::Url, path: &str, is_dir: bool) -> String {
    let mut url = base.clone();
    if let Ok(mut segments) = url.path_segments_mut() {
        segments
            .pop_if_empty()
            .push(if is_dir { "files" } else { "view" })
            .extend(path.split('/').filter(|segment| !segment.is_empty()));
        if is_dir {
            segments.push("");
        }
    }
    url.to_string()
}

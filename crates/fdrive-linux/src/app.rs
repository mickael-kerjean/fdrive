use std::path::{Path, PathBuf};

use clap::Parser;
use fdrive_linux::gui::{self, Boot, Credentials};

#[derive(Parser)]
#[command(
    name = "fdrive",
    about = "Filestash drive client",
    after_help = "Environment:\n  FILESTASH_SERVER  server to sign in to\n  FILESTASH_TOKEN   session token, skips the sign in (needs FILESTASH_SERVER)"
)]
struct Args {
    #[arg(value_name = "MOUNT")]
    mount: PathBuf,
    #[arg(long)]
    data: Option<PathBuf>,
    #[arg(long)]
    insecure: bool,
}

pub struct Setup {
    pub mount: PathBuf,
    pub data: PathBuf,
    pub prefill: Credentials,
    pub boot: Boot,
}

pub fn init() -> Result<Setup, Box<dyn std::error::Error>> {
    let args = Args::parse();
    let data = args.data.unwrap_or_else(gui::default_data);
    std::fs::create_dir_all(&data)?;
    crate::log::init(&data)?;
    instance_lock(&data)?;

    let env = |name| std::env::var(name).ok().filter(|v: &String| !v.trim().is_empty());
    let (server, token) = (env("FILESTASH_SERVER"), env("FILESTASH_TOKEN"));
    if server.is_none() && token.is_some() {
        return Err("FILESTASH_TOKEN needs FILESTASH_SERVER".into());
    }
    let server = server.as_deref().map(gui::normalize_server);
    let boot = match (&server, token) {
        (Some(url), Some(token)) => Boot::Fresh(Credentials {
            url: url.clone(),
            token,
            insecure: args.insecure,
        }),
        (Some(_), None) => Boot::Prompt,
        (None, _) => {
            let session = fdrive_core::config::load(&data);
            match session.ok() {
                true => Boot::Restored(Credentials::from(session)),
                false => Boot::Idle,
            }
        }
    };
    Ok(Setup {
        mount: args.mount,
        prefill: Credentials {
            url: server.unwrap_or_default(),
            insecure: args.insecure,
            ..Default::default()
        },
        data,
        boot,
    })
}

fn instance_lock(data: &Path) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(data.join("fdrive.lock"))
        .map_err(|err| format!("fdrive.lock: {err}"))?;
    match unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } {
        0 => {
            std::mem::forget(file);
            Ok(())
        }
        _ => Err(format!(
            "another instance is already running on {} — quit it first",
            data.display()
        )),
    }
}

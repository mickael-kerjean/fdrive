use std::path::{Path, PathBuf};

use clap::Parser;
use fdrive_core::sdk::normalize_server;

use fdrive_windows::config::AppConfig;
use fdrive_windows::gui::{self, Boot, Credentials};

#[derive(Parser)]
#[command(name = "fdrive-windows", about = "Filestash drive client — Windows")]
struct Args {
    #[arg(long, value_name = "URL")]
    server: Option<String>,
    #[arg(long, env = "FILESTASH_TOKEN", hide_env_values = true)]
    token: Option<String>,
    #[arg(long)]
    user: Option<String>,
    #[arg(long, env = "FILESTASH_PASSWORD", hide_env_values = true)]
    password: Option<String>,
    #[arg(long)]
    storage: Option<String>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    insecure: bool,
    #[arg(long)]
    unregister: bool,
}

pub struct Setup {
    pub root: PathBuf,
    pub data: PathBuf,
    pub config: AppConfig,
    pub unregister: bool,
    pub boot: Boot,
}

pub fn init() -> Setup {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(err)
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            gui::info(&err.to_string());
            std::process::exit(0);
        }
        Err(err) => fail(&err.to_string()),
    };

    let root = std::env::var("USERPROFILE")
        .map(|home| PathBuf::from(home).join("Filestash"))
        .unwrap_or_else(|_| fail("no %USERPROFILE%"));
    let data = std::env::var("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("Filestash"))
        .unwrap_or_else(|_| fail("no %LOCALAPPDATA%"));
    if let Err(err) = std::fs::create_dir_all(&data) {
        fail(&format!("{}: {err}", data.display()));
    }
    crate::log::init(&data).unwrap_or_else(|err| fail(&err.to_string()));
    instance_lock(&data, &root).unwrap_or_else(|err| fail(&err));
    let config_path = args.config.unwrap_or_else(|| data.join("fdrive.toml"));
    let config = AppConfig::load(&config_path).unwrap_or_else(|err| fail(&err.to_string()));

    if args.token.is_some() && args.user.is_some() {
        fail("--token and --user cannot be combined");
    }
    if args.server.is_none() && (args.token.is_some() || args.user.is_some()) {
        fail("--token and --user need --server");
    }
    if args.user.is_some() && args.password.as_deref().unwrap_or("").is_empty() {
        fail("--user needs --password (or FILESTASH_PASSWORD)");
    }
    let stored = fdrive_core::config::load(&data);
    let server = args.server.as_deref().map(normalize_server);
    let boot = match (&server, args.token, &args.user) {
        (Some(url), Some(token), _) => Boot::Fresh(Credentials {
            url: url.clone(),
            token,
            insecure: args.insecure,
            ..Default::default()
        }),
        (Some(url), None, Some(user)) => Boot::Fresh(Credentials {
            url: url.clone(),
            user: user.clone(),
            password: args.password.unwrap_or_default(),
            storage: args.storage.unwrap_or_default(),
            insecure: args.insecure,
            ..Default::default()
        }),
        (Some(url), None, None) => Boot::Prompt(url.clone()),
        (None, ..) => match stored.ok() {
            true => Boot::Restored(Credentials::from(stored)),
            false => Boot::Idle(Some(stored.url)),
        },
    };
    Setup {
        root,
        data,
        config,
        unregister: args.unregister,
        boot,
    }
}

fn fail(message: &str) -> ! {
    gui::alert(message);
    std::process::exit(2)
}

fn instance_lock(data: &Path, root: &Path) -> Result<(), String> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(data.join("instance.lock"))
        .map(std::mem::forget)
        .map_err(|_| {
            format!(
                "another instance is already running on {} — quit it first",
                root.display()
            )
        })
}

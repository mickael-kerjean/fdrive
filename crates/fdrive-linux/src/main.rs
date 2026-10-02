use std::path::PathBuf;

use clap::{Parser, Subcommand};
use fdrive_core::sdk::{login_url, normalize_server, Sdk};
use serde_json::json;

mod daemon;
mod ipc;

use ipc::Request;

#[derive(Parser)]
#[command(name = "fdrive", about = "Filestash Drive")]
struct Args {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Daemon,
    Status,
    LoginUrl { server: String },
    Login { server: String },
    Logout,
    Clear,
}

#[tokio::main]
async fn main() {
    let command = Args::parse().command;
    let result: Result<(), Box<dyn std::error::Error>> = async {
        let socket = ipc::socket_path()?;
        match command {
            Cmd::Daemon => {
                env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,fdrive_core=info")).init();
                let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
                let data = fdrive_linux::session::default_data();
                std::fs::create_dir_all(&data)?;
                daemon::run(PathBuf::from(home).join("Filestash"), data).await
            }
            Cmd::Status => {
                let state = match ipc::call(&socket, &Request::Status).await {
                    Ok(state) => state,
                    Err(_) => json!({ "phase": "stopped", "phaseText": "Off" }),
                };
                println!("{state}");
                Ok(())
            }
            Cmd::LoginUrl { server } => {
                let url = normalize_server(&server);
                Sdk::builder(&url).probe().await.map_err(|err| format!("{url}: {err}"))?;
                println!("{}", login_url(&url));
                Ok(())
            }
            Cmd::Login { server } => {
                let mut token = String::new();
                std::io::stdin().read_line(&mut token)?;
                let token = token.trim().to_owned();
                if token.is_empty() {
                    return Err("no token on stdin".into());
                }
                reply(ipc::call(&socket, &Request::Login { server, token }).await)
            }
            Cmd::Logout => reply(ipc::call(&socket, &Request::Logout).await),
            Cmd::Clear => reply(ipc::call(&socket, &Request::Clear).await),
        }
    }
    .await;
    if let Err(err) = result {
        eprintln!("fdrive: {err}");
        std::process::exit(1);
    }
}

fn reply(result: std::io::Result<serde_json::Value>) -> Result<(), Box<dyn std::error::Error>> {
    let value = result.map_err(|err| format!("the fdrive daemon is not running ({err})"))?;
    match value.get("error").and_then(|err| err.as_str()) {
        Some(err) => Err(err.into()),
        None => Ok(()),
    }
}

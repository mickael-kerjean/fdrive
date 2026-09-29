use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::signal::unix::{signal, SignalKind};

mod driver;

use driver::Driver;

const SOCKET: &str = "/run/docker/plugins/fdrive.sock";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let driver = Arc::new(Driver::new(tokio::runtime::Handle::current())?);
    std::fs::create_dir_all("/run/docker/plugins")?;
    let _ = std::fs::remove_file(SOCKET);
    let server = tiny_http::Server::http_unix(SOCKET.as_ref()).map_err(|err| err.to_string())?;
    log::info!("fdrive volume driver listening on {SOCKET}");
    std::thread::spawn({
        let driver = driver.clone();
        move || {
            for request in server.incoming_requests() {
                serve(&driver, request);
            }
        }
    });
    let mut term = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = term.recv() => {}
    }
    tokio::task::spawn_blocking(move || driver.shutdown()).await?;
    Ok(())
}

fn serve(driver: &Driver, mut request: tiny_http::Request) {
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    let req: Request = serde_json::from_str(&body).unwrap_or_default();
    let result = match request.url() {
        "/Plugin.Activate" => Ok(json!({ "Implements": ["VolumeDriver"] })),
        "/VolumeDriver.Capabilities" => Ok(json!({
            "Capabilities": { "Scope": "local" }
        })),
        "/VolumeDriver.Create" => driver.create(&req.name, req.opts.unwrap_or_default()).map(|_| json!({})),
        "/VolumeDriver.Remove" => driver.remove(&req.name).map(|_| json!({})),
        "/VolumeDriver.Mount" => driver.mount(&req.name, &req.id).map(|path| json!({
            "Mountpoint": path
        })),
        "/VolumeDriver.Unmount" => driver.unmount(&req.name, &req.id).map(|_| json!({})),
        "/VolumeDriver.Path" => driver.mountpoint(&req.name).map(|path| json!({
            "Mountpoint": path
        })),
        "/VolumeDriver.Get" => driver.mountpoint(&req.name).map(|path| json!({
            "Volume": { "Name": req.name, "Mountpoint": path }
        })),
        "/VolumeDriver.List" => Ok(json!({
            "Volumes": driver
                .names()
                .into_iter()
                .map(|name| json!({ "Name": name }))
                .collect::<Value>()
        })),
        url => Err(format!("unsupported endpoint {url}")),
    };
    let reply = result.unwrap_or_else(|err| {
        log::warn!("{} {}: {err}", request.url(), req.name);
        json!({ "Err": err })
    });
    let header = tiny_http::Header::from_bytes("Content-Type", "application/vnd.docker.plugins.v1.2+json").expect("static header");
    let resp = tiny_http::Response::from_string(reply.to_string());
    let _ = request.respond(resp.with_header(header));
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Request {
    #[serde(rename = "Name")] name: String,
    #[serde(rename = "ID")]   id: String,
    #[serde(rename = "Opts")] opts: Option<HashMap<String, String>>,
}

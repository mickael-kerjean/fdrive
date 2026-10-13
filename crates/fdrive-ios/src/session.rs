use std::collections::HashMap;
use std::path::Path;

use fdrive_core::remote::{self, Remote};

use crate::{runtime, FsError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct Session {
    pub url: String,
    pub token: String,
    pub ok: bool,
}

#[uniffi::export]
pub fn normalize_server(input: String) -> String {
    remote::normalize_server(&input)
}

#[uniffi::export]
pub fn login_url(base: String) -> String {
    remote::login_url(&base)
}

#[uniffi::export(async_runtime = "tokio")]
pub async fn probe(data_dir: String, url: String) -> Result<String, FsError> {
    Ok(Remote::builder(Path::new(&data_dir)).url(url).build()?.probe().await?)
}

#[uniffi::export]
pub fn logout(data_dir: String, url: String, token: String) -> Result<(), FsError> {
    let runtime = runtime()?;
    let sdk = Remote::builder(Path::new(&data_dir)).url(url).token(token).build()?;
    Ok(runtime.block_on(sdk.logout())?)
}

#[uniffi::export]
pub fn ping(data_dir: String, url: String, token: String) -> bool {
    let Ok(runtime) = runtime() else { return false };
    let Ok(sdk) = Remote::builder(Path::new(&data_dir)).url(url).token(token).build() else {
        return false;
    };
    runtime.block_on(sdk.ls("/")).is_ok()
}

#[uniffi::export]
pub fn assemble_token(cookies: HashMap<String, String>) -> String {
    let cookies: Vec<(String, String)> = cookies.into_iter().collect();
    remote::assemble_token(&cookies)
}

#[uniffi::export]
pub fn session_recall(data_dir: String) -> Session {
    let session = fdrive_core::config::load(Path::new(&data_dir));
    let ok = session.ok();
    Session { url: session.url, token: session.token, ok }
}

#[uniffi::export]
pub fn session_remember(data_dir: String, url: String, token: String) {
    fdrive_core::config::remember(Path::new(&data_dir), &url, &token);
}

#[uniffi::export]
pub fn session_forget(data_dir: String) {
    fdrive_core::config::forget(Path::new(&data_dir));
}

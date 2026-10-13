use std::collections::HashMap;
use std::path::Path;

use fdrive_core::sdk::{self, Sdk};

use crate::{runtime, FsError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct Session {
    pub url: String,
    pub token: String,
    pub ok: bool,
}

#[uniffi::export]
pub fn normalize_server(input: String) -> String {
    sdk::normalize_server(&input)
}

#[uniffi::export]
pub fn login_url(base: String) -> String {
    sdk::login_url(&base)
}

#[uniffi::export(async_runtime = "tokio")]
pub async fn probe(data_dir: String, url: String) -> Result<String, FsError> {
    Ok(Sdk::builder(Path::new(&data_dir)).url(url).build()?.probe().await?)
}

#[uniffi::export]
pub fn logout(data_dir: String, url: String, token: String) -> Result<(), FsError> {
    let runtime = runtime()?;
    let sdk = Sdk::builder(Path::new(&data_dir)).url(url).token(token).build()?;
    Ok(runtime.block_on(sdk.logout())?)
}

#[uniffi::export]
pub fn assemble_token(cookies: HashMap<String, String>) -> String {
    let cookies: Vec<(String, String)> = cookies.into_iter().collect();
    sdk::assemble_token(&cookies)
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct Features {
    pub server_locked: bool,
    pub remote_search: bool,
}

#[uniffi::export]
pub fn features_recall(data_dir: String) -> Features {
    let features = fdrive_core::config::features(Path::new(&data_dir));
    Features { server_locked: features.server_locked, remote_search: features.remote_search }
}

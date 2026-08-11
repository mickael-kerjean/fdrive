use std::collections::HashMap;
use std::path::Path;

use fdrive_core::sdk::{self, Sdk};

use crate::{runtime, FsError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct Session {
    pub url: String,
    pub token: String,
    pub insecure: bool,
    pub ok: bool,
}

#[uniffi::export]
pub fn normalize_server(input: String) -> String {
    sdk::normalize_server(&input)
}

#[uniffi::export(async_runtime = "tokio")]
pub async fn probe(url: String, insecure: bool) -> Result<String, FsError> {
    Ok(Sdk::builder(&url).insecure(insecure).probe().await?)
}

#[uniffi::export]
pub fn logout(url: String, insecure: bool, token: String) -> Result<(), FsError> {
    let runtime = runtime()?;
    let sdk = Sdk::builder(&url).insecure(insecure).token(token)?;
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
    Session { url: session.url, token: session.token, insecure: session.insecure, ok }
}

#[uniffi::export]
pub fn session_remember(data_dir: String, url: String, token: String, insecure: bool) {
    fdrive_core::config::remember(Path::new(&data_dir), &url, &token, insecure);
}

#[uniffi::export]
pub fn session_forget(data_dir: String) {
    fdrive_core::config::forget(Path::new(&data_dir));
}

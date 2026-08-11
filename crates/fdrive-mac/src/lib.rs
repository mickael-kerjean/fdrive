use std::io;

use fdrive_core::sdk;
use tokio::runtime::Runtime;

pub mod activity;
pub mod adapter;
pub mod session;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FsError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("not authenticated")]
    NotAuthenticated,
    #[error("permission denied")]
    PermissionDenied,
    #[error("not found")]
    NotFound,
    #[error("network error: {msg}")]
    Network { msg: String },
    #[error("{msg}")]
    Other { msg: String },
}

impl From<sdk::Error> for FsError {
    fn from(error: sdk::Error) -> Self {
        match error {
            sdk::Error::InvalidCredentials => Self::InvalidCredentials,
            sdk::Error::NotAuthenticated => Self::NotAuthenticated,
            sdk::Error::PermissionDenied => Self::PermissionDenied,
            sdk::Error::NotFound => Self::NotFound,
            sdk::Error::Http(error) => Self::Network { msg: error.to_string() },
            error => Self::Other { msg: error.to_string() },
        }
    }
}

impl From<io::Error> for FsError {
    fn from(error: io::Error) -> Self {
        Self::Other { msg: error.to_string() }
    }
}

fn runtime() -> Result<Runtime, FsError> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| FsError::Other { msg: error.to_string() })
}

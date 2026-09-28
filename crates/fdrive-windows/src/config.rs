use std::fs;
use std::io;
use std::path::Path;

use fdrive_core::config::Features;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub windows: WindowsConfig,
    #[serde(default)]
    pub features: Features,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowsConfig {
    pub provider_name: String,
    pub allow_pinning: bool,
    pub refresh_secs: u64,
    pub icon: Option<String>,
    pub autostart: bool,
}

impl Default for WindowsConfig {
    fn default() -> Self {
        Self {
            provider_name: "Filestash".to_string(),
            allow_pinning: true,
            refresh_secs: 10,
            icon: None,
            autostart: false,
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).map_err(io::Error::other),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod tests;

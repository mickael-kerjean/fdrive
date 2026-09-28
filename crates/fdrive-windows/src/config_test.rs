use super::AppConfig;

#[test]
fn empty_toml_is_a_valid_config() {
    let config = toml::from_str::<AppConfig>("").unwrap();
    assert_eq!(config.windows.provider_name, "Filestash");
    assert!(!config.features.autostart);
    assert!(!config.features.server_locked);
}

#[test]
fn provider_name_can_be_set() {
    let config = toml::from_str::<AppConfig>("[windows]\nprovider_name = \"Custom\"").unwrap();
    assert_eq!(config.windows.provider_name, "Custom");
}

#[test]
fn autostart_can_be_enabled() {
    let config = toml::from_str::<AppConfig>("[features]\nautostart = true").unwrap();
    assert!(config.features.autostart);
}

#[test]
fn server_can_be_locked() {
    let config = toml::from_str::<AppConfig>("[features]\nserver_locked = true").unwrap();
    assert!(config.features.server_locked);
}

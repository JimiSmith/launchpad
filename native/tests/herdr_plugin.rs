use std::process::Command;

fn manifest() -> toml::Table {
    toml::from_str(include_str!("../../herdr-plugin.toml")).unwrap()
}
/// The plugin's install script downloads the release named by its version.
#[test]
fn plugin_version_matches_the_package() {
    assert_eq!(
        manifest()["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}
#[test]
fn plugin_action_opens_the_manifest_pane_with_launchpad() {
    let manifest = manifest();
    let entry = |kind: &str, id: &str| {
        manifest[kind]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no {kind} {id}"))
            .clone()
    };
    assert_eq!(
        entry("actions", "open")["command"].as_array().unwrap(),
        &["bin/launchpad", "--herdr-plugin-open"].map(toml::Value::from)
    );
    // Launchpad opens this entrypoint id.
    assert_eq!(
        entry("panes", "launchpad")["command"].as_array().unwrap(),
        &["bin/launchpad"].map(toml::Value::from)
    );
}
#[test]
fn plugin_action_needs_herdrs_plugin_environment() {
    let result = Command::new(env!("CARGO_BIN_EXE_launchpad"))
        .env_clear()
        .arg("--herdr-plugin-open")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("HERDR_PLUGIN_ID is missing"));
}

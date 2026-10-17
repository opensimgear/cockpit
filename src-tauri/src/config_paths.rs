//! Shared application path anchor. No configuration or runtime is loaded here.
use std::path::PathBuf;

pub fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OSG_DCS_BIOS_CONFIG") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    let root = home.join("Library/Application Support/org.opensimgear.cockpit");
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or(home)
        .join("OpenSimGear/Cockpit");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("opensimgear/cockpit");
    root.join("dcs-bios.json")
}

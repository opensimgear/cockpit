//! Shared application path anchor. No configuration or runtime is loaded here.
use std::path::PathBuf;

pub fn database_path() -> PathBuf {
    std::env::var_os("OSG_SQLITE_DATABASE").map(PathBuf::from)
        .unwrap_or_else(|| config_path().with_file_name("cockpit.sqlite3"))
}

pub(crate) fn initialize(additional_roots:&[PathBuf])->Result<osg_persistence::Database,String> {
    let anchor=config_path();
    let mut roots=vec![anchor.parent().ok_or("Invalid configuration namespace.")?.to_path_buf()];
    if let Some(retained)=std::env::var_os("OSG_GAMES_CONFIG") {
        let path=PathBuf::from(retained);
        roots.push(path.parent().ok_or("Invalid retained games namespace.")?.to_path_buf());
    }
    roots.extend_from_slice(additional_roots);
    let database=database_path();
    osg_persistence::register_namespace(&roots,&database)?;
    osg_persistence::Database::open(database)
}

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

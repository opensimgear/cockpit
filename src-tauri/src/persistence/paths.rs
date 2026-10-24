//! Resolve all native application roots to one SQLite authority before workers start.
use std::{ffi::OsString, path::PathBuf};
type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;
fn system_environment(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}
pub fn database_path() -> PathBuf {
    let anchor = config_path();
    database_path_in(&system_environment, &anchor)
}
pub fn config_path() -> PathBuf {
    config_path_in(&system_environment)
}
pub(crate) fn initialize(
    additional_roots: &[PathBuf],
) -> Result<osg_persistence::Database, String> {
    initialize_in(additional_roots, &system_environment)
}
fn database_path_in(environment: Environment<'_>, anchor: &std::path::Path) -> PathBuf {
    environment("OSG_SQLITE_DATABASE")
        .map(PathBuf::from)
        .unwrap_or_else(|| anchor.with_file_name("cockpit.sqlite3"))
}
fn initialize_in(
    additional_roots: &[PathBuf],
    environment: Environment<'_>,
) -> Result<osg_persistence::Database, String> {
    let anchor = config_path_in(environment);
    let mut roots = vec![anchor
        .parent()
        .ok_or("Invalid configuration namespace.")?
        .to_path_buf()];
    if let Some(games) = environment("OSG_GAMES_CONFIG") {
        let path = PathBuf::from(games);
        roots.push(
            path.parent()
                .ok_or("Invalid games namespace.")?
                .to_path_buf(),
        );
    }
    roots.extend_from_slice(additional_roots);
    let database = database_path_in(environment, &anchor);
    osg_persistence::register_namespace(&roots, &database)?;
    osg_persistence::Database::open(database)
}
fn config_path_in(environment: Environment<'_>) -> PathBuf {
    if let Some(path) = environment("OSG_DCS_BIOS_CONFIG") {
        return PathBuf::from(path);
    }
    let home = environment("HOME").map(PathBuf::from).unwrap_or_default();
    #[cfg(target_os = "macos")]
    let root = home.join("Library/Application Support/org.opensimgear.cockpit");
    #[cfg(target_os = "windows")]
    let root = environment("APPDATA")
        .map(PathBuf::from)
        .unwrap_or(home)
        .join("OpenSimGear/Cockpit");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = environment("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("opensimgear/cockpit");
    root.join("dcs-bios.json")
}
#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;

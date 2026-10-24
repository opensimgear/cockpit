use super::*;
use std::collections::BTreeMap;
#[test]
fn platform_defaults_and_explicit_namespaces_are_resolved_without_changing_environment() {
    let dir = crate::test_directory();
    let home = dir.path().join("home");
    let settings = dir.path().join("settings");
    let mut values = BTreeMap::<&str, OsString>::from([("HOME", home.clone().into_os_string())]);
    let default = config_path_in(&|name| values.get(name).cloned());
    #[cfg(windows)]
    assert_eq!(default, home.join("OpenSimGear/Cockpit/dcs-bios.json"));
    #[cfg(target_os = "macos")]
    assert_eq!(
        default,
        home.join("Library/Application Support/org.opensimgear.cockpit/dcs-bios.json")
    );
    #[cfg(not(any(windows, target_os = "macos")))]
    assert_eq!(
        default,
        home.join(".config/opensimgear/cockpit/dcs-bios.json")
    );
    values.insert("APPDATA", settings.clone().into_os_string());
    values.insert("XDG_CONFIG_HOME", settings.clone().into_os_string());
    let configured = config_path_in(&|name| values.get(name).cloned());
    #[cfg(windows)]
    assert_eq!(
        configured,
        settings.join("OpenSimGear/Cockpit/dcs-bios.json")
    );
    #[cfg(not(any(windows, target_os = "macos")))]
    assert_eq!(
        configured,
        settings.join("opensimgear/cockpit/dcs-bios.json")
    );
    let explicit = dir.path().join("isolated/dcs.json");
    values.insert("OSG_DCS_BIOS_CONFIG", explicit.clone().into_os_string());
    assert_eq!(config_path_in(&|name| values.get(name).cloned()), explicit);
    assert_eq!(
        database_path_in(&|name| values.get(name).cloned(), &explicit),
        explicit.with_file_name("cockpit.sqlite3")
    );
    assert!(!dir.path().join("home").exists());
}
#[test]
fn native_desktop_and_configured_game_roots_share_the_requested_database() {
    let dir = crate::test_directory();
    let native = dir.path().join("native/dcs.json");
    let games = dir.path().join("games/config.json");
    let desktop = dir.path().join("desktop");
    let database = dir.path().join("database/cockpit.sqlite3");
    let values = BTreeMap::<&str, OsString>::from([
        ("OSG_DCS_BIOS_CONFIG", native.clone().into_os_string()),
        ("OSG_GAMES_CONFIG", games.clone().into_os_string()),
        ("OSG_SQLITE_DATABASE", database.clone().into_os_string()),
    ]);
    let initialized = initialize_in(&[desktop.clone()], &|name| values.get(name).cloned()).unwrap();
    assert_eq!(initialized.path(), database);
    initialized.integrity_check().unwrap();
    for anchor in [native, games, desktop.join("preferences.json")] {
        assert_eq!(
            osg_persistence::Database::for_namespace(&anchor)
                .unwrap()
                .path(),
            database
        );
        assert!(!anchor.exists());
    }
    assert!(initialize_in(&[], &|name| (name == "OSG_DCS_BIOS_CONFIG")
        .then(|| "relative.json".into()))
    .is_err());
    let other = dir.path().join("other/dcs.json");
    initialize_in(&[], &|name| {
        (name == "OSG_DCS_BIOS_CONFIG").then(|| other.clone().into_os_string())
    })
    .unwrap();
}

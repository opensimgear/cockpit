use super::*;
use std::path::Path;

fn record(
    client: &'static str,
    key_id: &str,
    path: &Path,
    name: &str,
    path_key: &str,
) -> RegistryEntry {
    RegistryEntry {
        client,
        key_id: key_id.into(),
        values: BTreeMap::from([
            (path_key.into(), path.to_string_lossy().into_owned()),
            ("DisplayName".into(), name.into()),
        ]),
    }
}

#[test]
fn converts_ea_ubisoft_and_gog_registry_layouts() {
    let temp = tempfile::tempdir().unwrap();
    let ea = temp.path().join("EA Game");
    let ubi = temp.path().join("Ubisoft Game");
    let gog = temp.path().join("GOG Game");
    for path in [&ea, &ubi, &gog] {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::write(ea.join("Game.exe"), b"fixture").unwrap();
    let candidates = convert([
        record("ea", "Apex Legends", &ea, "Apex Legends", "Install Dir"),
        record("ubisoft", "18403", &ubi, "Far Cry 6", "InstallDir"),
        record("gog", "1207658693", &gog, "Cyberpunk 2077", "PATH"),
    ]);
    assert_eq!(candidates.len(), 3);
    assert!(candidates[0].app_id.starts_with("registry-"));
    assert_eq!(candidates[0].client, "ea");
    assert_eq!(candidates[0].name, "Apex Legends");
    assert_eq!(candidates[0].executable_candidates.len(), 1);
    assert_eq!(candidates[1].client, "ubisoft");
    assert_eq!(candidates[1].app_id, "18403");
    assert_eq!(candidates[2].client, "gog");
    assert_eq!(candidates[2].app_id, "1207658693");
    for candidate in candidates {
        assert_eq!(candidate.kind, "unknown");
        assert!(candidate.game_id.is_none());
        assert!(candidate.launcher.is_none());
        assert!(candidate.artwork.is_none());
        assert!(Path::new(&candidate.install_path).is_absolute());
    }
}

#[test]
fn skips_missing_stale_and_non_absolute_install_records() {
    let temp = tempfile::tempdir().unwrap();
    let stale = temp.path().join("removed");
    let relative = Path::new("Games\\Relative");
    let entries = [
        RegistryEntry {
            client: "ea",
            key_id: "1".into(),
            values: BTreeMap::from([
                ("InstallDir".into(), stale.to_string_lossy().into_owned()),
                ("Name".into(), "Stale".into()),
            ]),
        },
        RegistryEntry {
            client: "gog",
            key_id: "2".into(),
            values: BTreeMap::from([("PATH".into(), relative.to_string_lossy().into_owned())]),
        },
        RegistryEntry {
            client: "ubisoft",
            key_id: "3".into(),
            values: BTreeMap::from([("DisplayName".into(), "No location".into())]),
        },
    ];
    assert!(convert(entries).is_empty());
}

#[test]
fn de_duplicates_registry_views_and_bounds_safe_app_ids() {
    let temp = tempfile::tempdir().unwrap();
    let install = temp.path().join("One Game");
    std::fs::create_dir_all(&install).unwrap();
    let first = record("gog", "120", &install, "One Game", "PATH");
    let duplicate_view = first.clone();
    let candidates = convert([first, duplicate_view]);
    assert_eq!(candidates.len(), 1);
    assert_eq!(safe_app_id("gog", "120").as_deref(), Some("120"));
    assert_eq!(safe_app_id("ubisoft", "18403").as_deref(), Some("18403"));
    let punctuated = safe_app_id("ea", "BattleField 2042").unwrap();
    assert!(punctuated.starts_with("registry-"));
    assert_eq!(punctuated.len(), 73);
    assert_ne!(
        safe_app_id("ea", "Apex: Legends"),
        safe_app_id("ea", "Apex/Legends")
    );
    assert_eq!(
        safe_app_id("ea", "BattleField").as_deref(),
        Some("battlefield")
    );
    assert_eq!(safe_app_id("ea", &"x".repeat(200)).unwrap().len(), 73);
    assert_ne!(
        safe_app_id("ea", &"x".repeat(200)),
        safe_app_id("ea", &"y".repeat(200))
    );
    assert!(safe_app_id("ea", "雪").unwrap().starts_with("registry-"));
    assert_eq!(safe_app_id("ea", "\0").as_deref(), None);
}

#[test]
fn excludes_launcher_records_even_when_their_install_path_exists() {
    let temp = tempfile::tempdir().unwrap();
    let install = temp.path().join("launcher");
    std::fs::create_dir_all(&install).unwrap();
    for (client, id, name, key) in [
        ("ea", "ea-desktop", "EA Desktop", "InstallDir"),
        ("ea", "origin-client", "Origin Client", "Install Dir"),
        ("ubisoft", "123", "Ubisoft Connect", "InstallDir"),
        ("gog", "galaxy", "GOG Galaxy", "PATH"),
    ] {
        assert!(convert([record(client, id, &install, name, key)]).is_empty());
    }
}

#[cfg(windows)]
#[test]
fn native_registry_enumeration_reads_private_hkcu_fixture_and_ignores_stale_values() {
    use super::{convert, native};
    use std::{ffi::c_void, ptr};

    type HKey = native::HKey;
    const HKCU: HKey = 0x8000_0001u32 as i32 as isize as HKey;
    const KEY_ALL_ACCESS: u32 = 0xF003F;
    const TEST_VIEW: u32 = native::KEY_WOW64_64KEY;
    const REG_SZ: u32 = 1;
    const SUCCESS: i32 = 0;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegOpenKeyExW(
            key: HKey,
            subkey: *const u16,
            options: u32,
            access: u32,
            result: *mut HKey,
        ) -> i32;
        fn RegCreateKeyExW(
            key: HKey,
            subkey: *const u16,
            reserved: u32,
            class: *mut u16,
            options: u32,
            access: u32,
            security: *mut c_void,
            result: *mut HKey,
            disposition: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            key: HKey,
            name: *const u16,
            reserved: u32,
            kind: u32,
            data: *const u8,
            bytes: u32,
        ) -> i32;
        fn RegCloseKey(key: HKey) -> i32;
        fn RegDeleteTreeW(key: HKey, subkey: *const u16) -> i32;
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    struct Cleanup(HKey, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let parent_path = wide("Software");
            let leaf = wide(self.1.rsplit('\\').next().unwrap_or(&self.1));
            let mut parent = ptr::null_mut();
            unsafe {
                if RegOpenKeyExW(
                    self.0,
                    parent_path.as_ptr(),
                    0,
                    KEY_ALL_ACCESS | TEST_VIEW,
                    &mut parent,
                ) == SUCCESS
                {
                    RegDeleteTreeW(parent, leaf.as_ptr());
                    RegCloseKey(parent);
                }
            }
        }
    }
    struct OwnedKey(HKey);
    impl Drop for OwnedKey {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    fn write_record(root: &str, id: &str, values: &[(&str, &str)]) {
        let key_path = wide(&format!("{root}\\{id}"));
        let mut key = ptr::null_mut();
        let opened = unsafe {
            RegCreateKeyExW(
                HKCU,
                key_path.as_ptr(),
                0,
                ptr::null_mut(),
                0,
                KEY_ALL_ACCESS | TEST_VIEW,
                ptr::null_mut(),
                &mut key,
                ptr::null_mut(),
            )
        };
        assert_eq!(
            opened, SUCCESS,
            "failed to create private registry fixture key"
        );
        let key = OwnedKey(key);
        for (name, value) in values {
            let name = wide(name);
            let value = wide(value);
            let bytes = (value.len() * std::mem::size_of::<u16>()) as u32;
            let result = unsafe {
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    bytes,
                )
            };
            assert_eq!(
                result, SUCCESS,
                "failed to set private registry fixture value"
            );
        }
    }

    let root = format!(
        "Software\\OpenSimGearGameLibraryTest-{}",
        uuid::Uuid::new_v4()
    );
    // Created before any registry writes so panic/assertion unwinding also removes it.
    let _cleanup = Cleanup(HKCU, root.clone());
    let temp = tempfile::tempdir().unwrap();
    let install = temp.path().join("installed");
    std::fs::create_dir_all(&install).unwrap();
    let install_text = install.to_string_lossy().into_owned();
    let stale_text = temp.path().join("removed").to_string_lossy().into_owned();
    write_record(
        &root,
        "FixtureGame",
        &[
            ("DisplayName", "Fixture Game"),
            ("InstallDir", &install_text),
            ("PATH", &install_text),
        ],
    );
    write_record(
        &root,
        "StaleGame",
        &[
            ("DisplayName", "Stale Game"),
            ("InstallDir", &stale_text),
            ("PATH", &stale_text),
        ],
    );
    write_record(
        &root,
        "EmptyPath",
        &[
            ("DisplayName", "Empty Path"),
            ("InstallDir", ""),
            ("PATH", ""),
        ],
    );
    write_record(&root, "MissingPath", &[("DisplayName", "Missing Path")]);

    let entries = native::entries_at(HKCU, &root, native::KEY_WOW64_64KEY, "ea");
    assert_eq!(
        entries.len(),
        4,
        "native enumeration should return each child key"
    );
    let candidates = convert(entries);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].app_id, "fixturegame");
    assert_eq!(candidates[0].name, "Fixture Game");
    assert_eq!(
        candidates[0].install_path,
        std::fs::canonicalize(&install)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    );
    for client in ["ubisoft", "gog"] {
        let candidates = convert(native::entries_at(
            HKCU,
            &root,
            native::KEY_WOW64_64KEY,
            client,
        ));
        assert_eq!(candidates.len(), 1, "native {client} values should convert");
        assert_eq!(candidates[0].client, client);
    }
}

use super::*;
use std::path::PathBuf;

struct Installation(tempfile::TempDir);
impl Installation {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("content/cars")).unwrap();
        Self(root)
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn car(&self, id: &str, name: &str) -> PathBuf {
        self.write(
            &format!("content/cars/{id}/ui/ui_car.json"),
            json!({"name":name}).to_string().as_bytes(),
        );
        self.write(
            &format!("content/cars/{id}/data.acd"),
            b"authored archive placeholder; not opened by discovery",
        );
        self.write(
            &format!("content/cars/{id}/skins/default/livery.dds"),
            b"authored skin placeholder",
        );
        self.root().join("content/cars").join(id)
    }
}

#[test]
fn installed_mods_and_variants_keep_exact_ids_and_sort_independently_of_creation_order() {
    let installation = Installation::new();
    installation.car("ks_example_s1", "Example Step 1");
    installation.car("ks_example", "Example");
    installation.car("Mod_é", "Mód ");
    assert_eq!(
        scan(installation.root()).unwrap(),
        json!([
            {"id":"Mod_é","name":"Mód"},
            {"id":"ks_example","name":"Example"},
            {"id":"ks_example_s1","name":"Example Step 1"}
        ])
        .as_array()
        .unwrap()
        .clone()
    );
    assert_eq!(
        scan(installation.root()).unwrap(),
        scan(installation.root()).unwrap()
    );
}

#[test]
fn raw_description_controls_do_not_damage_escaped_quotes_backslashes_or_unicode() {
    let source = format!(
        r#"{{"name":"Mód \"é\" \\ drift \uD83D\uDE97","description":"first{}last","path":"C:\\Cars\\é"}}"#,
        "\n\t\u{0001}"
    );
    let value = parse_ui(source.as_bytes()).unwrap();
    assert_eq!(value["name"], "Mód \"é\" \\ drift 🚗");
    assert_eq!(value["description"], "first\n\t\u{0001}last");
    assert_eq!(value["path"], "C:\\Cars\\é");
    let strict = br#"{"name":"Strict \\ path \"quoted\"","description":"escaped\nline\tend"}"#;
    assert_eq!(
        parse_ui(strict).unwrap(),
        serde_json::from_slice::<Value>(strict).unwrap()
    );
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(strict);
    assert_eq!(parse_ui(&bom), parse_ui(strict));
}

#[test]
fn invalid_json_escapes_utf8_and_unescaped_quote_errors_remain_invalid() {
    for input in [
        b"{\"name\":\"Bad\\\nline\"}".as_slice(),
        b"{\"name\":\"Bad\\q\",\"description\":\"raw\nline\"}".as_slice(),
        b"{\"name\":\"Bad\"quote\",\"description\":\"raw\nline\"}".as_slice(),
        b"{\"name\":\"bad\xff\"}".as_slice(),
        b"{\"name\":\"unterminated".as_slice(),
    ] {
        assert!(parse_ui(input).is_none(), "{input:?}");
    }
}

#[test]
fn incomplete_empty_and_malformed_mods_do_not_hide_complete_cars() {
    let installation = Installation::new();
    installation.car("valid", "Usable installed car");
    installation.car("NONE", "Reserved telemetry sentinel");
    installation.car(" leading_space", "Noncanonical telemetry identifier");
    fs::create_dir_all(installation.root().join("content/cars/empty_placeholder")).unwrap();
    let missing = installation.car("missing_ui", "Missing");
    fs::remove_file(missing.join("ui/ui_car.json")).unwrap();
    let broken = installation.car("broken", "Broken");
    fs::write(broken.join("ui/ui_car.json"), b"{malformed}").unwrap();
    let unnamed = installation.car("unnamed", "");
    fs::write(unnamed.join("ui/ui_car.json"), b"{\"name\":123}").unwrap();
    let control = installation.car("control_name", "Bad\nname");
    assert!(control.exists());
    let empty_data = installation.car("empty_data", "Empty data");
    fs::write(empty_data.join("data.acd"), b"").unwrap();
    let empty_skins = installation.car("empty_skins", "Empty skins");
    fs::remove_file(empty_skins.join("skins/default/livery.dds")).unwrap();
    let loose = installation.car("loose_mod", "Loose data mod");
    fs::remove_file(loose.join("data.acd")).unwrap();
    installation.write(
        "content/cars/loose_mod/data/car.ini",
        b"[HEADER]\nVERSION=1\n",
    );
    let result = scan(installation.root()).unwrap();
    assert_eq!(
        result,
        vec![
            json!({"id":"loose_mod","name":"Loose data mod"}),
            json!({"id":"valid","name":"Usable installed car"})
        ]
    );
}

#[test]
fn oversized_ui_is_skipped_but_aggregate_size_and_vehicle_limits_are_explicit() {
    let installation = Installation::new();
    installation.car("a", "A");
    installation.car("b", "B");
    let size = fs::metadata(installation.root().join("content/cars/a/ui/ui_car.json"))
        .unwrap()
        .len() as usize;
    assert_eq!(
        scan_with_limits(
            installation.root(),
            Limits {
                ui_bytes: size,
                ..LIMITS
            }
        )
        .unwrap()
        .len(),
        2
    );
    installation.write(
        "content/cars/b/ui/ui_car.json",
        json!({"name":"Too long"}).to_string().as_bytes(),
    );
    assert_eq!(
        scan_with_limits(
            installation.root(),
            Limits {
                ui_bytes: size,
                ..LIMITS
            }
        )
        .unwrap(),
        vec![json!({"id":"a","name":"A"})]
    );
    installation.write(
        "content/cars/b/ui/ui_car.json",
        json!({"name":"B"}).to_string().as_bytes(),
    );
    assert!(scan_with_limits(
        installation.root(),
        Limits {
            vehicles: 1,
            ..LIMITS
        }
    )
    .unwrap_err()
    .contains("car catalogue"));
    assert_eq!(
        scan_with_limits(
            installation.root(),
            Limits {
                total_ui_bytes: size * 2,
                ..LIMITS
            }
        )
        .unwrap()
        .len(),
        2
    );
    assert!(scan_with_limits(
        installation.root(),
        Limits {
            total_ui_bytes: size * 2 - 1,
            ..LIMITS
        }
    )
    .unwrap_err()
    .contains("metadata"));
}

#[test]
fn directory_work_is_bounded_and_empty_catalogues_are_valid() {
    let installation = Installation::new();
    assert!(scan(installation.root()).unwrap().is_empty());
    installation.car("a", "A");
    installation.car("b", "B");
    assert!(scan_with_limits(
        installation.root(),
        Limits {
            car_entries: 1,
            ..LIMITS
        }
    )
    .is_err());
    assert!(scan_with_limits(
        installation.root(),
        Limits {
            scan_entries: 2,
            ..LIMITS
        }
    )
    .unwrap_err()
    .contains("directory scan"));
    assert!(scan(Path::new("relative")).is_err());
    assert!(scan(&installation.root().join("missing")).is_err());
}

#[cfg(any(unix, windows))]
fn symlink_directory(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link)
    }
}
#[cfg(any(unix, windows))]
#[test]
fn redirected_car_and_descendant_folders_are_not_followed() {
    let installation = Installation::new();
    let outside = Installation::new();
    let external_car = outside.car("outside", "External car");
    let linked = installation.root().join("content/cars/linked_car");
    if let Err(error) = symlink_directory(&external_car, &linked) {
        #[cfg(windows)]
        if error.raw_os_error() == Some(1314) {
            return;
        }
        panic!("Cannot create test symlink: {error}");
    }
    let linked_ui = installation.car("linked_ui", "Internal label");
    fs::remove_dir_all(linked_ui.join("ui")).unwrap();
    symlink_directory(&external_car.join("ui"), &linked_ui.join("ui")).unwrap();
    let linked_skins = installation.car("linked_skins", "Internal skins");
    fs::remove_dir_all(linked_skins.join("skins")).unwrap();
    symlink_directory(&external_car.join("skins"), &linked_skins.join("skins")).unwrap();
    installation.car("internal", "Internal car");
    assert_eq!(
        scan(installation.root()).unwrap(),
        vec![json!({"id":"internal","name":"Internal car"})]
    );
    let redirected = Installation::new();
    fs::remove_dir(redirected.root().join("content/cars")).unwrap();
    symlink_directory(
        &outside.root().join("content/cars"),
        &redirected.root().join("content/cars"),
    )
    .unwrap();
    assert!(scan(redirected.root()).is_err());
}

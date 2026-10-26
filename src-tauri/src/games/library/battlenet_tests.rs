use super::*;

fn varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 0x80 {
        bytes.push((value as u8) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}
fn field(number: u8, value: &[u8]) -> Vec<u8> {
    let mut bytes = vec![(number << 3) | 2];
    bytes.extend(varint(value.len() as u64));
    bytes.extend(value);
    bytes
}
fn scalar(number: u8, value: u8) -> Vec<u8> {
    vec![number << 3, value]
}
fn text_field(number: u8, value: &str) -> Vec<u8> {
    field(number, value.as_bytes())
}

/// Lutris' product_db.py schema: ProductDb.product_installs=1; ProductInstall
/// uid=1, product_code=2, settings=3, cached_product_state=4; the nested
/// installed signal is CachedProductState.base_product_state=1, installed=1.
fn product(uid: &str, code: &str, install: &str, installed: bool) -> Vec<u8> {
    let mut settings = text_field(1, install);
    settings.extend(text_field(2, "US"));
    let state = scalar(1, u8::from(installed));
    let cached = field(1, &state);
    let mut record = text_field(1, uid);
    record.extend(text_field(2, code));
    record.extend(field(3, &settings));
    record.extend(field(4, &cached));
    field(1, &record)
}

#[test]
fn discovers_installed_known_product_and_skips_launcher_uninstalled_stale_and_duplicate_records() {
    let temp = crate::test_directory();
    let root = temp.path().join("World of Warcraft");
    fs::create_dir_all(root.join("_retail_")).unwrap();
    fs::write(root.join("_retail_/Wow.exe"), b"exe").unwrap();
    let missing = temp.path().join("removed-game");
    let mut db = product("uid-wow", "wow", root.to_str().unwrap(), true);
    db.extend(product(
        "uid-wow-duplicate",
        "wow",
        root.to_str().unwrap(),
        true,
    ));
    db.extend(product(
        "uid-old",
        "fenris",
        missing.to_str().unwrap(),
        true,
    ));
    let diablo = temp.path().join("Diablo IV");
    fs::create_dir_all(&diablo).unwrap();
    db.extend(product(
        "uid-fenris-valid",
        "fenris",
        diablo.to_str().unwrap(),
        true,
    ));
    db.extend(product("uid-pending", "s2", root.to_str().unwrap(), false));
    db.extend(product("agent", "agent", root.to_str().unwrap(), true));
    let file = temp.path().join("product.db");
    fs::write(&file, db).unwrap();

    let mut scan = Scan::default();
    scan_paths(&[file], &mut scan);
    assert_eq!(scan.candidates.len(), 2);
    assert!(scan
        .candidates
        .iter()
        .any(|candidate| { candidate.id == "battlenet:fenris" && candidate.name == "Diablo IV" }));
    let game = &scan.candidates[0];
    assert_eq!(game.id, "battlenet:wow");
    assert_eq!(game.app_id, "wow");
    assert_eq!(game.client, "battlenet");
    assert_eq!(game.name, "World of Warcraft");
    assert_eq!(game.kind, "game");
    assert_eq!(game.game_id, None);
    assert_eq!(game.launcher, None);
    assert!(game.artwork.is_none());
    assert_eq!(game.executable_candidates.len(), 1);
    assert!(game.executable_candidates[0].ends_with("Wow.exe"));
}

#[test]
fn unknown_product_falls_back_to_real_install_directory_name() {
    let temp = crate::test_directory();
    let root = temp.path().join("My New Blizzard Game");
    fs::create_dir_all(&root).unwrap();
    let file = temp.path().join("product.db");
    fs::write(
        &file,
        product("u1", "newProd", root.to_str().unwrap(), true),
    )
    .unwrap();
    let mut scan = Scan::default();
    scan_paths(&[file], &mut scan);
    assert_eq!(scan.candidates[0].name, "My New Blizzard Game");
    assert_eq!(scan.candidates[0].kind, "unknown");
    assert_eq!(scan.candidates[0].app_id, "newProd");
}

#[test]
fn malformed_truncated_and_oversize_databases_are_rejected_with_diagnostics() {
    let temp = crate::test_directory();
    let truncated = temp.path().join("truncated.db");
    fs::write(&truncated, [0x0a, 0x05, 0x01]).unwrap();
    let oversized = temp.path().join("large.db");
    fs::write(&oversized, vec![0; MAX_DATABASE_BYTES as usize + 1]).unwrap();
    let mut scan = Scan::default();
    scan_paths(&[truncated, oversized], &mut scan);
    assert!(scan.candidates.is_empty());
    assert_eq!(scan.diagnostics.len(), 2);
    assert!(scan
        .diagnostics
        .iter()
        .any(|line| line.contains("malformed")));
    assert!(scan
        .diagnostics
        .iter()
        .any(|line| line.contains("could not be read")));
}

#[test]
fn protobuf_decoder_skips_unknown_fields_and_rejects_bad_wire_data() {
    let mut db = product("u", "s1", "C:\\Games\\StarCraft", true);
    db.extend(field(7, b"unknown extension"));
    assert!(decode_database(&db).is_some());
    assert!(decode_database(&[0x0a, 0x80]).is_none());
    assert!(decode_database(&[0x0b, 0x00]).is_none());
}

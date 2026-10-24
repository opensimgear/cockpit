use super::*;
fn write(database: &Database, domain: &str, key: &str, value: &[u8]) {
    database
        .commit(&[Change {
            domain,
            key,
            expected_revision: database.revision(domain, key).unwrap(),
            payload: Some(value),
        }])
        .unwrap();
}
#[test]
fn restore_rotates_identity_invalidates_handles_and_discards_historical_authority() {
    let dir = crate::test_directory();
    let path = dir.path().join("cockpit.sqlite3");
    let database = Database::open(&path).unwrap();
    let lease = NamespaceLease::acquire(&path).unwrap();
    assert!(NamespaceLease::acquire(&path).is_err());
    for domain in [
        "secrets",
        "security",
        "companion-client",
        "recovery-profile",
        "recovery-output",
        "recovery-desktop",
        "recovery-visibility",
        "recovery-dcs",
    ] {
        write(&database, domain, "historical", b"old authority");
    }
    for (domain, key) in [
        ("profiles", "selection"),
        ("companion", "listener"),
        ("companion", "firmware"),
    ] {
        write(&database, domain, key, b"old intent");
    }
    write(&database, "ui", "theme", b"true");
    write(&database, "widgets", "library", b"saved content");
    let backup = dir.path().join("backup.sqlite3");
    database.backup(&backup).unwrap();
    write(&database, "ui", "theme", b"false");
    write(&database, "storage", "backups", b"current index");
    let before = database.epoch().to_owned();
    let restored = database
        .restore_from(&backup, &snapshot_hash(&backup).unwrap(), &lease)
        .unwrap();
    assert_ne!(restored.epoch(), before);
    assert!(database
        .read("ui", "theme")
        .unwrap_err()
        .contains("restored or replaced"));
    assert!(database
        .commit(&[Change {
            domain: "ui",
            key: "theme",
            expected_revision: 1,
            payload: Some(b"false")
        }])
        .is_err());
    assert_eq!(
        restored.read("ui", "theme").unwrap().unwrap().payload,
        b"true"
    );
    assert_eq!(
        restored
            .read("widgets", "library")
            .unwrap()
            .unwrap()
            .payload,
        b"saved content"
    );
    for domain in [
        "secrets",
        "security",
        "companion-client",
        "recovery-profile",
        "recovery-output",
        "recovery-desktop",
        "recovery-visibility",
        "recovery-dcs",
    ] {
        assert!(restored.read(domain, "historical").unwrap().is_none());
    }
    for (domain, key) in [
        ("profiles", "selection"),
        ("companion", "listener"),
        ("companion", "firmware"),
    ] {
        assert!(restored.read(domain, key).unwrap().is_none());
    }
    restored.integrity_check().unwrap();
    assert_eq!(
        restored
            .read("storage", "backups")
            .unwrap()
            .unwrap()
            .payload,
        b"current index"
    );
    assert!(Database::open_existing(&backup)
        .unwrap()
        .read("secrets", "historical")
        .unwrap()
        .is_some());
}
#[test]
fn restore_busy_foreign_corrupt_missing_and_wrong_namespace_preserve_current_data() {
    let dir = crate::test_directory();
    let database = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    let lease = NamespaceLease::acquire(database.path()).unwrap();
    write(&database, "ui", "theme", b"true");
    let backup = dir.path().join("backup.sqlite3");
    database.backup(&backup).unwrap();
    let epoch = database.epoch().to_owned();
    assert!(database
        .restore_from(&backup, "wrong hash", &lease)
        .is_err());
    let connection = database.connection().unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(database
        .restore_from(&backup, &snapshot_hash(&backup).unwrap(), &lease)
        .unwrap_err()
        .contains("busy"));
    connection.execute_batch("ROLLBACK").unwrap();
    let other = NamespaceLease::acquire(&dir.path().join("other.sqlite3")).unwrap();
    assert!(database
        .restore_from(&backup, &snapshot_hash(&backup).unwrap(), &other)
        .is_err());
    assert!(database.restore_from(database.path(), "", &lease).is_err());
    let bad = dir.path().join("bad.sqlite3");
    std::fs::write(&bad, b"not sqlite").unwrap();
    assert!(database.restore_from(&bad, "", &lease).is_err());
    let empty = dir.path().join("empty.sqlite3");
    std::fs::write(&empty, []).unwrap();
    assert!(database.restore_from(&empty, "", &lease).is_err());
    let missing = dir.path().join("missing.sqlite3");
    assert!(database.restore_from(&missing, "", &lease).is_err());
    assert!(!missing.exists());
    assert_eq!(database.epoch(), epoch);
    assert_eq!(
        database.read("ui", "theme").unwrap().unwrap().payload,
        b"true"
    );
}
#[test]
fn pinned_handles_do_not_recreate_deleted_databases_or_accept_old_layouts() {
    let dir = crate::test_directory();
    let path = dir.path().join("cockpit.sqlite3");
    let database = Database::open(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(database.read("ui", "theme").is_err());
    assert!(!path.exists());
    let replacement = Database::open(&path).unwrap();
    assert_ne!(replacement.epoch(), database.epoch());
    assert!(database.read("ui", "theme").is_err());
    let connection = replacement.connection().unwrap();
    connection.execute_batch("DROP TABLE incarnation").unwrap();
    drop(connection);
    assert!(Database::open(&path)
        .unwrap_err()
        .contains("fresh database"));
}
#[test]
fn registered_namespace_never_repins_an_old_process_after_restore() {
    let dir = crate::test_directory();
    let path = dir.path().join("cockpit.sqlite3");
    let native = dir.path().join("native");
    let anchor = native.join("profiles-v2/library.json");
    crate::register_namespace(&[native], &path).unwrap();
    let database = Database::for_namespace(&anchor).unwrap();
    let backup = dir.path().join("backup.sqlite3");
    database.backup(&backup).unwrap();
    let lease = NamespaceLease::acquire(&path).unwrap();
    database
        .restore_from(&backup, &snapshot_hash(&backup).unwrap(), &lease)
        .unwrap();
    assert!(Database::for_namespace(&anchor).is_err());
    assert!(Database::existing_for_namespace(&anchor).is_err());
}

#[test]
fn fresh_native_host_ciphertext_commits_with_restored_public_content() {
    let dir = crate::test_directory();
    let database = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    let lease = NamespaceLease::acquire(database.path()).unwrap();
    write(
        &database,
        "companion",
        "publications",
        b"validated archived panels",
    );
    write(
        &database,
        "secrets",
        "companion-host",
        b"old protected authority",
    );
    let backup = dir.path().join("backup.sqlite3");
    database.backup(&backup).unwrap();
    let hash = snapshot_hash(&backup).unwrap();
    assert!(database
        .restore_with_fresh_host(&backup, &hash, &lease, Some(&[]))
        .is_err());
    let restored = database
        .restore_with_fresh_host(
            &backup,
            &hash,
            &lease,
            Some(b"fresh native sealed empty identity"),
        )
        .unwrap();
    assert_eq!(
        restored
            .read("secrets", "companion-host")
            .unwrap()
            .unwrap()
            .payload,
        b"fresh native sealed empty identity"
    );
    assert_eq!(
        restored
            .read("companion", "publications")
            .unwrap()
            .unwrap()
            .payload,
        b"validated archived panels"
    );
    assert!(database.read("secrets", "companion-host").is_err());
}

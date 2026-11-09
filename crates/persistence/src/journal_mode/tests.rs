use super::*;
use crate::{snapshot_hash, Change, Database, NamespaceLease};

#[test]
fn local_primary_pins_wal_full_and_keeps_snapshots_portable() {
    let directory = crate::test_directory();
    let path = directory.path().join("cockpit.sqlite3");
    let archive = directory.path().join("backup.sqlite3");
    let mut lease = NamespaceLease::acquire(&path).unwrap();
    let primary = lease.open_database().unwrap();
    assert_eq!(primary.journal_mode(), "wal");
    assert!(header(&path).unwrap() == Header::Wal);
    let connection = primary.connection().unwrap();
    let synchronous: i64 = connection
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .unwrap();
    assert_eq!(synchronous, 2);
    drop(connection);
    assert!(path.with_file_name("cockpit.sqlite3-wal").is_file());
    assert!(path.with_file_name("cockpit.sqlite3-shm").is_file());
    assert!(Database::open_existing(&path).is_err());
    primary
        .commit(&[Change {
            domain: "fixture",
            key: "retained",
            expected_revision: 0,
            payload: Some(b"before"),
        }])
        .unwrap();
    primary.backup(&archive).unwrap();
    require_snapshot(&archive).unwrap();
    let snapshot = Database::open_existing(&archive).unwrap();
    assert_eq!(snapshot.journal_mode(), "delete");
    assert_eq!(
        snapshot
            .read("fixture", "retained")
            .unwrap()
            .unwrap()
            .payload,
        b"before"
    );
    let digest = snapshot_hash(&archive).unwrap();
    primary
        .commit(&[Change {
            domain: "fixture",
            key: "retained",
            expected_revision: 1,
            payload: Some(b"after"),
        }])
        .unwrap();
    let restored = primary.restore_from(&archive, &digest, &lease).unwrap();
    assert_eq!(restored.journal_mode(), "wal");
    assert_ne!(restored.epoch(), primary.epoch());
    assert!(primary.read("fixture", "retained").is_err());
    assert_eq!(
        restored
            .read("fixture", "retained")
            .unwrap()
            .unwrap()
            .payload,
        b"before"
    );
    crate::register_database_namespace(&[directory.path().into()], &restored).unwrap();
    assert_eq!(
        Database::for_namespace(&directory.path().join("anchor.json"))
            .unwrap()
            .epoch(),
        restored.epoch()
    );
    assert!(crate::register_database_namespace(&[directory.path().into()], &primary).is_err());
    assert_eq!(lease.open_database().unwrap().epoch(), restored.epoch());
}

#[test]
fn unresolved_sidecars_and_hardlinked_files_do_not_promote() {
    let directory = crate::test_directory();
    let path = directory.path().join("cockpit.sqlite3");
    let standalone = Database::open(&path).unwrap();
    let pending = directory.path().join("cockpit.sqlite3-journal");
    std::fs::write(&pending, b"retained rollback evidence").unwrap();
    assert_eq!(select(&path).unwrap(), (JournalMode::Delete, false));
    assert_eq!(
        std::fs::read(&pending).unwrap(),
        b"retained rollback evidence"
    );
    std::fs::remove_file(&pending).unwrap();
    let alias = directory.path().join("alias.sqlite3");
    std::fs::hard_link(&path, alias).unwrap();
    let mut lease = NamespaceLease::acquire(&path).unwrap();
    assert_eq!(lease.open_database().unwrap().journal_mode(), "delete");
    assert_eq!(standalone.journal_mode(), "delete");
}

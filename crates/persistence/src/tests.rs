use super::*;

fn setup() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    (dir, db)
}
fn change<'a>(key: &'a str, expected: u64, bytes: Option<&'a [u8]>) -> Change<'a> {
    Change {
        domain: "settings",
        key,
        format_version: 1,
        expected_revision: expected,
        payload: bytes,
    }
}

#[test]
fn commit_reopen_revisions_and_delete_tombstones() {
    let (_dir, db) = setup();
    assert_eq!(db.revision("settings", "theme").unwrap(), 0);
    assert!(db.read("settings", "theme").unwrap().is_none());
    assert_eq!(
        db.commit(&[change("theme", 0, Some(b"true"))]).unwrap(),
        [1]
    );
    let reopened = Database::open(db.path()).unwrap();
    assert_eq!(
        reopened.read("settings", "theme").unwrap().unwrap().payload,
        b"true"
    );
    assert_eq!(reopened.commit(&[change("theme", 1, None)]).unwrap(), [2]);
    assert!(db.read("settings", "theme").unwrap().is_none());
    assert_eq!(db.revision("settings", "theme").unwrap(), 2);
    assert!(db
        .commit(&[change("theme", 0, Some(b"false"))])
        .unwrap_err()
        .contains("conflict"));
    db.commit(&[change("theme", 2, Some(b"false"))]).unwrap();
    db.integrity_check().unwrap();
}

#[test]
fn related_edits_rollback_on_any_conflict_or_invalid_change() {
    let (_dir, db) = setup();
    db.commit(&[change("a", 0, Some(b"old"))]).unwrap();
    for changes in [
        vec![change("b", 0, Some(b"new")), change("a", 0, Some(b"stale"))],
        vec![
            change("b", 0, Some(b"new")),
            change("b", 0, Some(b"duplicate")),
        ],
        vec![change("b", 0, Some(b"new")), change("", 0, None)],
    ] {
        assert!(db.commit(&changes).is_err());
        assert!(db.read("settings", "b").unwrap().is_none());
        assert_eq!(db.read("settings", "a").unwrap().unwrap().payload, b"old");
    }
    assert!(db.commit(&[]).is_err());
    let mut invalid = change("b", 0, None);
    invalid.format_version = 0;
    assert!(db.commit(&[invalid]).is_err());
    assert!(db.read("", "a").is_err());
    assert!(db.revision("settings", "\n").is_err());
}

#[test]
fn validated_import_is_atomic_resumable_and_cannot_overwrite_later_edits() {
    let (_dir, db) = setup();
    let source = Import {
        source: "theme",
        domain: "settings",
        key: "theme",
        format_version: 1,
        payload: Some(b"old"),
    };
    assert!(!db.origin_imported("desktop-dev").unwrap());
    db.import("desktop-dev", &[source]).unwrap();
    assert!(db.origin_imported("desktop-dev").unwrap());
    db.commit(&[change("theme", 1, Some(b"new"))]).unwrap();
    db.import(
        "desktop-dev",
        &[Import {
            source: "theme",
            domain: "settings",
            key: "theme",
            format_version: 1,
            payload: Some(b"stale"),
        }],
    )
    .unwrap();
    db.import(
        "desktop-packaged",
        &[Import {
            source: "theme",
            domain: "settings",
            key: "theme",
            format_version: 1,
            payload: Some(b"stale"),
        }],
    )
    .unwrap();
    assert_eq!(
        db.read("settings", "theme").unwrap().unwrap().payload,
        b"new"
    );
    let snapshot: Vec<u8> = db
        .connection()
        .unwrap()
        .query_row(
            "SELECT snapshot FROM imports WHERE origin='desktop-dev'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(snapshot, b"old");
    db.import(
        "missing",
        &[Import {
            source: "absent",
            domain: "settings",
            key: "absent",
            format_version: 1,
            payload: None,
        }],
    )
    .unwrap();
    assert!(db.read("settings", "absent").unwrap().is_none());
    let invalid = [
        Import {
            source: "valid",
            domain: "settings",
            key: "new",
            format_version: 1,
            payload: Some(b"valid"),
        },
        Import {
            source: "invalid",
            domain: "settings",
            key: "bad",
            format_version: 0,
            payload: None,
        },
    ];
    assert!(db.import("interrupted", &invalid).is_err());
    assert!(!db.origin_imported("interrupted").unwrap());
    assert!(db.read("settings", "new").unwrap().is_none());
}

#[test]
fn busy_is_bounded_and_never_acknowledges_or_partially_commits() {
    let (_dir, db) = setup();
    let mut other = db.connection().unwrap();
    let lock = other
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let start = std::time::Instant::now();
    assert!(db
        .commit(&[change("a", 0, Some(b"new"))])
        .unwrap_err()
        .contains("busy"));
    assert!(start.elapsed() < Duration::from_secs(3));
    drop(lock);
    assert!(db.read("settings", "a").unwrap().is_none());
    db.commit(&[change("a", 0, Some(b"new"))]).unwrap();
}

#[test]
fn database_full_rolls_back_and_read_only_diagnostics_are_stable() {
    let (_dir, db) = setup();
    let connection = db.connection().unwrap();
    let pages: i64 = connection
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    connection
        .pragma_update(None, "max_page_count", pages)
        .unwrap();
    let result = connection
        .execute(
            "INSERT INTO origins(name) VALUES(?1)",
            ["x".repeat(1024 * 1024)],
        )
        .unwrap_err();
    assert!(failure(result).contains("disk full"));
    let read_only =
        Connection::open_with_flags(db.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert!(
        failure(read_only.execute("DELETE FROM origins", []).unwrap_err()).contains("read-only")
    );
    db.integrity_check().unwrap();
}

#[test]
fn rejects_future_schema_foreign_application_and_corruption_without_replacement() {
    let (dir, db) = setup();
    let connection = db.connection().unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(Database::open(db.path())
        .unwrap_err()
        .contains("Unsupported"));
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION + 1
    );
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
        .pragma_update(None, "application_id", 12)
        .unwrap();
    assert!(Database::open(db.path()).is_err());
    let corrupt = dir.path().join("corrupt.sqlite3");
    std::fs::write(&corrupt, b"preserve corrupt data").unwrap();
    assert!(Database::open(&corrupt).unwrap_err().contains("corrupt"));
    assert_eq!(std::fs::read(corrupt).unwrap(), b"preserve corrupt data");

    let foreign = dir.path().join("foreign.sqlite3");
    let unrelated = Connection::open(&foreign).unwrap();
    unrelated
        .execute_batch(
            "CREATE TABLE private_data(value TEXT); INSERT INTO private_data VALUES('original')",
        )
        .unwrap();
    assert!(Database::open(&foreign)
        .unwrap_err()
        .contains("Unrecognized"));
    assert_eq!(
        unrelated
            .query_row("SELECT value FROM private_data", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "original"
    );
}

#[test]
fn foreign_keys_and_constraints_are_enforced() {
    let (_dir, db) = setup();
    let connection = db.connection().unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert!(connection
        .execute("INSERT INTO records VALUES('unknown','a',1,1,x'01')", [])
        .is_err());
    assert!(connection
        .execute("INSERT INTO domains VALUES('')", [])
        .is_err());
}

#[test]
fn online_backup_preserves_a_consistent_committed_snapshot() {
    let (dir, db) = setup();
    db.commit(&[change("a", 0, Some(b"before"))]).unwrap();
    let target = dir.path().join("backup/snapshot.sqlite3");
    db.backup(&target).unwrap();
    assert!(db.backup(&target).is_err());
    db.commit(&[change("a", 1, Some(b"after"))]).unwrap();
    let backup = Database::open(target).unwrap();
    assert_eq!(
        backup.read("settings", "a").unwrap().unwrap().payload,
        b"before"
    );
    backup.integrity_check().unwrap();
}

#[test]
fn safe_bounded_snapshots_and_path_failures() {
    let (dir, _db) = setup();
    let path = dir.path().join("source.json");
    assert!(snapshot(&path, 3).unwrap().is_none());
    std::fs::write(&path, b"1234").unwrap();
    assert!(snapshot(&path, 3).is_err());
    assert_eq!(snapshot(&path, 4).unwrap().unwrap(), b"1234");
    assert!(Database::open("relative.sqlite3").is_err());
    assert!(Database::open(dir.path()).is_err());
    assert!(Database::open(dir.path().join("../escape.sqlite3")).is_err());
    assert!(Database::open(path.join("blocked/cockpit.sqlite3")).is_err());
    assert!(snapshot(dir.path(), 100).is_err());
}

#[test]
fn concurrent_writers_reject_stale_revisions() {
    let (_dir, db) = setup();
    let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let db = db.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                db.commit(&[change("shared", 0, Some(b"value"))])
            })
        })
        .collect();
    gate.wait();
    assert_eq!(
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        1
    );
    assert_eq!(db.revision("settings", "shared").unwrap(), 1);
}

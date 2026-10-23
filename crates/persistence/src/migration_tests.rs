use super::*;
use crate::{Change, APPLICATION_ID, SCHEMA_VERSION};

fn setup() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    (dir, db)
}
fn source<'a>(name: &'a str, payload: Option<&'a [u8]>) -> Import<'a> {
    Import {
        source: name,
        domain: "settings",
        key: name,
        format_version: 1,
        payload,
    }
}
fn valid(sources: &[StagedSource]) -> Result<(), String> {
    for source in sources {
        if let Some(payload) = &source.payload {
            serde_json::from_slice::<serde_json::Value>(payload)
                .map_err(|_| "Invalid snapshot".to_string())?;
        }
    }
    Ok(())
}

#[test]
fn native_and_webview_staging_is_durable_and_cutover_waits_for_every_origin() {
    let (dir, db) = setup();
    assert_eq!(
        db.begin_migration(&["native", "webview:dev"])
            .unwrap()
            .pending_origins
            .len(),
        2
    );
    db.stage_migration("native", 0, &[source("native", Some(b"{}"))])
        .unwrap();
    assert!(db.read("settings", "native").unwrap().is_none());
    assert!(db.capture_migration().is_err());
    let reopened = Database::open(db.path()).unwrap();
    assert_eq!(
        reopened.migration_status().unwrap().pending_origins,
        ["webview:dev"]
    );
    // A failed or blocked webview never calls stage; no receipt appears.
    assert!(!reopened.origin_imported("webview:dev").unwrap());
    reopened.stage_migration("webview:dev", 1, &[]).unwrap();
    let capture = reopened.capture_migration().unwrap();
    assert_eq!(capture.generation(), 2);
    assert_eq!(capture.sources().len(), 1);
    reopened
        .finish_migration(capture, &dir.path().join("pre-cutover.sqlite3"), valid)
        .unwrap();
    assert!(reopened.migration_status().unwrap().ready);
    assert_eq!(
        reopened
            .read("settings", "native")
            .unwrap()
            .unwrap()
            .payload,
        b"{}"
    );
    assert!(reopened.origin_imported("webview:dev").unwrap());
    assert!(Database::open(dir.path().join("pre-cutover.sqlite3"))
        .unwrap()
        .read("settings", "native")
        .unwrap()
        .is_none());
    assert!(reopened
        .stage_migration("native", 3, &[source("native", Some(b"null"))])
        .is_err());
    assert!(reopened.capture_migration().is_err());
}

#[test]
fn identical_retries_do_not_duplicate_sources_and_stale_changes_are_rejected() {
    let (_dir, db) = setup();
    db.begin_migration(&["native"]).unwrap();
    assert_eq!(
        db.stage_migration("native", 0, &[source("a", None)])
            .unwrap()
            .generation,
        1
    );
    assert_eq!(
        db.stage_migration("native", 0, &[source("a", None)])
            .unwrap()
            .generation,
        1
    );
    assert!(db
        .stage_migration("native", 0, &[source("a", Some(b"{}"))])
        .is_err());
    assert_eq!(db.capture_migration().unwrap().sources().len(), 1);
    assert!(db.begin_migration(&["different"]).is_err());
    assert!(db.begin_migration(&[]).is_err());
    assert!(db.begin_migration(&["a", "a"]).is_err());
    assert!(db.begin_migration(&["\n"]).is_err());
    assert!(db.stage_migration("undeclared", 1, &[]).is_err());
    assert!(db
        .stage_migration("native", 1, &[source("a", None), source("a", None)])
        .is_err());
    let mut invalid = source("b", None);
    invalid.format_version = 0;
    assert!(db.stage_migration("native", 1, &[invalid]).is_err());
}

#[test]
fn failed_validation_or_backup_keeps_sources_pending_and_preserves_live_records() {
    let (dir, db) = setup();
    db.begin_migration(&["native"]).unwrap();
    db.stage_migration("native", 0, &[source("a", Some(b"invalid"))])
        .unwrap();
    assert!(db
        .finish_migration(
            db.capture_migration().unwrap(),
            &dir.path().join("failed.sqlite3"),
            valid
        )
        .is_err());
    assert!(!db.migration_status().unwrap().ready);
    assert!(!dir.path().join("failed.sqlite3").exists());
    db.stage_migration("native", 1, &[source("a", Some(b"{}"))])
        .unwrap();
    std::fs::write(dir.path().join("blocked"), b"original").unwrap();
    assert!(db
        .finish_migration(
            db.capture_migration().unwrap(),
            &dir.path().join("blocked/backup.sqlite3"),
            valid
        )
        .is_err());
    assert!(!db.migration_status().unwrap().ready);
    assert!(!db.origin_imported("native").unwrap());
    assert!(db.read("settings", "a").unwrap().is_none());
}

#[test]
fn concurrent_stage_during_validation_cannot_cut_over_an_old_capture() {
    let (dir, db) = setup();
    db.begin_migration(&["native"]).unwrap();
    db.stage_migration("native", 0, &[source("a", Some(b"{}"))])
        .unwrap();
    let captured = db.capture_migration().unwrap();
    db.stage_migration("native", 1, &[source("a", Some(b"[]"))])
        .unwrap();
    assert!(db
        .finish_migration(captured, &dir.path().join("stale.sqlite3"), valid)
        .unwrap_err()
        .contains("changed during"));
    assert!(!db.migration_status().unwrap().ready);
    assert!(dir.path().join("stale.sqlite3").is_file());
    assert!(db.read("settings", "a").unwrap().is_none());
    db.finish_migration(
        db.capture_migration().unwrap(),
        &dir.path().join("current.sqlite3"),
        valid,
    )
    .unwrap();
}

#[test]
fn conflicting_origins_require_explicit_resolution_but_equal_snapshots_are_allowed() {
    let (dir, db) = setup();
    db.begin_migration(&["native", "desktop"]).unwrap();
    db.stage_migration("native", 0, &[source("a", Some(b"{}"))])
        .unwrap();
    db.stage_migration("desktop", 1, &[source("a", Some(b"[]"))])
        .unwrap();
    assert!(db
        .finish_migration(
            db.capture_migration().unwrap(),
            &dir.path().join("conflict.sqlite3"),
            valid
        )
        .unwrap_err()
        .contains("Conflicting"));
    assert!(!dir.path().join("conflict.sqlite3").exists());
    db.stage_migration("desktop", 2, &[source("a", Some(b"{}"))])
        .unwrap();
    db.commit(&[Change {
        domain: "settings",
        key: "a",
        format_version: 1,
        expected_revision: 0,
        payload: Some(b"true"),
    }])
    .unwrap();
    db.finish_migration(
        db.capture_migration().unwrap(),
        &dir.path().join("resolved.sqlite3"),
        valid,
    )
    .unwrap();
    assert_eq!(
        db.read("settings", "a").unwrap().unwrap().payload,
        b"true",
        "later live edits cannot be overwritten by an old import"
    );
}

#[test]
fn source_checksum_corruption_prevents_cutover_and_has_no_receipt() {
    let (dir, db) = setup();
    db.begin_migration(&["native"]).unwrap();
    db.stage_migration("native", 0, &[source("a", Some(b"{}"))])
        .unwrap();
    db.connection()
        .unwrap()
        .execute("UPDATE migration_snapshots SET checksum='wrong'", [])
        .unwrap();
    assert!(db
        .finish_migration(
            db.capture_migration().unwrap(),
            &dir.path().join("bad.sqlite3"),
            valid
        )
        .unwrap_err()
        .contains("checksum"));
    assert!(!db.migration_status().unwrap().ready);
    assert!(!db.origin_imported("native").unwrap());
}

#[test]
fn schema_one_upgrade_and_failed_migration_are_transactional() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("schema.sql"))
        .unwrap();
    connection
        .pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
        .execute("INSERT INTO domains VALUES('settings')", [])
        .unwrap();
    connection
        .execute("INSERT INTO records VALUES('settings','a',1,7,x'7b7d')", [])
        .unwrap();
    let db = Database::open(&path).unwrap();
    assert_eq!(db.revision("settings", "a").unwrap(), 7);
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    let broken = dir.path().join("broken-v1.sqlite3");
    let original = rusqlite::Connection::open(&broken).unwrap();
    original.execute_batch(include_str!("schema.sql")).unwrap();
    original
        .pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    original.pragma_update(None, "user_version", 1).unwrap();
    original
        .execute_batch("CREATE TABLE migration_snapshots(collision TEXT)")
        .unwrap();
    assert!(Database::open(&broken).is_err());
    assert_eq!(
        original
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        original
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='migration_state'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

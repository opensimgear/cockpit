use super::*;
use serde_json::{json, Value};
fn validate(value: &Value) -> Result<(), String> {
    if value.is_object() {
        Ok(())
    } else {
        Err("Expected object".into())
    }
}
fn repository(db: &Database) -> Repository<Value> {
    Repository::new(db.clone(), "settings", "preferences", 1024, validate).unwrap()
}
#[test]
fn only_sqlite_is_authoritative_even_when_retired_files_exist() {
    let dir = crate::test_directory();
    let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    let legacy = dir.path().join("preferences.json");
    std::fs::write(&legacy, b"{retired").unwrap();
    let store = repository(&db);
    assert!(store.load().unwrap().is_none());
    store.save(&json!({"saved":true})).unwrap();
    std::fs::write(&legacy, b"changed retired file").unwrap();
    assert_eq!(
        repository(&db).load().unwrap().unwrap(),
        json!({"saved":true})
    );
    assert_eq!(std::fs::read(legacy).unwrap(), b"changed retired file");
}
#[test]
fn stale_reader_and_invalid_edits_cannot_replace_an_acknowledged_commit() {
    let dir = crate::test_directory();
    let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    let first = repository(&db);
    let stale = repository(&db);
    first.load().unwrap();
    stale.load().unwrap();
    first.save(&json!({"a":1})).unwrap();
    assert!(stale
        .save(&json!({"b":2}))
        .unwrap_err()
        .contains("conflict"));
    assert!(first.save(&json!([])).is_err());
    assert!(first.save(&json!({"long":"x".repeat(2048)})).is_err());
    first.save(&json!({"a":2})).unwrap();
    assert_eq!(stale.load().unwrap().unwrap()["a"], 2);
    stale.save(&json!({"b":3})).unwrap();
    assert_eq!(first.load().unwrap().unwrap()["b"], 3);
}
#[test]
fn explicit_reset_keeps_database_evidence_and_cannot_bypass_newer_commits() {
    let dir = crate::test_directory();
    let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    let stale = repository(&db);
    stale.load().unwrap();
    db.commit(&[Change {
        domain: "settings",
        key: "preferences",
        expected_revision: 0,
        payload: Some(b"broken"),
    }])
    .unwrap();
    let recovery = repository(&db);
    assert!(recovery.load().is_err());
    recovery.reset(&json!({"reset":true})).unwrap();
    assert_eq!(
        db.read("recovery/explicit-reset", "settings:preferences:1")
            .unwrap()
            .unwrap()
            .payload,
        b"broken"
    );
    assert!(stale.reset(&json!({})).unwrap_err().contains("conflict"));
    assert!(recovery.reset(&json!([])).is_err());
    assert!(recovery.reset(&json!({"long":"x".repeat(2048)})).is_err());
    assert_eq!(recovery.load().unwrap().unwrap(), json!({"reset":true}));
}
#[test]
fn corrupt_or_oversized_documents_never_default_and_repository_bounds_are_checked() {
    let dir = crate::test_directory();
    let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    for (i, bytes) in [b"{invalid".to_vec(), b"[]".to_vec(), vec![b' '; 2048]]
        .into_iter()
        .enumerate()
    {
        db.commit(&[Change {
            domain: "settings",
            key: "preferences",
            expected_revision: i as u64,
            payload: Some(&bytes),
        }])
        .unwrap();
        assert!(repository(&db).load().is_err());
    }
    assert!(Repository::<Value>::new(db.clone(), "", "k", 1, validate).is_err());
    assert!(Repository::<Value>::new(db.clone(), "d", "k", 0, validate).is_err());
    assert!(Repository::<Value>::new(db, "d", "k", crate::MAX_PAYLOAD + 1, validate).is_err());
}

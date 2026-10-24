use super::*;
#[test]
fn passive_missing_and_retired_files_are_not_authority() {
    let dir = crate::test_directory();
    let anchor = dir.path().join("nested/run.json");
    assert!(Evidence::inspect(&anchor, "recovery", 32)
        .unwrap()
        .is_none());
    assert!(!anchor.parent().unwrap().exists());
    std::fs::create_dir(anchor.parent().unwrap()).unwrap();
    std::fs::write(&anchor, b"retired").unwrap();
    let mut store = Evidence::open(&anchor, "recovery", 32).unwrap();
    assert!(store.payload().is_none());
    store.write(b"intent").unwrap();
    assert_eq!(std::fs::read(&anchor).unwrap(), b"retired");
    assert_eq!(
        Evidence::inspect(&anchor, "recovery", 32).unwrap().unwrap(),
        b"intent"
    );
}
#[test]
fn stale_and_same_revision_payload_changes_preserve_foreign_evidence() {
    let dir = crate::test_directory();
    let anchor = dir.path().join("run.json");
    let mut first = Evidence::open(&anchor, "recovery", 32).unwrap();
    let mut stale = Evidence::open(&anchor, "recovery", 32).unwrap();
    first.write(b"intent").unwrap();
    assert!(stale.write(b"stale").is_err());
    assert!(stale.verify().is_err());
    first
        .database
        .connection()
        .unwrap()
        .execute(
            "UPDATE records SET payload=?1 WHERE domain='recovery'",
            [b"foreign".as_slice()],
        )
        .unwrap();
    assert!(first.verify().is_err());
    assert!(first.write(b"overwrite").is_err());
    assert_eq!(
        Evidence::inspect(&anchor, "recovery", 32).unwrap().unwrap(),
        b"foreign"
    );
}
#[test]
fn namespace_identity_bounds_and_delete_recreate_are_fenced() {
    let dir = crate::test_directory();
    let anchor = dir.path().join("a/run.json");
    let database = dir.path().join("data/cockpit.sqlite3");
    crate::register_namespace(&[dir.path().into()], &database).unwrap();
    let mut store = Evidence::open(&anchor, "recovery", 4).unwrap();
    assert!(store.write(b"large").is_err());
    store.write(b"good").unwrap();
    assert!(Evidence::inspect(&anchor, "recovery", 2).is_err());
    assert!(
        Evidence::inspect(&dir.path().join("b/run.json"), "recovery", 4)
            .unwrap()
            .is_none()
    );
    store
        .database
        .commit(&[Change {
            domain: "recovery",
            key: "a/run.json",
            expected_revision: 1,
            payload: None,
        }])
        .unwrap();
    assert!(store.write(b"good").is_err());
    assert!(Evidence::open(&anchor, "recovery", 0).is_err());
    assert!(Evidence::open(Path::new("relative"), "recovery", 4).is_err());
}
#[test]
fn sibling_inventory_is_bounded_scoped_and_excludes_deletions() {
    let dir = crate::test_directory();
    let anchor = dir.path().join("runs/inventory.json");
    crate::register_namespace(&[dir.path().into()], &dir.path().join("db/cockpit.sqlite3"))
        .unwrap();
    assert!(Evidence::siblings(&anchor, "recovery", 2)
        .unwrap()
        .is_empty());
    assert!(!dir.path().join("db").exists());
    let mut a = Evidence::open(&dir.path().join("runs/a.json"), "recovery", 8).unwrap();
    a.write(b"a").unwrap();
    let mut b = Evidence::open(&dir.path().join("runs/b.json"), "recovery", 8).unwrap();
    b.write(b"b").unwrap();
    let mut nested = Evidence::open(&dir.path().join("runs/nested/c.json"), "recovery", 8).unwrap();
    nested.write(b"c").unwrap();
    assert_eq!(
        Evidence::siblings(&anchor, "recovery", 2).unwrap(),
        ["a.json", "b.json"]
    );
    assert!(Evidence::siblings(&anchor, "recovery", 1).is_err());
    assert!(Evidence::siblings(&anchor, "recovery", 0).is_err());
    b.database
        .commit(&[Change {
            domain: "recovery",
            key: "runs/b.json",
            expected_revision: 1,
            payload: None,
        }])
        .unwrap();
    assert_eq!(
        Evidence::siblings(&anchor, "recovery", 1).unwrap(),
        ["a.json"]
    );
}
#[test]
fn busy_commit_never_acknowledges_or_changes_cached_evidence() {
    let dir = crate::test_directory();
    let anchor = dir.path().join("run.json");
    let mut store = Evidence::open(&anchor, "recovery", 8).unwrap();
    store.write(b"intent").unwrap();
    let blocker = store.database.connection().unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(store.write(b"receipt").unwrap_err().contains("busy"));
    assert_eq!(store.payload(), Some(b"intent".as_slice()));
    blocker.execute_batch("ROLLBACK").unwrap();
    store.write(b"receipt").unwrap();
    assert_eq!(
        Evidence::inspect(&anchor, "recovery", 8).unwrap().unwrap(),
        b"receipt"
    );
}

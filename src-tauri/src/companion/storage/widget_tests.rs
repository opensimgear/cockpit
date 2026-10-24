use std::fs;

use super::*;
fn bundle(id: &str, revision: u64) -> Value {
    let mut bundle = builtins();
    bundle["library"]["definitions"][0]["id"] = json!(id);
    bundle["library"]["definitions"][0]["revision"] = json!(revision);
    bundle
}
fn save(bundle: Value, expected: Value) -> Value {
    json!({"command":"widget_save","bundle":bundle,"expectedVersion":expected})
}
#[test]
fn sole_toggle_seed_and_generic_guide_validate() {
    let r = Runtime::new(None);
    let list = r.request(&json!({"command":"widgets_list"})).unwrap();
    assert_eq!(list["total"], 1);
    assert_eq!(list["widgets"][0]["id"], "toggle-v2");
    assert_eq!(list["widgets"][0]["title"], "Toggle");
    validate_widget_bundle(&guide()["example"]).unwrap();
    assert_eq!(guide()["formatVersion"], 2);
}
#[test]
fn create_revise_page_and_restore_sources() {
    let dir = crate::test_directory();
    let path = dir.path().join("widgets.json");
    let r = Runtime::new(Some(path.clone()));
    let first = bundle("custom", 1);
    assert_eq!(
        r.request(&save(first.clone(), Value::Null)).unwrap()["version"],
        1
    );
    assert!(r
        .request(&save(first.clone(), Value::Null))
        .unwrap_err()
        .contains("conflict"));
    let mut next = bundle("custom", 2);
    next["library"]["definitions"][0]["title"] = json!("Revised");
    r.request(&save(next, json!(1))).unwrap();
    assert_eq!(
        r.request(&json!({"command":"widget_get","id":"custom","version":1}))
            .unwrap(),
        first
    );
    assert_eq!(
        r.request(&json!({"command":"widget_get","id":"custom"}))
            .unwrap()["library"]["definitions"][0]["revision"],
        2
    );
    let restored = Runtime::new(Some(path));
    assert_eq!(
        restored
            .request(&json!({"command":"widgets_list","limit":1}))
            .unwrap()["nextOffset"],
        1
    );
    assert_eq!(
        restored
            .request(&json!({"command":"widgets_list","offset":100}))
            .unwrap()["widgets"],
        json!([])
    );
}
#[test]
fn retired_file_is_ignored_and_sqlite_is_the_only_source_library() {
    let dir = crate::test_directory();
    let path = dir.path().join("widgets-v1.json");
    let other = dir.path().join("pairing.json");
    fs::write(&other, b"preserve pairing and trust").unwrap();
    fs::write(&path,r#"{"bundleVersion":1,"library":{"formatVersion":1,"definitions":[{"id":"obsolete"}]},"manifest":[],"assets":[]}"#).unwrap();
    let r = Runtime::new(Some(path.clone()));
    assert_eq!(
        r.request(&json!({"command":"widget_library_get"})).unwrap(),
        builtins()
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["bundleVersion"],
        1
    );
    assert_eq!(fs::read(&other).unwrap(), b"preserve pairing and trust");
    r.request(&save(bundle("after-reset", 1), Value::Null))
        .unwrap();
    assert_eq!(
        Runtime::new(Some(path))
            .request(&json!({"command":"widgets_list"}))
            .unwrap()["total"],
        2
    );
}
#[test]
fn immutable_revisions_invalid_source_and_stale_writes_preserve_previous_commit() {
    let r = Runtime::new(None);
    let first = bundle("custom", 1);
    r.request(&save(first.clone(), Value::Null)).unwrap();
    let mut conflict = first.clone();
    conflict["library"]["definitions"][0]["title"] = json!("Overwrite");
    assert!(r
        .request(&json!({"command":"widget_library_merge","bundle":conflict}))
        .is_err());
    let mut invalid = bundle("invalid", 1);
    invalid["library"]["definitions"][0]["scene"][0] = json!({"kind":"script","code":"alert(1)"});
    assert!(r.request(&save(invalid, Value::Null)).is_err());
    assert!(r.request(&save(bundle("custom", 3), json!(1))).is_err());
    assert_eq!(
        r.request(&json!({"command":"widget_get","id":"custom"}))
            .unwrap(),
        first
    );
}
#[test]
fn identical_imports_are_idempotent_and_legacy_imports_fail() {
    let r = Runtime::new(None);
    let b = bundle("imported", 1);
    for _ in 0..2 {
        r.request(&json!({"command":"widget_library_merge","bundle":b}))
            .unwrap();
    }
    assert_eq!(
        r.request(&json!({"command":"widgets_list"})).unwrap()["total"],
        2
    );
    assert!(r.request(&json!({"command":"widget_library_merge","bundle":{"bundleVersion":1,"library":{"formatVersion":1,"definitions":[]},"manifest":[],"assets":[]}})).is_err());
}
#[test]
fn corrupted_storage_and_failed_write_do_not_claim_success() {
    let dir = crate::test_directory();
    let path = dir.path().join("widgets.json");
    crate::persistence::test_support::record(&path, "widgets", "library", b"bad");
    assert!(Runtime::new(Some(path.clone()))
        .request(&json!({"command":"widget_library_get"}))
        .is_err());
    crate::persistence::test_support::record(
        &path,
        "widgets",
        "library",
        &vec![b' '; BUNDLE_BYTES + 1],
    );
    assert!(Runtime::new(Some(path.clone()))
        .request(&json!({"command":"widget_library_get"}))
        .is_err());
    let path = dir.path().join("healthy/widgets.json");
    let r = Runtime::new(Some(path.clone()));
    r.request(&json!({"command":"widget_library_get"})).unwrap();
    let db = osg_persistence::Database::for_namespace(&path).unwrap();
    let lock = osg_persistence::test_support::hold_writer(&db).unwrap();
    assert!(r
        .request(&save(bundle("failed", 1), Value::Null))
        .unwrap_err()
        .contains("busy"));
    assert!(r
        .request(&json!({"command":"widget_get","id":"failed"}))
        .is_err());
    drop(lock);
    r.request(&save(bundle("failed", 1), Value::Null)).unwrap();
}
#[test]
fn requests_concurrency_and_single_source_save_limits() {
    let r = std::sync::Arc::new(Runtime::new(None));
    for request in [
        json!({}),
        json!({"command":"unknown"}),
        json!({"command":"widgets_list","limit":0}),
        json!({"command":"widgets_list","offset":-1}),
        json!({"command":"widget_get","id":"missing"}),
        json!({"command":"widget_get","id":"toggle-v2","version":0}),
        json!({"command":"widget_library_get","path":"secret"}),
        json!({"command":"widget_save","bundle":bundle("new",1)}),
    ] {
        assert!(r.request(&request).is_err());
    }
    let handles = (0..4)
        .map(|_| {
            let r = r.clone();
            std::thread::spawn(move || {
                r.request(&save(bundle("concurrent", 1), Value::Null))
                    .is_ok()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap() as usize)
            .sum::<usize>(),
        1
    );
    let mut multiple = bundle("one", 1);
    multiple["library"]["definitions"]
        .as_array_mut()
        .unwrap()
        .push(bundle("two", 1)["library"]["definitions"][0].clone());
    assert!(r.request(&save(multiple, Value::Null)).is_err());
    assert!(r
        .request(
            &json!({"command":"widget_library_merge","bundle":{"extra":"x".repeat(BUNDLE_BYTES+1)}})
        )
        .is_err());
}

#[test]
fn concurrent_widget_writers_reject_stale_library_revisions() {
    let dir = crate::test_directory();
    let path = dir.path().join("widgets.json");
    let first = Runtime::new(Some(path.clone()));
    let second = Runtime::new(Some(path.clone()));
    first
        .request(&json!({"command":"widget_library_get"}))
        .unwrap();
    second
        .request(&json!({"command":"widget_library_get"}))
        .unwrap();
    first
        .request(&save(bundle("first", 1), Value::Null))
        .unwrap();
    assert!(second
        .request(&save(bundle("second", 1), Value::Null))
        .unwrap_err()
        .contains("conflict"));
    assert!(second
        .request(&json!({"command":"widget_get","id":"second"}))
        .is_err());
    let reopened = Runtime::new(Some(path));
    assert_eq!(
        reopened
            .request(&json!({"command":"widgets_list"}))
            .unwrap()["total"],
        2
    );
}

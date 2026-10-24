use super::*;
fn bundle(id: &str) -> Value {
    let mut value = builtins();
    value["definitions"][0]["id"] = json!(id);
    value
}
fn save(bundle: Value, expected: u64) -> Value {
    json!({"command":"widget_save","bundle":bundle,"expectedRevision":expected})
}
#[test]
fn current_seed_and_guide_have_plain_sources_without_format_or_source_revision() {
    let runtime = Runtime::new(None);
    let list = runtime.request(&json!({"command":"widgets_list"})).unwrap();
    assert_eq!(list["total"], 1);
    assert_eq!(list["revision"], 0);
    assert_eq!(list["widgets"][0]["id"], "toggle");
    validate_widget_bundle(&guide()["example"]).unwrap();
    assert!(guide()["example"]["definitions"][0]
        .get("revision")
        .is_none());
}
#[test]
fn current_sources_replace_by_id_only_at_the_exact_library_revision() {
    let runtime = Runtime::new(None);
    let source = bundle("mcp-widget");
    let created = runtime.request(&save(source.clone(), 0)).unwrap();
    assert_eq!(created["revision"], 1);
    let mut changed = source.clone();
    changed["definitions"][0]["title"] = json!("Changed source");
    assert!(runtime
        .request(&save(changed.clone(), 0))
        .unwrap_err()
        .contains("conflict"));
    let updated = runtime.request(&save(changed.clone(), 1)).unwrap();
    assert_eq!(updated["revision"], 2);
    assert_eq!(
        runtime
            .request(&json!({"command":"widget_get","id":"mcp-widget"}))
            .unwrap(),
        json!({"definitions":changed["definitions"],"revision":2})
    );
    assert_eq!(
        runtime
            .request(&json!({"command":"widgets_list","offset":1,"limit":1}))
            .unwrap()["total"],
        2
    );
}
#[test]
fn sql_is_authoritative_and_stale_or_busy_edits_do_not_change_visible_sources() {
    let directory = crate::test_directory();
    let path = directory.path().join("widgets.json");
    std::fs::write(&path, b"ignored retired JSON").unwrap();
    let first = Runtime::new(Some(path.clone()));
    let second = Runtime::new(Some(path.clone()));
    first.request(&save(bundle("saved"), 0)).unwrap();
    assert!(second.request(&save(bundle("stale"), 0)).is_err());
    let database = osg_persistence::Database::for_namespace(&path).unwrap();
    let blocker = osg_persistence::test_support::hold_writer(&database).unwrap();
    assert!(first
        .request(&save(bundle("blocked"), 1))
        .unwrap_err()
        .contains("busy"));
    drop(blocker);
    let reopened = Runtime::new(Some(path.clone()));
    assert_eq!(
        reopened
            .request(&json!({"command":"widgets_list"}))
            .unwrap()["total"],
        2
    );
    assert_eq!(std::fs::read(path).unwrap(), b"ignored retired JSON");
}
#[test]
fn malformed_unknown_and_oversized_requests_never_replace_saved_content() {
    let runtime = Runtime::new(None);
    runtime.request(&save(bundle("saved"), 0)).unwrap();
    for request in [
        json!({"command":"widget_get","id":"missing"}),
        json!({"command":"widgets_list","limit":0}),
        json!({"command":"widgets_list","offset":262145}),
        json!({"command":"widget_get","id":"saved","version":1}),
        json!({"command":"widget_save","bundle":bundle("old"),"expectedVersion":1}),
        json!({"command":"unknown"}),
        json!({"command":"widget_save","bundle":{"definitions":[]},"expectedRevision":1}),
    ] {
        assert!(runtime.request(&request).is_err(), "{request}");
    }
    let mut bad = bundle("bad");
    bad["definitions"][0]["scene"] = json!([{"kind":"script","code":"unsafe"}]);
    assert!(runtime.request(&save(bad, 1)).is_err());
    assert_eq!(
        runtime.request(&json!({"command":"widgets_list"})).unwrap()["revision"],
        1
    );
}
#[test]
fn corrupt_sql_is_not_replaced_by_seed_or_retired_file() {
    let directory = crate::test_directory();
    let path = directory.path().join("widgets.json");
    let database = osg_persistence::Database::for_namespace(&path).unwrap();
    database
        .commit(&[osg_persistence::Change {
            domain: "widgets",
            key: "library",
            expected_revision: 0,
            payload: Some(b"bad"),
        }])
        .unwrap();
    assert!(Runtime::new(Some(path))
        .request(&json!({"command":"widgets_list"}))
        .is_err());
    assert_eq!(
        database
            .read("widgets", "library")
            .unwrap()
            .unwrap()
            .payload,
        b"bad"
    );
}

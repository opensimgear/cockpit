use super::tests::{event, setup};
use super::*;

fn count(database: &Database, table: &str) -> i64 {
    database
        .connection()
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn retries_in_a_batch_and_after_reopen_create_one_event_and_fts_row() {
    let (_directory, database) = setup();
    let original = event("Device connected");
    database
        .activity_append(&[original.clone(), original.clone()])
        .unwrap();
    let reopened = Database::open_existing(database.path()).unwrap();
    reopened.activity_append(&[original]).unwrap();
    assert_eq!(
        reopened
            .activity_query(&ActivityQuery::default())
            .unwrap()
            .total,
        1
    );
    assert_eq!(count(&reopened, "activity_identity"), 1);
    assert_eq!(count(&reopened, "activity_fts"), 1);
}

#[test]
fn distinct_keys_preserve_identical_actions_as_distinct_events() {
    let (_directory, database) = setup();
    let first = event("Device connected");
    let mut second = first.clone();
    second.event_key.push_str("-second");
    database.activity_append(&[first, second]).unwrap();
    assert_eq!(
        database
            .activity_query(&ActivityQuery::default())
            .unwrap()
            .total,
        2
    );
    assert_eq!(count(&database, "activity_identity"), 2);
    assert_eq!(count(&database, "activity_fts"), 2);
}

#[test]
fn reusing_key_for_different_content_rolls_back_the_entire_batch() {
    let (_directory, database) = setup();
    let original = event("Device connected");
    database.activity_append(&[original.clone()]).unwrap();
    let mut conflict = original;
    conflict.message = "Different action".into();
    let error = database
        .activity_append(&[event("Would otherwise be inserted"), conflict])
        .unwrap_err();
    assert!(error.contains("reused with different content"));
    assert_eq!(
        database
            .activity_query(&ActivityQuery::default())
            .unwrap()
            .total,
        1
    );
    assert_eq!(count(&database, "activity_identity"), 1);
    assert_eq!(count(&database, "activity_fts"), 1);
}

#[test]
fn clear_and_prune_remove_identities_so_old_keys_can_be_reused() {
    let (_directory, database) = setup();
    let cleared = event("Cleared action");
    database.activity_append(&[cleared.clone()]).unwrap();
    assert_eq!(database.activity_clear().unwrap(), 1);
    assert_eq!(count(&database, "activity_identity"), 0);
    database.activity_append(&[cleared]).unwrap();
    assert_eq!(
        database
            .activity_query(&ActivityQuery::default())
            .unwrap()
            .total,
        1
    );

    let mut pruned = event("Pruned action");
    database.activity_append(&[pruned.clone()]).unwrap();
    pruned.occurred_at = now().saturating_sub(8 * DAY_MS);
    database.connection().unwrap().execute(
        "UPDATE activity_events SET occurred_at=?1 WHERE id=(SELECT event_id FROM activity_identity WHERE event_key=?2)",
        params![pruned.occurred_at, pruned.event_key],
    ).unwrap();
    assert_eq!(database.activity_prune().unwrap(), 1);
    assert_eq!(count(&database, "activity_identity"), 1);
    pruned.occurred_at = now();
    database.activity_append(&[pruned]).unwrap();
    assert_eq!(
        database
            .activity_query(&ActivityQuery::default())
            .unwrap()
            .total,
        2
    );
    assert_eq!(count(&database, "activity_identity"), 2);
}

#[test]
fn event_keys_must_be_nonempty_bounded_ascii_identifiers() {
    let (_directory, database) = setup();
    for key in [
        String::new(),
        "x".repeat(129),
        "bad key".into(),
        "ümlaut".into(),
    ] {
        let mut invalid = event("Invalid key");
        invalid.event_key = key;
        assert!(database.activity_append(&[invalid]).is_err());
    }
    let mut valid = event("Valid key");
    valid.event_key = "source:action-123.uuid_1".into();
    database.activity_append(&[valid]).unwrap();
    assert_eq!(
        database
            .activity_query(&ActivityQuery::default())
            .unwrap()
            .total,
        1
    );
}

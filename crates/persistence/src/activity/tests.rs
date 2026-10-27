use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_EVENT_KEY: AtomicU64 = AtomicU64::new(1);

pub(super) fn setup() -> (tempfile::TempDir, Database) {
    let dir = crate::test_directory();
    let db = Database::open(dir.path().join("activity.sqlite3")).unwrap();
    (dir, db)
}
pub(super) fn event(message: &str) -> NewActivityEvent {
    NewActivityEvent {
        event_key: format!("test-{}", NEXT_EVENT_KEY.fetch_add(1, Ordering::Relaxed)),
        occurred_at: now(),
        category: "device".into(),
        event_type: "device.connected".into(),
        level: "info".into(),
        message: message.into(),
        detail: "USB path 0123:abcd".into(),
        entity_id: Some("device-42".into()),
        entity_name: Some("München panel".into()),
        source: "cockpit".into(),
    }
}
fn all(db: &Database) -> ActivityPage {
    db.activity_query(&ActivityQuery::default()).unwrap()
}

#[test]
fn persisted_reopen_facets_and_clear_keep_ids_monotonic() {
    let (_dir, db) = setup();
    db.activity_append(&[event("Connected")]).unwrap();
    let reopened = Database::open_existing(db.path()).unwrap();
    let page = all(&reopened);
    assert_eq!(page.total, 1);
    assert_eq!(page.categories, ["device"]);
    assert_eq!(page.levels, ["info"]);
    assert_eq!(page.event_types, ["device.connected"]);
    let id = page.events[0].id;
    assert_eq!(reopened.activity_clear().unwrap(), 1);
    assert_eq!(reopened.activity_clear().unwrap(), 0);
    assert_eq!(all(&db).total, 0);
    assert!(all(&db).categories.is_empty());
    assert_eq!(
        db.connection()
            .unwrap()
            .query_row("SELECT count(*) FROM activity_fts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.activity_append(&[event("Reconnected")]).unwrap();
    assert!(all(&db).events[0].id > id);
}

#[test]
fn retention_defaults_persists_and_prunes_writes_reads_settings_and_periodic() {
    let (_dir, db) = setup();
    assert_eq!(db.activity_settings().unwrap(), ActivitySettings::default());
    let mut old = event("Old");
    old.occurred_at -= 8 * DAY_MS;
    db.activity_append(&[old.clone(), event("Fresh")]).unwrap();
    assert_eq!(all(&db).total, 1);
    db.activity_settings_save(30).unwrap();
    db.activity_append(&[old.clone()]).unwrap();
    assert_eq!(all(&db).total, 2);
    assert_eq!(
        Database::open_existing(db.path())
            .unwrap()
            .activity_settings()
            .unwrap()
            .retention_days,
        30
    );
    db.activity_settings_save(7).unwrap();
    assert_eq!(all(&db).total, 1);
    // Age a retained event without a wall-clock sleep to exercise read pruning.
    let connection = db.connection().unwrap();
    connection
        .execute(
            "UPDATE activity_events SET occurred_at=?1",
            [old.occurred_at],
        )
        .unwrap();
    assert_eq!(all(&db).total, 0);
    db.activity_append(&[event("Periodic")]).unwrap();
    connection
        .execute(
            "UPDATE activity_events SET occurred_at=?1",
            [old.occurred_at],
        )
        .unwrap();
    assert_eq!(db.activity_prune().unwrap(), 1);
    assert_eq!(db.activity_prune().unwrap(), 0);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM activity_fts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn full_text_unicode_punctuation_and_all_indexed_fields() {
    let (_dir, db) = setup();
    db.activity_append(&[event("Motor ready"), event("Unrelated")])
        .unwrap();
    for search in [
        "motor",
        "\"motor\"",
        "München",
        "münchen",
        "0123:abcd",
        "device-42",
        "device.connected",
        "device",
        "motor AND",
        "***",
        "\" OR * : ( )",
    ] {
        let query = ActivityQuery {
            search: search.into(),
            ..Default::default()
        };
        let count = db.activity_query(&query).unwrap().total;
        match search {
            "motor" | "\"motor\"" => assert_eq!(count, 1, "{search}"),
            "motor AND" | "***" | "\" OR * : ( )" => assert_eq!(count, 0, "{search}"),
            _ => assert_eq!(count, 2, "{search}"),
        }
    }
    db.activity_clear().unwrap();
    assert_eq!(
        db.activity_query(&ActivityQuery {
            search: "Motor".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        0
    );
}

#[test]
fn filters_inclusive_dates_and_stable_pagination() {
    let (_dir, db) = setup();
    let time = now();
    let mut first = event("First");
    first.occurred_at = time;
    let mut second = first.clone();
    second.event_key = event("Second").event_key;
    second.message = "Second".into();
    second.level = "warning".into();
    let mut third = first.clone();
    third.event_key = event("Third").event_key;
    third.message = "Third".into();
    third.occurred_at = time - 100;
    third.category = "profile".into();
    third.event_type = "profile.saved".into();
    third.entity_id = None;
    db.activity_append(&[first, second, third]).unwrap();
    let query = ActivityQuery {
        limit: 1,
        ..Default::default()
    };
    assert_eq!(
        db.activity_query(&query).unwrap().events[0].message,
        "Second"
    );
    let query = ActivityQuery { offset: 1, ..query };
    assert_eq!(
        db.activity_query(&query).unwrap().events[0].message,
        "First"
    );
    assert_eq!(
        db.activity_query(&ActivityQuery {
            offset: 99,
            ..Default::default()
        })
        .unwrap()
        .events
        .len(),
        0
    );
    let query = ActivityQuery {
        categories: vec!["device".into()],
        levels: vec!["warning".into()],
        event_types: vec!["device.connected".into()],
        entity_id: Some("device-42".into()),
        since: Some(time),
        until: Some(time),
        ..Default::default()
    };
    let page = db.activity_query(&query).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.events[0].message, "Second");
    assert_eq!(page.categories, ["device", "profile"]);
    assert_eq!(
        db.activity_query(&ActivityQuery {
            until: Some(time - 1),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );
}

#[test]
fn pagination_ceiling_excludes_new_and_backdated_writes_across_reopen() {
    let (_dir, db) = setup();
    let timestamp = now();
    let original: Vec<_> = (0..6)
        .map(|index| NewActivityEvent {
            occurred_at: timestamp,
            ..event(&format!("Original {index}"))
        })
        .collect();
    db.activity_append(&original).unwrap();
    let first = db
        .activity_query(&ActivityQuery {
            limit: 2,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.through_id, 6);
    db.activity_append(&[
        NewActivityEvent {
            occurred_at: timestamp + 1,
            ..event("New")
        },
        NewActivityEvent {
            occurred_at: timestamp - 1,
            ..event("Backdated")
        },
    ])
    .unwrap();
    let reopened = Database::open_existing(db.path()).unwrap();
    let mut ids: Vec<_> = first.events.iter().map(|event| event.id).collect();
    for offset in [2, 4] {
        let page = reopened
            .activity_query(&ActivityQuery {
                limit: 2,
                offset,
                through_id: Some(first.through_id),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 6);
        assert_eq!(page.through_id, first.through_id);
        ids.extend(page.events.iter().map(|event| event.id));
    }
    assert_eq!(ids, [6, 5, 4, 3, 2, 1]);
    assert_eq!(all(&reopened).total, 8);
    assert_eq!(all(&reopened).through_id, 8);
}

#[test]
fn pagination_ceiling_does_not_resurrect_pruned_or_cleared_events() {
    let (_dir, db) = setup();
    assert_eq!(all(&db).through_id, 0);
    db.activity_append(&[event("One"), event("Two")]).unwrap();
    let ceiling = all(&db).through_id;
    let pinned = ActivityQuery {
        through_id: Some(ceiling),
        ..Default::default()
    };
    db.connection()
        .unwrap()
        .execute(
            "UPDATE activity_events SET occurred_at=?1 WHERE id=1",
            [now() - 8 * DAY_MS],
        )
        .unwrap();
    let page = db.activity_query(&pinned).unwrap();
    assert_eq!(page.through_id, ceiling);
    assert_eq!(page.total, 1);
    assert_eq!(page.events[0].message, "Two");
    db.activity_clear().unwrap();
    db.activity_append(&[event("After clear")]).unwrap();
    assert_eq!(db.activity_query(&pinned).unwrap().total, 0);
    assert_eq!(all(&db).total, 1);
    assert!(all(&db).through_id > ceiling);
    for through_id in [0, 9_007_199_254_740_992, u64::MAX] {
        assert!(db
            .activity_query(&ActivityQuery {
                through_id: Some(through_id),
                ..Default::default()
            })
            .is_err());
    }
}

#[test]
fn invalid_input_is_atomic_and_epoch_is_fenced() {
    let (_dir, db) = setup();
    for invalid in [
        NewActivityEvent {
            occurred_at: -1,
            ..event("bad")
        },
        NewActivityEvent {
            category: "".into(),
            ..event("bad")
        },
        event(""),
    ] {
        assert!(db.activity_append(&[event("valid"), invalid]).is_err());
    }
    assert_eq!(all(&db).total, 0);
    for query in [
        ActivityQuery {
            limit: 0,
            ..Default::default()
        },
        ActivityQuery {
            limit: 501,
            ..Default::default()
        },
        ActivityQuery {
            since: Some(-1),
            ..Default::default()
        },
        ActivityQuery {
            until: Some(MAX_DATE + 1),
            ..Default::default()
        },
        ActivityQuery {
            since: Some(2),
            until: Some(1),
            ..Default::default()
        },
    ] {
        assert!(db.activity_query(&query).is_err());
    }
    for days in [0, 3651] {
        assert!(db.activity_settings_save(days).is_err());
    }
    db.connection()
        .unwrap()
        .execute(
            "UPDATE incarnation SET epoch=lower(hex(randomblob(16)))",
            [],
        )
        .unwrap();
    assert!(db
        .activity_append(&[event("stale")])
        .unwrap_err()
        .contains("Restart"));
    assert!(db.activity_clear().is_err());
    assert!(db.activity_prune().is_err());
    assert!(db.activity_query(&ActivityQuery::default()).is_err());
    assert!(db.activity_settings_save(30).is_err());
}

#[test]
fn concurrent_writers_preserve_rows_and_search_index() {
    let (_dir, db) = setup();
    let threads: Vec<_> = (0..4)
        .map(|worker| {
            let db = db.clone();
            std::thread::spawn(move || {
                for index in 0..10 {
                    db.activity_append(&[event(&format!("worker{worker} item{index}"))])
                        .unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(all(&db).total, 40);
    assert_eq!(
        db.activity_query(&ActivityQuery {
            search: "worker2".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        10
    );
    db.integrity_check().unwrap();
}

#[test]
fn initializes_activity_on_existing_current_database_without_losing_records() {
    let (_dir, db) = setup();
    db.commit(&[crate::Change {
        domain: "settings",
        key: "retained",
        expected_revision: 0,
        payload: Some(b"value"),
    }])
    .unwrap();
    db.connection()
        .unwrap()
        .execute_batch(
            "DROP TABLE activity_fts; DROP TABLE activity_identity; DROP TABLE activity_events; DROP TABLE activity_settings;",
        )
        .unwrap();
    let reopened = Database::open(db.path()).unwrap();
    assert_eq!(
        reopened
            .read("settings", "retained")
            .unwrap()
            .unwrap()
            .payload,
        b"value"
    );
    assert_eq!(reopened.activity_settings().unwrap().retention_days, 7);
    reopened.activity_append(&[event("Installed")]).unwrap();
    assert_eq!(all(&reopened).total, 1);
}

#[test]
fn passive_existing_open_preserves_core_database_bytes_until_boot_initialization() {
    let (_dir, db) = setup();
    db.commit(&[crate::Change {
        domain: "settings",
        key: "retained",
        expected_revision: 0,
        payload: Some(b"value"),
    }])
    .unwrap();
    db.connection()
        .unwrap()
        .execute_batch(
            "DROP TABLE activity_fts; DROP TABLE activity_identity; DROP TABLE activity_events; DROP TABLE activity_settings;",
        )
        .unwrap();
    let path = db.path().to_owned();
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let passive = Database::open_existing(&path).unwrap();
    assert_eq!(
        passive
            .read("settings", "retained")
            .unwrap()
            .unwrap()
            .payload,
        b"value"
    );
    let activity_objects: i64 = passive
        .connection()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'activity_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(activity_objects, 0);
    drop(passive);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let boot = Database::open(&path).unwrap();
    assert_eq!(
        boot.read("settings", "retained").unwrap().unwrap().payload,
        b"value"
    );
    assert_eq!(
        boot.activity_settings().unwrap(),
        ActivitySettings::default()
    );
    boot.activity_append(&[event("Initialized on boot")])
        .unwrap();
    assert_eq!(all(&boot).total, 1);
}

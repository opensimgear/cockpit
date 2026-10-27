use super::tests::{event, setup};
use super::*;

#[test]
fn bookkeeping_never_crowds_useful_history_search_facets_or_pages_and_is_not_deleted() {
    let (_directory, database) = setup();
    let mut bookkeeping = event("ui preferences save completed for remembered.");
    bookkeeping.category = "configuration".into();
    bookkeeping.event_type = "ui_preferences_save".into();
    bookkeeping.entity_id = Some("remembered".into());
    let mut failure = bookkeeping.clone();
    failure.event_type = "ui_preferences_save_failed".into();
    failure.level = "error".into();
    failure.message = "Remembered devices could not be saved.".into();
    let mut changed = event("Race profile updated.");
    changed.category = "profiles".into();
    changed.event_type = "profile_editor_save".into();
    let mut events = vec![changed, failure];
    events.extend((0..150).map(|_| {
        let mut duplicate = bookkeeping.clone();
        duplicate.event_key = event("bookkeeping").event_key;
        duplicate
    }));
    database.activity_append(&events).unwrap();
    let page = database
        .activity_query(&ActivityQuery {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.events.len(), 1);
    assert!(!page.event_types.contains(&"ui_preferences_save".into()));
    let next = database
        .activity_query(&ActivityQuery {
            limit: 1,
            offset: 1,
            through_id: Some(page.through_id),
            ..Default::default()
        })
        .unwrap();
    assert_ne!(page.events[0].id, next.events[0].id);
    let search = database
        .activity_query(&ActivityQuery {
            search: "remembered".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(search.total, 1);
    assert_eq!(search.events[0].level, "error");
    let cache = database
        .activity_query(&ActivityQuery {
            event_types: vec!["ui_preferences_save".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cache.total, 0);
    assert_eq!(
        database.activity_clear().unwrap(),
        152,
        "Read filtering must preserve stored history until the user clears it."
    );
}

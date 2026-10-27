use super::{diff, Snapshot};
use serde_json::json;
use std::collections::BTreeMap;

fn event<'a>(events: &'a [osg_persistence::NewActivityEvent], kind: &str) -> &'a osg_persistence::NewActivityEvent {
    events.iter().find(|event| event.event_type == kind).unwrap_or_else(|| panic!("missing {kind}: {events:?}"))
}

#[test]
fn profile_session_messages_include_human_states_and_current_safe_reason() {
    let names = BTreeMap::from([("profile-id".to_owned(), "Race".to_owned())]);
    let mut before = Snapshot::default();
    before.profile_session(
        json!({"state":"running","owner":{"profileId":"profile-id","session":"OWNER_SENTINEL","profileRevision":91},"degradation":[],"fault":null}),
        &names,
    );
    let mut after = Snapshot::default();
    after.profile_session(
        json!({"state":"degraded","owner":{"profileId":"profile-id","session":"OWNER_SENTINEL","profileRevision":92},"degradation":["Wheel unavailable"],"fault":null}),
        &names,
    );
    let events = diff(&before, &after);
    let entry = event(&events, "profile_session_degraded");
    assert_eq!(entry.message, "Profile “Race” is degraded. Reason: Wheel unavailable");
    assert!(entry.detail.contains("Session state: Active → Degraded"));
    assert!(entry.detail.contains("Reason: Wheel unavailable"));
    assert!(!entry.message.contains("OWNER_SENTINEL"));
    assert!(!entry.detail.contains("OWNER_SENTINEL"));
    assert!(!entry.detail.contains("profileRevision"));
}

#[test]
fn reason_updates_are_recorded_even_when_state_stays_degraded() {
    let names = BTreeMap::from([("profile-id".to_owned(), "Race".to_owned())]);
    let mut before = Snapshot::default();
    before.profile_session(json!({"state":"degraded","degradation":["First safe reason"]}), &names);
    let mut after = Snapshot::default();
    after.profile_session(json!({"state":"degraded","degradation":["Second safe reason"]}), &names);
    let entries = diff(&before, &after);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].message.contains("Second safe reason"));
    assert!(entries[0].detail.contains("Second safe reason"));
    assert!(!entries[0].detail.contains("First safe reason"));
}

#[test]
fn integration_state_and_device_availability_retain_safe_reasons() {
    let mut before = Snapshot::default();
    before.insert("integrations", "integration", "dcs", "DCS World", json!({"state":"ready","message":""}));
    let mut after = Snapshot::default();
    after.insert("integrations", "integration", "dcs", "DCS World", json!({"state":"needs-attention","message":"Resource token expired; API unavailable."}));
    let events = diff(&before, &after);
    let integration = event(&events, "integration_changed");
    assert_eq!(integration.message, "DCS World integration needs attention. Reason: Resource token expired; API unavailable.");
    assert!(integration.detail.contains("Integration state: Active → Degraded"));

    before.insert("devices", "module", "wheel", "Wheel", json!({"online":true}));
    after.insert("devices", "module", "wheel", "Wheel", json!({"online":false,"reason":"USB connection lost."}));
    let events = diff(&before, &after);
    let device = event(&events, "module_offline");
    assert!(device.message.contains("offline"));
    assert!(device.message.contains("USB connection lost."));
    assert!(device.detail.contains("Reason: USB connection lost."));
}

#[test]
fn vehicle_and_discovery_details_show_readable_before_after_and_counts() {
    let mut before = Snapshot::default();
    before.insert("games", "game_vehicle", "dcs", "DCS", json!({"id":"old","name":"Old aircraft"}));
    let mut after = Snapshot::default();
    after.insert("games", "game_vehicle", "dcs", "DCS", json!({"id":"new","name":"New aircraft"}));
    let events = diff(&before, &after);
    let vehicle = event(&events, "game_vehicle_changed");
    assert!(vehicle.detail.contains("“Old aircraft” → “New aircraft”"));

    before = Snapshot::default();
    before.discovery(&json!({"scanId":"scan-1","status":"scanning","proposed":0,"diagnostics":[]}));
    after = Snapshot::default();
    after.discovery(&json!({"scanId":"scan-1","status":"ready","proposed":2,"artworkReady":1,"tagged":1,"diagnostics":["One library could not be read."]}));
    let events = diff(&before, &after);
    let discovery = event(&events, "discovery_completed");
    assert!(discovery.message.contains("finished"));
    assert!(discovery.detail.contains("2 proposed changes; 1 artwork ready; 1 tags applied."));
    assert!(discovery.detail.contains("Diagnostic: One library could not be read."));
}

#[test]
fn profile_revision_only_changes_are_suppressed_but_graph_changes_are_described() {
    let mut before = Snapshot::default();
    before.insert("profiles", "profile", "race", "Race", json!({"name":"Race","revision":1,"assignments":[]}));
    let mut after = Snapshot::default();
    after.insert("profiles", "profile", "race", "Race", json!({"name":"Race","revision":2,"assignments":[]}));
    assert!(diff(&before, &after).is_empty());
    after = Snapshot::default();
    after.insert("profiles", "profile", "race", "Race", json!({"name":"Race","revision":3,"assignments":[{"id":"wheel","name":"Wheel"}]}));
    let events = diff(&before, &after);
    let updated = event(&events, "profile_updated");
    assert!(updated.detail.contains("Assignments"));
    assert!(!updated.detail.contains("Revision"));
}

#[test]
fn game_tag_changes_use_captured_tag_names() {
    let mut before = Snapshot::default();
    before.games(&[json!({"id":"sim","name":"Sim","tagIds":[],"tagNames":{"track":"Track"}})]);
    let mut after = Snapshot::default();
    after.games(&[json!({"id":"sim","name":"Sim","tagIds":["track"],"tagNames":{"track":"Track"}})]);
    let events = diff(&before, &after);
    let updated = event(&events, "game_updated");
    assert!(updated.detail.contains("Tags added: Track"), "{}", updated.detail);
    assert!(!updated.detail.contains("tagNames"));
}

#[test]
fn game_session_revisions_are_silent_and_session_changes_hide_internal_ids() {
    let mut before=Snapshot::default();before.insert("integrations","game_session","sim","Sim",json!({"status":"ready","contextSession":"session-1","catalogueRevision":"sha256:OLD"}));
    let mut after=Snapshot::default();after.insert("integrations","game_session","sim","Sim",json!({"status":"ready","contextSession":"session-1","catalogueRevision":"sha256:NEW"}));
    assert!(diff(&before,&after).is_empty());
    after.insert("integrations","game_session","sim","Sim",json!({"status":"waiting-game","contextSession":null,"catalogueRevision":"sha256:NEW"}));
    let entries=diff(&before,&after);let detail=&entries[0].detail;
    assert!(detail.contains("Integration availability"));assert!(detail.contains("session ended"));
    assert!(!detail.contains("sha256"));assert!(!detail.contains("session-1"));
}

#[test]
fn adding_a_game_does_not_create_empty_setup_and_waiting_session_events() {
    let mut after=Snapshot::default();after.games(&[json!({"id":"new","name":"New","integrationSetup":{"state":"setup-required"},"integrationContext":{"status":"waiting-game"}})]);
    let events=diff(&Snapshot::default(),&after);
    assert_eq!(events.len(),1,"{events:?}");assert_eq!(events[0].event_type,"game_created");
}

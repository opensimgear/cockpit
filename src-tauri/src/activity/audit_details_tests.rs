use super::*;
use super::super::{changes,context::Context};
use serde_json::json;

#[test]
fn edits_show_canonical_values_and_application_failures_preserve_saved_changes() {
    let before=json!({"id":"race","name":"Race","revision":1});
    let context=Context{kind:"profile",before:changes::profile_snapshot(&before)};
    let request=json!({"command":"profile_editor_save","document":{"id":"race","name":"New Race"}});
    let response=json!({"ok":true,"state":{"profiles":{"document":{"id":"race","name":"New Race","revision":2},"saveApplication":{"state":"failed","error":"Profile saved, but Wheel is offline; reconnect it."}}}});
    let entry=mutation_with_context(&request,&response,Some(&context)).unwrap();
    assert_eq!(entry.event_type,"profile_editor_save_apply_failed");
    assert_eq!(entry.level,"warning");
    assert!(entry.message.contains("saved"));
    assert!(entry.message.contains("Wheel is offline"));
    assert!(entry.detail.contains("Name: “Race” → “New Race”"));
    assert!(entry.detail.contains("reconnect it"));
    assert!(!entry.detail.contains("Revision"));
}

#[test]
fn failures_keep_domain_reasons_without_claiming_requested_changes() {
    let context=Context{kind:"profile",before:json!({"id":"race","name":"Race"})};
    let entry=mutation_with_context(&json!({"command":"profile_rename","id":"race","name":"Never saved"}),
        &json!({"ok":false,"error":{"message":"Resource token expired; authorization revoked; API unavailable."}}),Some(&context)).unwrap();
    assert!(entry.message.starts_with("Could not rename"));
    assert!(entry.message.contains("Resource token expired"));
    assert!(entry.detail.contains("authorization revoked"));
    assert!(!entry.detail.contains('→'));
    assert!(!entry.detail.contains("Never saved"));
}

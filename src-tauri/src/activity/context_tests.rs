use super::*;
use serde_json::json;

fn context(kind:&'static str,before:Value)->Context { Context {kind,before:if kind=="game" {changes::game_snapshot(&before)}else{before}} }

#[test]
fn profile_details_use_canonical_graph_not_submitted_graph() {
    let before=json!({"id":"rig","name":"Rig","resources":[{"id":"wheel","name":"Wheel","required":false}],"processingNodes":[{"id":"scale","name":"Scale","operation":{"kind":"scale","factor":1}}]});
    let after=json!({"id":"rig","name":"Rig","resources":[{"id":"wheel","name":"Wheel","required":true}],"processingNodes":[{"id":"scale","name":"Scale","operation":{"kind":"scale","factor":2}}]});
    let result=context("profile",changes::profile_snapshot(&before)).details(
        &json!({"command":"profile_save","document":{"id":"rig","name":"REQUEST_SECRET"}}),
        &json!({"ok":true,"state":{"profiles":{"document":after}}})).join("\n");
    assert!(result.contains("Required: no → yes"));
    assert!(result.contains("Factor: 1 → 2"));
    assert!(!result.contains("REQUEST_SECRET"));
}

#[test]
fn errors_and_missing_canonical_state_do_not_claim_settings_changes() {
    let captured=context("configuration",json!({"mode":"manual"}));
    for response in [json!({"ok":false,"error":"failed"}),json!({"ok":true}),json!({"ok":false,"state":{"activityChangeAfter":{"mode":"automatic"}}})] {
        assert!(captured.details(&json!({"command":"profile_selection_mode","mode":"automatic","password":"SENTINEL"}),&response).is_empty());
    }
}

#[test]
fn injected_snapshots_support_settings_and_projected_scripts() {
    let before=json!({"id":"rig","processingNodes":[{"id":"script","operation":{"kind":"script","source":"old script"}}]});
    let after=json!({"id":"rig","processingNodes":[{"id":"script","operation":{"kind":"script","source":"SENSITIVE_SCRIPT"}}]});
    let result=context("profile",changes::profile_snapshot(&before)).details(&json!({"command":"profile_save"}),
        &json!({"ok":true,"state":{"activityChangeAfter":changes::profile_snapshot(&after)}})).join("\n");
    assert!(result.contains("Script content updated."));
    assert!(!result.contains("SENSITIVE_SCRIPT"));
    let result=context("configuration",json!({"mode":"manual","userFolder":"C:/old"})).details(&json!({"command":"profile_local_settings_save"}),
        &json!({"ok":true,"state":{"activityChangeAfter":{"mode":"automatic","userFolder":"C:/new"}}})).join("\n");
    assert!(result.contains("Mode: “manual” → “automatic”"));
    assert!(result.contains("User folder: “C:/old” → “C:/new”"));
}

#[test]
fn game_uses_matching_catalogue_and_tag_names_with_safe_fallback() {
    let captured=context("game",json!({"id":"race","name":"Race","tagIds":[],"tagNames":{"racing":"Racing games"}}));
    for games in [json!({"catalogue":[{"id":"unrelated","name":"No"},{"id":"race","name":"Race night","tagIds":["racing"]}]}),
        json!({"catalogue":[{"id":"race","name":"Race night","tagIds":["racing"]}],"tags":{"racing":"Racing games"}})] {
        let result=captured.details(&json!({"command":"game_save","gameId":"race","name":"REQUEST_SECRET"}),&json!({"ok":true,"state":{"games":games}})).join("\n");
        assert!(result.contains("Name: “Race” → “Race night”"));
        assert!(result.contains("Tags added: Racing games"));
        assert!(!result.contains("REQUEST_SECRET"));
    }
}

#[test]
fn ui_preferences_and_retention_show_before_and_acknowledged_after() {
    for (key,before,after,expected) in [
        ("theme",json!(false),json!(true),"Dark theme: disabled → enabled"),
        ("devices-mode",json!("list"),json!("topology"),"Device view: “list” → “topology”"),
        ("canvas-styling",json!({"cornerRadius":24,"arrowStyle":"smooth"}),json!({"cornerRadius":30,"arrowStyle":"smooth"}),"Corner radius: 24 → 30"),
        ("tray",json!({"closeToTray":false}),json!({"closeToTray":true}),"Close to tray: disabled → enabled"),
    ] {
        let result=context("ui_preferences",before).details(&json!({"command":"ui_preferences_save","key":key,"value":"REQUEST_SECRET"}),
            &json!({"ok":true,"state":{"uiPreferences":{"revision":2,"value":after}}}));
        assert_eq!(result,[expected]);
    }
    assert_eq!(context("activity_settings",json!({"retentionDays":7})).details(&json!({"command":"activity_settings_save","retentionDays":999}),
        &json!({"ok":true,"state":{"activitySettings":{"retentionDays":30}}})),["Retention (days): 7 → 30"]);
}

#[test]
fn unrelated_profile_and_unknown_preferences_never_use_request_payload() {
    let result=context("profile",json!({"id":"rig","name":"Rig"})).details(&json!({"command":"profile_save","document":{"id":"rig","name":"REQUEST_SECRET"}}),
        &json!({"ok":true,"state":{"profiles":{"document":{"id":"other","name":"Other"}}}}));
    assert!(result.is_empty());
    for key in ["unknown","remembered"] {
        assert!(context("ui_preferences",json!({})).details(&json!({"command":"ui_preferences_save","key":key}),
            &json!({"ok":true,"state":{"uiPreferences":{"value":{"password":"SENTINEL"}}}})).is_empty());
    }
}

#[test]
fn acknowledged_rename_without_state_is_narrow() {
    assert_eq!(context("profile",json!({"id":"rig","name":"Old"})).details(&json!({"command":"profile_rename","id":"rig","name":"New"}),&json!({"ok":true})),["Name: “Old” → “New”"]);
    assert!(context("profile",json!({"id":"rig","name":"Old"})).details(&json!({"command":"profile_save","document":{"name":"New"}}),&json!({"ok":true})).is_empty());
}

#[test]
fn canonical_removals_and_unselected_profile_rename_are_exact() {
    assert_eq!(context("profile",json!({"id":"rig","name":"Rig"})).details(&json!({"command":"profile_delete","id":"rig"}),
        &json!({"ok":true,"state":{"profiles":{"profiles":[{"id":"other"}],"document":{"id":"other"}}}})),["Profile “Rig” removed."]);
    assert_eq!(context("game",json!({"id":"race","name":"Race"})).details(&json!({"command":"game_remove","gameId":"race"}),
        &json!({"ok":true,"state":{"games":{"catalogue":[]}}})),["Game “Race” removed."]);
    assert_eq!(context("profile",json!({"id":"rig","name":"Old"})).details(&json!({"command":"profile_rename","id":"rig","name":"Submitted"}),
        &json!({"ok":true,"state":{"profiles":{"profiles":[{"id":"rig","name":"Saved"}],"document":{"id":"other"}}}})),["Name: “Old” → “Saved”"]);
}

#[test]
fn canonical_local_settings_integration_and_device_rows_are_supported() {
    assert_eq!(context("configuration",json!({"settings":{"mode":"manual"}})).details(&json!({"command":"profile_local_settings_save"}),
        &json!({"ok":true,"state":{"localSettings":{"settings":{"mode":"automatic"}}}})),["Mode: “manual” → “automatic”"]);
    assert_eq!(context("integration",json!({"state":"stopped"})).details(&json!({"command":"integration_start","gameId":"race"}),
        &json!({"ok":true,"state":{"games":{"catalogue":[{"id":"race","integrationSetup":{"state":"ready"}}]}}})),["State: “stopped” → “ready”"]);
    for state in [json!({"controller":{"id":"wheel","connected":true}}),json!({"sources":[{"id":"wheel","connected":true}]})] {
        assert_eq!(context("controller",json!({"id":"wheel","connected":false})).details(&json!({"command":"connect","id":"wheel"}),
            &json!({"ok":true,"state":state})),["Connected: no → yes"]);
    }
}

#[test]
fn injected_preferences_retention_and_tag_names_preserve_exact_values() {
    assert_eq!(context("ui_preferences",json!(false)).details(&json!({"command":"ui_preferences_save","key":"theme"}),
        &json!({"ok":true,"state":{"activityChangeAfter":true}})),["Dark theme: disabled → enabled"]);
    assert_eq!(context("activity_settings",json!({"retentionDays":7})).details(&json!({"command":"activity_settings_save"}),
        &json!({"ok":true,"state":{"activityChangeAfter":{"retentionDays":30}}})),["Retention (days): 7 → 30"]);
    assert_eq!(context("game",json!({"tagIds":[],"tagNames":{"race":"Racing"}})).details(&json!({"command":"game_tags_save"}),
        &json!({"ok":true,"state":{"activityChangeAfter":{"tagIds":["race"]}}})),["Tags added: Racing"]);
    assert!(context("unknown",Value::Null).details(&json!({}),&json!({"ok":true})).is_empty());
}

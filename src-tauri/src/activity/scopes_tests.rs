use super::Scope;
use serde_json::json;

#[test]
fn command_families_cover_their_exact_subjects() {
    let cases = [
        (json!({"command":"profile_create"}), "profile:new-id"),
        (
            json!({"command":"profile_duplicate"}),
            "profile:copied-profile",
        ),
        (
            json!({"command":"profile_save","document":{"id":"profile-1"}}),
            "profile:profile-1",
        ),
        (
            json!({"command":"profile_delete","id":"profile-2"}),
            "profile:profile-2",
        ),
        (
            json!({"command":"profile_apply"}),
            "profile_session:current",
        ),
        (
            json!({"command":"game_discovery_start"}),
            "discovery:current",
        ),
        (
            json!({"command":"game_integration_settings_save","gameId":"game-1"}),
            "integration:game-1",
        ),
        (json!({"command":"game_save","id":"game-2"}), "game:game-2"),
        (
            json!({"command":"game_remove","gameId":"game-3"}),
            "game:game-3",
        ),
        (json!({"command":"game_rescan"}), "discovery:current"),
        (
            json!({"command":"integration_remove","id":"game-4"}),
            "integration:game-4",
        ),
        (
            json!({"command":"controller_forget","sourceId":"hid-1"}),
            "controller:hid-1",
        ),
        (
            json!({"command":"module_usb_config_save","key":"usb-1"}),
            "module:usb-1",
        ),
        (
            json!({"command":"module_usb_forget","moduleId":"usb-2"}),
            "module:usb-2",
        ),
        (
            json!({"command":"companion_revoke","companion":{"companionId":"tablet-1"}}),
            "companion:tablet-1",
        ),
        (
            json!({"command":"companion_disable","companionId":"tablet-2"}),
            "companion:tablet-2",
        ),
        (
            json!({"command":"forget","gatewayId":"gateway-1","moduleId":"module-1"}),
            "module:gateway-1:module-1",
        ),
        (
            json!({"command":"forget","gatewayId":"gateway-2"}),
            "gateway:gateway-2",
        ),
    ];
    for (request, subject) in cases {
        let scope = Scope::request(&request);
        assert!(scope.covers(subject), "{request} should cover {subject}");
    }
}

#[test]
fn request_key_aliases_resolve_for_profile_game_controller_and_usb_scopes() {
    assert!(
        Scope::request(&json!({"command":"profile_assignments_save","profileId":"profile-a"}))
            .covers("profile:profile-a")
    );
    assert!(
        Scope::request(&json!({"command":"game_tags_save","gameId":"game-a"}))
            .covers("game:game-a")
    );
    assert!(
        Scope::request(&json!({"command":"controller_update","id":"hid-a"}))
            .covers("controller:hid-a")
    );
    assert!(
        Scope::request(&json!({"command":"module_usb_config_save","id":"usb-a"}))
            .covers("module:usb-a")
    );
}

#[test]
fn wildcard_creation_scopes_do_not_watch_other_event_families() {
    let profile = Scope::request(&json!({"command":"profile_import"}));
    assert!(profile.covers("profile:imported"));
    assert!(profile.covers("profile:other"));
    assert!(!profile.covers("profile_session:current"));
    assert!(!profile.covers("game:game-1"));
    assert!(profile.watches("profile"));
    assert!(!profile.watches("profile_session"));
}

#[test]
fn plural_controller_requests_cover_each_entity_but_not_unlisted_entities() {
    let scope = Scope::request(
        &json!({"command":"controller_visibility_restore","sourceIds":["hid-1","hid-2"]}),
    );
    assert!(scope.covers("controller:hid-1"));
    assert!(scope.covers("controller:hid-2"));
    assert!(!scope.covers("controller:hid-3"));
    assert!(scope.watches("controller"));
}

#[test]
fn lifecycle_scope_is_limited_to_profile_session_watermarks() {
    let session = Scope::request(&json!({"command":"profile_stop"}));
    assert!(session.covers("profile_session:current"));
    assert!(session.watches("profile_session"));

    let metadata =
        Scope::request(&json!({"command":"profile_editor_save","document":{"id":"profile-1"}}));
    assert!(metadata.covers("profile:profile-1"));
    assert!(!metadata.watches("profile_session"));
}

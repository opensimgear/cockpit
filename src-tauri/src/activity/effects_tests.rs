use super::*;

#[test]
fn profile_create_and_duplicate_use_acknowledged_new_identity_and_safe_projection() {
    let raw = json!({"id":"created","name":"Race","revision":1,"resources":[{"id":"script","kind":"script","body":"DO_NOT_STORE"}],"assignments":[{"deviceId":"machine"}]});
    let response = json!({"ok":true,"state":{"profiles":{"document":raw}}});
    let effect = expected(&json!({"command":"profile_duplicate","id":"source","name":"Race copy"}), &response, None);
    assert!(effect.contains_key("profile:created"));
    assert!(!effect.contains_key("profile:source"));
    assert!(!effect.values().any(|value| value.contains("DO_NOT_STORE") || value.contains("machine")));
}

#[test]
fn profile_activity_change_after_is_already_projected_and_is_the_terminal_state() {
    let safe = json!({"id":"saved","name":"Safe","resources":[],"assignments":[{"secret":"DO_NOT_COPY"}]});
    let response = json!({"ok":true,"state":{"profiles":{"document":{"id":"saved","name":"raw"},"activityChangeAfter":safe}}});
    let effect = expected(&json!({"command":"profile_save","id":"saved"}), &response, None);
    assert!(effect.contains_key("profile:saved"));
    assert!(!effect.values().any(|value| value.contains("DO_NOT_COPY")));
}

#[test]
fn profile_document_alias_uses_saved_id_from_response_over_source_document_id() {
    let response = json!({"ok":true,"state":{"profiles":{"document":{"id":"new-id","name":"Imported","revision":1,"resources":[]}}}});
    let effect = expected(
        &json!({"command":"profile_import","document":{"id":"source-id","name":"Imported"}}),
        &response,
        None,
    );
    assert!(effect.contains_key("profile:new-id"));
    assert!(!effect.contains_key("profile:source-id"));
}

#[test]
fn profile_delete_claims_only_the_removed_profile() {
    let effect = expected(
        &json!({"command":"profile_delete","id":"removed"}),
        &json!({"ok":true,"state":{"profiles":{"profiles":[]}}}),
        None,
    );
    assert_eq!(effect, BTreeMap::from([("profile:removed".into(), REMOVED.into())]));
}

#[test]
fn failed_mutation_has_no_expected_effect_and_game_save_does_not_claim_derived_state() {
    assert!(expected(&json!({"command":"game_save","gameId":"sim"}), &json!({"ok":false,"error":"failed"}), None).is_empty());
    let game = json!({"id":"sim","name":"Simulator","installPath":"C:/sim","executable":"C:/sim/game.exe","installed":true,"integrationSetup":{"state":"ready"},"profileContext":{"processSession":"live"},"currentVehicle":{"id":"car"}});
    let effect = expected(&json!({"command":"game_save","gameId":"sim"}), &json!({"ok":true,"state":{"games":{"catalogue":[game]}}}), None);
    assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), ["game:sim"]);
}

#[test]
fn failed_profile_controls_claim_only_an_acknowledged_terminal_session_failure() {
    let failed = json!({"ok":false,"error":"activation failed","state":{"profiles":{"profileSession":{"state":"faulted","fault":"Output could not be restored"}}}});
    let effect = expected(&json!({"command":"profile_apply"}), &failed, None);
    assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), ["profile_session:current"]);

    let nonterminal = json!({"ok":false,"state":{"profiles":{"profileSession":{"state":"degraded"}}}});
    assert!(expected(&json!({"command":"profile_apply"}), &nonterminal, None).is_empty());
    assert!(expected(&json!({"command":"profile_delete","id":"p"}), &failed, None).is_empty());
}

#[test]
fn a_stopped_session_terminal_effect_does_not_depend_on_a_profile_owner_id() {
    let first = expected(
        &json!({"command":"profile_stop"}),
        &json!({"ok":true,"state":{"profiles":{"profileSession":{"state":"stopped","appliedSnapshot":{"profileId":"first"}}}}}),
        None,
    );
    let second = expected(
        &json!({"command":"profile_stop"}),
        &json!({"ok":true,"state":{"profiles":{"profileSession":{"state":"stopped","appliedSnapshot":{"profileId":"second"}}}}}),
        None,
    );
    assert_eq!(first, second);
}

#[test]
fn game_discovery_apply_targets_selected_catalogue_entities_and_never_the_whole_catalogue() {
    let catalogue = json!([
        {"id":"one","name":"One","installed":true},
        {"id":"two","name":"Two","installed":true},
    ]);
    let response = json!({"ok":true,"state":{"games":{"catalogue":catalogue,"discovery":{"scanId":"scan","status":"applied","entries":[
        {"id":"entry-1","gameId":"raw-one","action":"add","selected":true},
        {"id":"entry-2","gameId":"two","action":"add","selected":true}
    ]}}}});
    let request = json!({"command":"game_discovery_apply","scanId":"scan","actions":[{"id":"entry-1","gameId":"one","selected":true},{"id":"entry-2","gameId":"two","selected":false}]});
    let effect = expected(&request, &response, None);
    assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), ["discovery:current", "game:one"]);
}

#[test]
fn forgetting_a_missing_gateway_module_claims_its_removal() {
    let effect=expected(&json!({"command":"forget","gatewayId":"pico:one","moduleId":"panel"}),&json!({"ok":true,"state":{"gatewayId":"pico:one","modules":[]}}),None);
    assert_eq!(effect.get("module:pico:one:panel").map(String::as_str),Some("removed"));
}

#[test]
fn discovery_apply_uses_entry_game_ids_defaults_overrides_and_proven_removals() {
    let catalogue = json!([
        {"id":"default-game","name":"Default game","installed":true},
        {"id":"override-on","name":"Override on","installed":true},
    ]);
    let response = json!({"ok":true,"state":{"games":{"catalogue":catalogue,"discovery":{"scanId":"scan","status":"applied","entries":[
        {"id":"entry-default","gameId":"default-game","installationId":"install-default","action":"add","selected":true},
        {"id":"entry-override-off","gameId":"override-off","installationId":"install-off","action":"add","selected":true},
        {"id":"entry-override-on","gameId":"override-on","installationId":"install-on","action":"add","selected":false},
        {"id":"entry-remove","gameId":"removed-game","installationId":"install-remove","action":"remove","selected":false}
    ]}}}});
    let request = json!({"command":"game_discovery_apply","actions":[
        {"id":"entry-override-off","selected":false},
        {"id":"entry-override-on","selected":true},
        {"id":"entry-remove","selected":true}
    ]});
    let effect = expected(&request, &response, None);
    assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), [
        "discovery:current", "game:default-game", "game:override-on", "game:removed-game"
    ]);
    assert_eq!(effect["game:removed-game"], REMOVED);

    let mut observed = Snapshot::default();
    observed.games(response["state"]["games"]["catalogue"].as_array().unwrap());
    observed.discovery(&response["state"]["games"]["discovery"]);
    let observed_states = capture::states(&observed);
    assert_eq!(effect["game:default-game"], observed_states["game:default-game"]);
    assert_eq!(effect["game:override-on"], observed_states["game:override-on"]);
    assert_eq!(effect["discovery:current"], observed_states["discovery:current"]);
}

#[test]
fn discovery_projects_only_the_captured_review_subject() {
    let response = json!({"ok":true,"state":{"games":{"discovery":{"scanId":"scan-1","status":"ready","proposed":2,"libraries":[{"id":"steam","status":"complete"}],"artworkReady":1,"tagged":1,"diagnostics":["Safe diagnostic"]}}}});
    let effect = expected(&json!({"command":"game_discovery_start"}), &response, None);
    assert!(effect.contains_key("discovery:current"));
    assert_eq!(effect.len(), 1);
}

#[test]
fn discovery_identity_ignores_counts_and_diagnostics_as_capture_states_does() {
    let make = |proposed| json!({"ok":true,"state":{"games":{"discovery":{"scanId":"scan-1","status":"ready","proposed":proposed,"libraries":[],"artworkReady":0,"tagged":0,"diagnostics":[]}}}});
    let first = expected(&json!({"command":"game_discovery_start"}), &make(1), None);
    let second = expected(&json!({"command":"game_discovery_start"}), &make(99), None);
    assert_eq!(first, second);
}

#[test]
fn game_rescan_claims_the_game_metadata_entity_not_discovery() {
    let catalogue = json!([{"id":"sim","name":"Simulator","installPath":"C:/sim","installed":true}]);
    let effect = expected(
        &json!({"command":"game_rescan","gameId":"sim"}),
        &json!({"ok":true,"state":{"games":{"catalogue":catalogue,"discovery":{"scanId":"unrelated","status":"ready"}}}}),
        None,
    );
    assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), ["game:sim"]);
}

#[test]
fn integration_mutations_use_the_acknowledged_games_integration_projection() {
    let response = json!({"ok":true,"state":{"games":{"catalogue":[
        {"id":"sim","name":"Simulator","integrationSetup":{"state":"needs-attention","installed":true,"verified":false,"version":"2.0","message":"Retry setup"}},
        {"id":"other","name":"Other","integrationSetup":{"state":"ready","installed":true,"verified":true,"version":"1.0"}}
    ]}}});
    for command in ["game_integration_settings_save", "game_integration_setup", "game_integration_remove", "game_integration_recover", "game_integration_cancel", "integration_start", "dcs_stop"] {
        let effect = expected(&json!({"command":command,"gameId":"sim"}), &response, None);
        assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), ["integration:sim"], "{command}");
    }
}

#[test]
fn integration_response_without_canonical_game_id_does_not_claim_any_subject() {
    let response = json!({"ok":true,"state":{"integrationSetup":{"state":"ready"}}});
    assert!(expected(&json!({"command":"integration_start"}), &response, None).is_empty());
}

#[test]
fn module_and_controller_effects_use_canonical_observer_keys() {
    let module = expected(
        &json!({"command":"module_usb_config_save","key":"usb-slot"}),
        &json!({"ok":true,"state":{"moduleUsb":{"module":{"key":"usb-slot","snapshot":{"connected":true,"present":true,"identity":{"moduleId":"panel"},"route":{"kind":"usb"}}}}}}),
        None,
    );
    assert!(module.contains_key("module:usb-slot"));
    let mut collector = Snapshot::default();
    collector.insert("devices", "module", "usb-slot", "usb-slot", json!({
        "connected":true,"present":true,"identity":{"moduleId":"panel"},"route":{"kind":"usb"}
    }));
    assert_eq!(module, capture::states(&collector));

    let controller = expected(
        &json!({"command":"controller_update","sourceId":"wheel-1"}),
        &json!({"ok":true,"state":{"controllers":{"source":{"sourceId":"wheel-1","name":"Wheel","online":true}}}}),
        None,
    );
    assert!(controller.contains_key("controller:wheel-1"));
}

#[test]
fn nested_companion_payload_aliases_resolve_only_the_targeted_resource() {
    let response = json!({"ok":true,"state":{"companions":{"host":{
        "clients":[
            {"companionId":"screen-a","name":"A","connected":true},
            {"companionId":"screen-b","name":"B","connected":false}
        ],
        "instances":[{"instanceId":"slot-z","companionId":"screen-a"},{"instanceId":"slot-a","companionId":"screen-a"}]
    }}}});
    let effect = expected(
        &json!({"command":"companion_assign","companion":{"companionId":"screen-a","instanceId":"slot-a"}}),
        &response,
        None,
    );
    assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), ["companion:screen-a"]);
    let mut collector = Snapshot::default();
    collector.insert("companions", "companion", "screen-a", "A", json!({
        "connected":true,"instanceId":"slot-a","name":"A"
    }));
    assert_eq!(effect, capture::states(&collector));
}

#[test]
fn flat_module_usb_status_response_matches_actual_slot_projection() {
    let effect = expected(
        &json!({"command":"module_usb_status","key":"usb-slot"}),
        &json!({"ok":true,"state":{"moduleUsb":{"key":"usb-slot","present":true,"snapshot":{"connected":true,"identity":{"moduleId":"panel"},"route":{"kind":"usb"}}}}}),
        None,
    );
    let mut collector = Snapshot::default();
    collector.insert("devices", "module", "usb-slot", "usb-slot", json!({
        "connected":true,"present":true,"identity":{"moduleId":"panel"},"route":{"kind":"usb"}
    }));
    assert_eq!(effect, capture::states(&collector));
}

#[test]
fn nested_instance_id_alias_resolves_to_companion_subject() {
    let response = json!({"ok":true,"state":{"companions":{"host":{
        "clients":[{"companionId":"screen-a","name":"A","connected":false}],
        "instances":[{"instanceId":"slot-a","companionId":"screen-a"}]
    }}}});
    let effect = expected(
        &json!({"command":"companion_disconnect","companion":{"instanceId":"slot-a"}}),
        &response,
        None,
    );
    assert_eq!(effect.keys().map(String::as_str).collect::<Vec<_>>(), ["companion:screen-a"]);
}

#[test]
fn omitted_gateway_request_id_is_resolved_from_acknowledged_gateway_state() {
    let effect = expected(
        &json!({"command":"connect"}),
        &json!({"ok":true,"state":{"gateway":{"id":"pico:SERIAL1"},"connected":true,"running":true,"ports":[{"gatewayId":"pico:SERIAL1"}]}}),
        None,
    );
    assert!(effect.contains_key("gateway:pico:SERIAL1"));
}

#[test]
fn gateway_module_uses_pair_of_canonical_gateway_and_module_ids() {
    let effect = expected(
        &json!({"command":"config_save","moduleId":"panel-a"}),
        &json!({"ok":true,"state":{"gatewayId":"stm32:SERIAL2","modules":[{"moduleId":"panel-a","name":"Panel","connected":true,"present":true,"firmwareVersion":"1.2"}]}}),
        None,
    );
    assert!(effect.contains_key("module:stm32:SERIAL2:panel-a"));
}

#[test]
fn controller_forget_marks_removal_even_when_ack_returns_only_the_remaining_list() {
    let effect = expected(
        &json!({"command":"controller_forget","sourceId":"wheel-1"}),
        &json!({"ok":true,"state":{"controllers":{"sources":[]}}}),
        None,
    );
    assert_eq!(effect.get("controller:wheel-1").map(String::as_str), Some(REMOVED));
}

#[test]
fn game_process_projection_is_scoped_to_launch_target() {
    let target = json!({"id":"sim","name":"Simulator","installed":true,"profileContext":{"processSession":"session-1"}});
    let other = json!({"id":"other","name":"Other","installed":true,"profileContext":{"processSession":"session-2"}});
    let effect = expected(
        &json!({"command":"profile_game_launch","gameId":"sim"}),
        &json!({"ok":true,"state":{"profiles":{"profileSession":{"state":"running","owner":{"profileId":"profile-1"}}},"games":{"catalogue":[target,other]}}}),
        None,
    );
    assert!(effect.contains_key("profile_session:current"));
    assert!(effect.contains_key("game_process:sim"));
    assert!(!effect.contains_key("game_process:other"));
    assert!(!effect.contains_key("game:sim"));
}

use super::event;
use osg_persistence::NewActivityEvent;
use serde_json::Value;

/// Deliberate allowlist: requests, credentials, drafts, action values and errors
/// are never copied into activity. Only safe identities and operation names remain.
pub(super) fn mutation(request: &Value, response: &Value) -> Option<NewActivityEvent> {
    let command = request["command"].as_str()?;
    // Inventory polls persist fresh observation timestamps. Successful cache
    // writes are bookkeeping; explicit Forget is recorded from tombstone changes.
    if command=="ui_preferences_save" && request["key"]=="remembered" && response["ok"]==true { return None; }
    let category = match command {
        "profile_create" | "profile_duplicate" | "profile_import" | "profile_rename" | "profile_delete" | "profile_save" | "profile_editor_save" | "profile_draft_save" | "profile_assignments_save" | "profile_select" | "profile_selection_mode" | "profile_game_rule_save" | "profile_game_rule_remove" | "profile_game_preference_save" | "profile_game_preference_remove" | "profile_apply" | "profile_stop" | "profile_recover" | "profile_game_launch" | "profile_reset" | "profile_input_learn_start" | "profile_input_learn_cancel" | "profile_activation_cancel" | "profile_retired_recovery_recover" | "profile_retired_recovery_acknowledge" | "profile_retired_recovery_provision_recover" | "profile_retired_recovery_provision_cancel" => "profiles",
        "game_add" | "game_save" | "game_remove" | "game_installation_select" | "game_discovery_start" | "game_discovery_apply" | "game_background_save" | "game_tags_save" | "game_tag_definitions_save" => "games",
        "integration_install" | "integration_remove" | "integration_start" | "integration_stop" | "integration_verify" | "game_integration_settings_save" | "game_integration_setup" | "game_integration_remove" | "game_integration_recover" | "game_integration_cancel" | "dcs_install" | "dcs_remove" | "dcs_start" | "dcs_stop" => "integrations",
        "companion_pair" | "companion_pair_offer" | "companion_pair_code" | "companion_pair_cancel" | "companion_pair_confirm" | "companion_approve" | "companion_update" | "companion_assign" | "companion_unassign" | "companion_disconnect" | "companion_takeover" | "companion_forget" | "companion_revoke" | "companion_rename" | "companion_enable" | "companion_disable" | "companion_listener_save" | "companion_server_start" | "companion_server_stop" | "companion_publish_commit" | "companion_prune" | "companion_panel_delete" | "companion_usb_connect" | "companion_usb_disconnect" | "companion_usb_arm" | "companion_usb_disarm" | "companion_usb_forget" | "companion_usb_reconnect" | "companion_usb_scan" | "companion_usb_update_prepare" | "companion_usb_provision" | "companion_usb_flash" | "companion_usb_flash_cancel" | "companion_firmware_flash" => "companions",
        "widget_save" | "widget_library_merge" | "designer_workspace_write" | "ui_preferences_save" | "persistence_ui_write" | "desktop_preferences_save" | "set_autostart" | "set_tray_preferences" | "profile_progress_settings_save" | "desktop_stop" | "desktop_recover" | "mcp_settings_update" | "mcp_credential_create" | "mcp_credential_rotate" | "mcp_credential_revoke" | "local_settings_save" | "profile_local_settings_save" | "profile_desktop_settings_save" | "profile_desktop_preferences_save" | "profile_desktop_configure" | "profile_desktop_stop" | "profile_desktop_recover" | "storage_create" | "storage_remove" | "storage_restore" | "storage_restart" => "configuration",
        "connect" | "disconnect" | "reconnect" | "start" | "stop" | "apply" | "save" | "update-profile" | "claim" | "release" | "forget" | "register" | "unregister" | "enable" | "disable" | "configure" | "config_apply" | "config_apply_save" | "config_save" | "config_discard" | "config_reset" | "module_usb_connect" | "module_usb_disconnect" | "module_usb_claim" | "module_usb_release" | "module_usb_config_apply" | "module_usb_config_apply_save" | "module_usb_config_save" | "module_usb_config_discard" | "module_usb_config_reset" | "controller_update" | "controller_reassign" | "controller_rename" | "controller_forget" | "controller_visibility_apply" | "controller_visibility_launch" | "controller_visibility_restore" | "firmware_flash" | "flash" => "devices",
        "activity_settings_save" => "system",
        _ => return None,
    };
    let failed = response["ok"] != true;
    let identity = |value: &Value| ["id", "profileId", "sourceId", "companionId", "instanceId", "gameId", "moduleId", "gatewayId", "resourceId", "activationId", "learnId", "key"].iter().find_map(|key| value[*key].as_str().map(str::to_owned));
    let document = &response["state"]["profiles"]["document"];
    let id = if command=="game_discovery_start" {response["state"]["games"]["discovery"]["scanId"].as_str().map(str::to_owned)}else{identity(request).or_else(||identity(&request["companion"])).or_else(||identity(&request["document"])).or_else(||identity(document)).or_else(||(command=="game_add").then(||response["state"]["games"]["addedGameId"].as_str().map(str::to_owned)).flatten())}.map(|id| bounded(&id, 256));
    // Only naming operations may carry a user supplied label. No value/settings
    // object is ever recorded, even when a mutation is rejected.
    let saved_name = response["state"]["games"]["catalogue"].as_array().into_iter().flatten().chain(response["state"]["profiles"]["profiles"].as_array().into_iter().flatten()).find(|row|row["id"].as_str()==id.as_deref()).and_then(|row|row["name"].as_str());
    let name = request["name"].as_str().or_else(||request["companion"]["name"].as_str()).or_else(||request["document"]["name"].as_str()).or_else(||document["name"].as_str()).or(saved_name).map(|name| bounded(name, 128));
    let label = name.as_deref().or(id.as_deref()).unwrap_or(category);
    let operation = command.replace('_', " ");
    let message = if failed { format!("{operation} failed for {label}.") } else { format!("{operation} completed for {label}.") };
    let mut captured=event(category, &format!("{command}{}", if failed { "_failed" } else { "" }), if failed { "error" } else { "info" }, message, id, name);
    if command=="ui_preferences_save" {
        let label=match request["key"].as_str().unwrap_or("") {"theme"=>"Appearance","devices-mode"=>"Device view","canvas-navigation"=>"Canvas navigation","canvas-styling"=>"Canvas styling","tray"=>"Tray settings","progress"=>"Progress notifications","windows"=>"Window layout","remembered"=>"Remembered devices",_=>"Preferences"};
        captured.message=format!("{label} {}.",if failed {"could not be saved"} else {"updated"});
        captured.entity_name=Some(label.into());
    }
    captured.source=if request["clientId"].as_str().is_some_and(|id|id.starts_with("cockpit-mcp-")){"mcp"}else{"cockpit"}.into();
    if failed {
        let reason=response["error"].as_str().or_else(||response["error"]["message"].as_str()).unwrap_or("Native operation was rejected. Inspect the related setup or status for details.");
        captured.detail=safe_detail(reason);
    }
    Some(captured)
}
pub(super) fn safe_detail(reason: &str) -> String {
    let lower=reason.to_ascii_lowercase();
    if ["password","passphrase","secret","bearer","authorization","token","ssid","api key","apikey","api_key","cookie","credential","private key"].iter().any(|word|lower.contains(word)){"Native operation needs attention. Inspect the related setup or status for details; sensitive diagnostic content was omitted.".into()}else{bounded(reason,1024)}
}
fn bounded(value: &str, length: usize) -> String { value.chars().filter(|ch| !ch.is_control()).take(length).collect() }

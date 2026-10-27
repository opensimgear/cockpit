use super::event;
use osg_persistence::NewActivityEvent;
use serde_json::Value;

/// Deliberate allowlist: persist safe identities, canonical changes and redacted
/// diagnostics. Raw requests, credentials, drafts and action values stay out.
#[cfg(test)]
pub(super) fn mutation(request: &Value, response: &Value) -> Option<NewActivityEvent> {
    mutation_with_context(request, response, None)
}
pub(super) fn mutation_with_context(request: &Value, response: &Value, context: Option<&super::context::Context>) -> Option<NewActivityEvent> {
    let command = request["command"].as_str()?;
    // Inventory polls persist fresh observation timestamps. Successful cache
    // writes are bookkeeping; explicit Forget is recorded from tombstone changes.
    if command=="ui_preferences_save" && request["key"]=="remembered" && response["ok"]==true { return None; }
    let category=category(command)?;
    mutation_event(request,response,context,command,category)
}
pub(super) fn category(command:&str)->Option<&'static str> {
    Some(match command {
        "profile_create" | "profile_duplicate" | "profile_import" | "profile_rename" | "profile_delete" | "profile_save" | "profile_editor_save" | "profile_draft_save" | "profile_assignments_save" | "profile_select" | "profile_selection_mode" | "profile_game_rule_save" | "profile_game_rule_remove" | "profile_game_preference_save" | "profile_game_preference_remove" | "profile_apply" | "profile_stop" | "profile_recover" | "profile_game_launch" | "profile_reset" | "profile_input_learn_start" | "profile_input_learn_cancel" | "profile_activation_cancel" | "profile_retired_recovery_recover" | "profile_retired_recovery_acknowledge" | "profile_retired_recovery_provision_recover" | "profile_retired_recovery_provision_cancel" => "profiles",
        "game_add" | "game_save" | "game_remove" | "game_installation_select" | "game_discovery_start" | "game_discovery_apply" | "game_background_save" | "game_tags_save" | "game_tag_definitions_save" => "games",
        "integration_install" | "integration_remove" | "integration_start" | "integration_stop" | "integration_verify" | "game_integration_settings_save" | "game_integration_setup" | "game_integration_remove" | "game_integration_recover" | "game_integration_cancel" | "dcs_install" | "dcs_remove" | "dcs_start" | "dcs_stop" => "integrations",
        "companion_pair" | "companion_pair_offer" | "companion_pair_code" | "companion_pair_cancel" | "companion_pair_confirm" | "companion_approve" | "companion_update" | "companion_assign" | "companion_unassign" | "companion_disconnect" | "companion_takeover" | "companion_forget" | "companion_revoke" | "companion_rename" | "companion_enable" | "companion_disable" | "companion_listener_save" | "companion_server_start" | "companion_server_stop" | "companion_publish_commit" | "companion_prune" | "companion_panel_delete" | "companion_usb_connect" | "companion_usb_disconnect" | "companion_usb_arm" | "companion_usb_disarm" | "companion_usb_forget" | "companion_usb_reconnect" | "companion_usb_scan" | "companion_usb_update_prepare" | "companion_usb_provision" | "companion_usb_flash" | "companion_usb_flash_cancel" | "companion_firmware_flash" => "companions",
        "widget_save" | "widget_library_merge" | "designer_workspace_write" | "ui_preferences_save" | "persistence_ui_write" | "desktop_preferences_save" | "set_autostart" | "set_tray_preferences" | "profile_progress_settings_save" | "desktop_stop" | "desktop_recover" | "mcp_settings_update" | "mcp_credential_create" | "mcp_credential_rotate" | "mcp_credential_revoke" | "local_settings_save" | "profile_local_settings_save" | "profile_desktop_settings_save" | "profile_desktop_preferences_save" | "profile_desktop_configure" | "profile_desktop_stop" | "profile_desktop_recover" | "storage_create" | "storage_remove" | "storage_restore" | "storage_restart" => "configuration",
        "connect" | "disconnect" | "reconnect" | "start" | "stop" | "apply" | "save" | "update-profile" | "claim" | "release" | "forget" | "register" | "unregister" | "enable" | "disable" | "configure" | "config_apply" | "config_apply_save" | "config_save" | "config_discard" | "config_reset" | "module_usb_connect" | "module_usb_disconnect" | "module_usb_claim" | "module_usb_release" | "module_usb_config_apply" | "module_usb_config_apply_save" | "module_usb_config_save" | "module_usb_config_discard" | "module_usb_config_reset" | "controller_update" | "controller_reassign" | "controller_rename" | "controller_forget" | "controller_visibility_apply" | "controller_visibility_launch" | "controller_visibility_restore" | "firmware_flash" | "flash" => "devices",
        "activity_settings_save" => "system",
        _ => return None,
    })
}
fn mutation_event(request:&Value,response:&Value,context:Option<&super::context::Context>,command:&str,category:&str)->Option<NewActivityEvent> {
    let failed = response["ok"] != true;
    let identity = |value: &Value| ["id", "profileId", "sourceId", "companionId", "instanceId", "gameId", "moduleId", "gatewayId", "resourceId", "activationId", "learnId", "key"].iter().find_map(|key| value[*key].as_str().map(str::to_owned));
    let document = &response["state"]["profiles"]["document"];
    let id = if category=="devices"&&request["moduleId"].is_string()&&request["gatewayId"].is_string(){Some(format!("{}:{}",request["gatewayId"].as_str().unwrap(),request["moduleId"].as_str().unwrap()))}else if command=="game_discovery_start" {response["state"]["games"]["discovery"]["scanId"].as_str().map(str::to_owned)}else if !failed&&matches!(command,"profile_create"|"profile_duplicate"|"profile_import"){identity(document).or_else(||identity(request))}else{identity(request).or_else(||identity(&request["companion"])).or_else(||identity(&request["document"])).or_else(||identity(document)).or_else(||(command=="game_add").then(||response["state"]["games"]["addedGameId"].as_str().map(str::to_owned)).flatten())}.map(|id| bounded(&id, 256));
    // Only naming operations may carry a user supplied label. No value/settings
    // object is ever recorded, even when a mutation is rejected.
    let saved_name = response["state"]["games"]["catalogue"].as_array().into_iter().flatten().chain(response["state"]["profiles"]["profiles"].as_array().into_iter().flatten()).find(|row|row["id"].as_str()==id.as_deref()).and_then(|row|row["name"].as_str());
    let name = request["name"].as_str().or_else(||request["companion"]["name"].as_str()).or_else(||request["document"]["name"].as_str()).or_else(||document["name"].as_str()).or(saved_name).or_else(||context.and_then(|context|context.before["name"].as_str())).map(|name| bounded(name, 128));
    let message = super::messages::operation(command, name.as_deref(), id.as_deref(), failed);
    let mut captured=event(category, &format!("{command}{}", if failed { "_failed" } else { "" }), if failed { "error" } else { "info" }, message, id, name);
    if command=="ui_preferences_save" {
        let label=match request["key"].as_str().unwrap_or("") {"theme"=>"Appearance","devices-mode"=>"Device view","canvas-navigation"=>"Canvas navigation","canvas-styling"=>"Canvas styling","tray"=>"Tray settings","progress"=>"Progress notifications","windows"=>"Window layout","remembered"=>"Remembered devices",_=>"Preferences"};
        captured.message=format!("{label} {}.",if failed {"could not be saved"} else {"updated"});
        captured.entity_name=Some(label.into());
    }
    captured.source=if request["clientId"].as_str().is_some_and(|id|id.starts_with("cockpit-mcp-")){"mcp"}else{"cockpit"}.into();
    if let Some(context)=context {captured.detail=context.details(request,response).join("\n");}
    if failed {
        let reason=response["error"].as_str().or_else(||response["error"]["message"].as_str()).unwrap_or("Native operation was rejected. Inspect the related setup or status for details.");
        captured.detail=safe_detail(reason);
        // Keep the actionable reason in the compact row as well as the details.
        captured.message=format!("{} {}",captured.message,captured.detail);
        if let Some(name)=context.and_then(|context|context.before["name"].as_str()) {captured.entity_name=Some(bounded(name,128));}
    } else {
        let application=[&response["state"]["profiles"]["saveApplication"],&response["state"]["profiles"]["panelSaveApplication"],&response["state"]["companions"]["panelSaveApplication"]].into_iter().find(|application|application["state"]=="failed");
        if let Some(application)=application {
            let reason=safe_detail(application["error"].as_str().unwrap_or("The saved changes could not be applied to the active profile."));
            captured.message=format!("{} {reason}",captured.message);
            if !captured.detail.is_empty(){captured.detail.push('\n');}
            captured.detail.push_str(&reason);
            captured.event_type=format!("{command}_apply_failed");
            captured.level="warning".into();
        }
    }
    Some(captured)
}
pub(super) fn safe_detail(reason: &str) -> String {
    super::diagnostics::reason(reason)
}
pub(super) fn terminal(event:&mut NewActivityEvent,request:&Value,response:&Value,previous:Option<&(String,String)>) {
    terminal_integration(event,request,response);
    if !matches!(request["command"].as_str(),Some("profile_apply"|"profile_stop"|"profile_recover"|"profile_game_launch")){return;}
    let status=&response["state"]["profiles"]["profileSession"];
    if !status["state"].is_string(){return;}
    let mut names=std::collections::BTreeMap::new();
    for profile in response["state"]["profiles"]["profiles"].as_array().into_iter().flatten(){if let (Some(id),Some(name))=(profile["id"].as_str(),profile["name"].as_str()){names.insert(id.to_owned(),name.to_owned());}}
    if let Some((id,name))=previous {names.insert(id.clone(),name.clone());}
    let mut status=status.clone();
    if status["owner"].is_null()&&status["appliedSnapshot"].is_null(){if let Some((id,_))=previous{status["appliedSnapshot"]=serde_json::json!({"profileId":id});}}
    let mut snapshot=super::Snapshot::default();snapshot.profile_session(status,&names);
    if let Some(observed)=super::capture::diff(&super::Snapshot::default(),&snapshot).pop() {
        if response["ok"]==true {
            event.message=observed.message;event.detail=observed.detail;event.level=observed.level;
            if request["command"]=="profile_recover"{event.message="Profile outputs recovered.".into();}
            if request["command"]=="profile_game_launch"{event.message=format!("Game launch requested. {}",event.message);}
            event.entity_id=observed.entity_id;event.entity_name=observed.entity_name;
        } else if !observed.detail.is_empty()&&!event.detail.contains(&observed.detail){event.detail.push('\n');event.detail.push_str(&observed.detail);}
    }
}
fn terminal_integration(event:&mut NewActivityEvent,request:&Value,response:&Value) {
    let command=request["command"].as_str().unwrap_or("");
    if response["ok"]!=true||!(command.starts_with("game_integration_")||command.starts_with("integration_")||command.starts_with("dcs_")){return;}
    let id=request["gameId"].as_str().or(event.entity_id.as_deref());
    let game=response["state"]["games"]["catalogue"].as_array().into_iter().flatten().find(|game|game["id"].as_str()==id);
    let setup=game.and_then(|game|game.get("integrationSetup")).or_else(||response["state"].get("integrationSetup"));
    if let Some(setup)=setup {
        if let Some(state)=setup["state"].as_str() {
            if !event.detail.is_empty(){event.detail.push('\n');}event.detail.push_str(&format!("Integration state: {}",state.replace('-'," ")));
            if matches!(state,"needs-attention"|"failed"|"recovery-needed"){event.level="warning".into();}
        }
        if let Some(reason)=setup["message"].as_str().filter(|reason|!reason.is_empty()) {
            let reason=safe_detail(reason);if !event.detail.is_empty(){event.detail.push('\n');}event.detail.push_str(&reason);
            if event.level=="warning" {event.message.push(' ');event.message.push_str(&reason);}
        }
    }
}
fn bounded(value: &str, length: usize) -> String { value.chars().filter(|ch| !ch.is_control()).take(length).collect() }

#[cfg(test)]
#[path="audit_details_tests.rs"]
mod detail_tests;

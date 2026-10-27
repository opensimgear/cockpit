//! Human readable activity text. Event types remain machine oriented; messages do not.

pub(crate) fn operation(command: &str, name: Option<&str>, id: Option<&str>, failed: bool) -> String {
    let label = name.or(id).filter(|value| !value.is_empty());
    let quoted = label.map(|value| format!("“{}”", bounded(value, 128)));
    let subject = |noun: &str| quoted.clone().unwrap_or_else(|| noun.to_owned());
    if command=="profile_stop" && label.is_none() {return if failed {"Could not stop the profile session."} else {"Profile session stopped."}.into();}
    let (message, fallback) = match command {
        "profile_create" => (format!("Profile {} created.", subject("profile")), format!("Could not create profile {}.", subject("profile"))),
        "profile_duplicate" => (format!("Profile {} duplicated.", subject("profile")), format!("Could not duplicate profile {}.", subject("profile"))),
        "profile_import" => (format!("Profile {} imported.", subject("profile")), format!("Could not import profile {}.", subject("profile"))),
        "profile_rename" => (format!("Profile renamed to {}.", subject("profile")), format!("Could not rename profile to {}.", subject("profile"))),
        "profile_delete" => (format!("Profile {} deleted.", subject("profile")), format!("Could not delete profile {}.", subject("profile"))),
        "profile_save" | "profile_editor_save" | "profile_draft_save" | "profile_assignments_save" => (format!("Profile {} saved.", subject("profile")), format!("Could not save profile {}.", subject("profile"))),
        "profile_select" => (format!("Profile {} selected.", subject("profile")), format!("Could not select profile {}.", subject("profile"))),
        "profile_selection_mode" => (
            format!("Profile selection settings for {} updated.", subject("profile")),
            format!("Could not update profile selection settings for {}.", subject("profile")),
        ),
        "profile_game_rule_save" | "profile_game_preference_save" => (format!("Game settings saved for profile {}.", subject("profile")), format!("Could not save game settings for profile {}.", subject("profile"))),
        "profile_game_rule_remove" | "profile_game_preference_remove" => (
            format!("Game settings removed from profile {}.", subject("profile")),
            format!("Could not remove game settings from profile {}.", subject("profile")),
        ),
        "profile_apply" => (format!("Activation requested for profile {}.", subject("profile")), format!("Could not request activation for profile {}.", subject("profile"))),
        "profile_stop" => (format!("Profile session {} stopped.", subject("profile")), format!("Could not stop profile session {}.", subject("profile"))),
        "profile_recover" | "profile_retired_recovery_recover" | "profile_retired_recovery_provision_recover" => {
            (format!("Recovery started for profile {}.", subject("profile")), format!("Could not start recovery for profile {}.", subject("profile")))
        }
        "profile_game_launch" => (
            format!("Game launch requested for profile {}.", subject("profile")),
            format!("Could not request game launch for profile {}.", subject("profile")),
        ),
        "profile_reset" => (format!("Profile {} reset.", subject("profile")), format!("Could not reset profile {}.", subject("profile"))),
        "profile_input_learn_start" => (
            format!("Input learning started for profile {}.", subject("profile")),
            format!("Could not start input learning for profile {}.", subject("profile")),
        ),
        "profile_input_learn_cancel" => (
            format!("Input learning cancelled for profile {}.", subject("profile")),
            format!("Could not cancel input learning for profile {}.", subject("profile")),
        ),
        "profile_activation_cancel" => (format!("Activation cancelled for profile {}.", subject("profile")), format!("Could not cancel activation for profile {}.", subject("profile"))),
        "profile_retired_recovery_acknowledge" => (
            format!("Recovery notice acknowledged for profile {}.", subject("profile")),
            format!("Could not acknowledge recovery notice for profile {}.", subject("profile")),
        ),
        "profile_retired_recovery_provision_cancel" => (format!("Profile recovery cancelled for {}.", subject("profile")), format!("Could not cancel profile recovery for {}.", subject("profile"))),
        "game_add" | "game_save" => (format!("Game {} saved.", subject("game")), format!("Could not save game {}.", subject("game"))),
        "game_remove" => (format!("Game {} removed.", subject("game")), format!("Could not remove game {}.", subject("game"))),
        "game_installation_select" => (format!("Installation selected for game {}.", subject("game")), format!("Could not select an installation for game {}.", subject("game"))),
        "game_discovery_start" => ("Game discovery started.".into(), "Could not start game discovery.".into()),
        "game_discovery_apply" => ("Discovered game changes applied.".into(), "Could not apply discovered game changes.".into()),
        "game_background_save" => (
            format!("Background image updated for game {}.", subject("game")),
            format!("Could not update the background image for game {}.", subject("game")),
        ),
        "game_tags_save" | "game_tag_definitions_save" => (format!("Tags updated for game {}.", subject("game")), format!("Could not update tags for game {}.", subject("game"))),
        "integration_install" | "dcs_install" | "game_integration_setup" => (format!("Integration setup started for {}.", subject("game")), format!("Could not set up the integration for {}.", subject("game"))),
        "integration_remove" | "game_integration_remove" | "dcs_remove" => (format!("Integration removed for {}.", subject("game")), format!("Could not remove the integration for {}.", subject("game"))),
        "integration_start" | "dcs_start" => (format!("Integration started for {}.", subject("game")), format!("Could not start the integration for {}.", subject("game"))),
        "integration_stop" | "dcs_stop" => (format!("Integration stopped for {}.", subject("game")), format!("Could not stop the integration for {}.", subject("game"))),
        "integration_verify" => (format!("Integration checked for {}.", subject("game")), format!("Could not check the integration for {}.", subject("game"))),
        "game_integration_settings_save" => (format!("Integration settings saved for {}.", subject("game")), format!("Could not save integration settings for {}.", subject("game"))),
        "game_integration_recover" => (format!("Integration recovery started for {}.", subject("game")), format!("Could not start integration recovery for {}.", subject("game"))),
        "game_integration_cancel" => (format!("Integration setup cancelled for {}.", subject("game")), format!("Could not cancel integration setup for {}.", subject("game"))),
        "companion_pair" | "companion_pair_offer" | "companion_pair_code" | "companion_pair_confirm" => (format!("Companion pairing started for {}.", subject("companion")), format!("Could not pair companion {}.", subject("companion"))),
        "companion_pair_cancel" => (
            format!("Companion pairing cancelled for {}.", subject("companion")),
            format!("Could not cancel pairing for companion {}.", subject("companion")),
        ),
        "companion_approve" => (format!("Companion {} approved.", subject("companion")), format!("Could not approve companion {}.", subject("companion"))),
        "companion_update" => (format!("Companion {} updated.", subject("companion")), format!("Could not update companion {}.", subject("companion"))),
        "companion_assign" => (format!("Companion {} assigned.", subject("companion")), format!("Could not assign companion {}.", subject("companion"))),
        "companion_unassign" => (format!("Companion {} unassigned.", subject("companion")), format!("Could not unassign companion {}.", subject("companion"))),
        "companion_disconnect" => (format!("Companion {} disconnected.", subject("companion")), format!("Could not disconnect companion {}.", subject("companion"))),
        "companion_takeover" => (
            format!("Companion connection taken over for {}.", subject("companion")),
            format!("Could not take over the connection for companion {}.", subject("companion")),
        ),
        "companion_forget" | "companion_revoke" => (format!("Companion access removed for {}.", subject("companion")), format!("Could not remove companion access for {}.", subject("companion"))),
        "companion_rename" => (format!("Companion renamed to {}.", subject("companion")), format!("Could not rename companion to {}.", subject("companion"))),
        "companion_enable" | "companion_disable" => (
            format!("Companion settings updated for {}.", subject("companion")),
            format!("Could not update companion settings for {}.", subject("companion")),
        ),
        "companion_listener_save" => ("Companion listener settings saved.".into(), "Could not save companion listener settings.".into()),
        "companion_server_start" | "companion_server_stop" => ("Companion server settings updated.".into(), "Could not update companion server settings.".into()),
        "companion_publish_commit" => ("Companion changes published.".into(), "Could not publish companion changes.".into()),
        "companion_prune" | "companion_panel_delete" => (format!("Companion data removed for {}.", subject("companion")), format!("Could not remove companion data for {}.", subject("companion"))),
        "companion_usb_connect" | "module_usb_connect" => (format!("USB device {} connected.", subject("device")), format!("Could not connect USB device {}.", subject("device"))),
        "companion_usb_disconnect" | "module_usb_disconnect" => (format!("USB device {} disconnected.", subject("device")), format!("Could not disconnect USB device {}.", subject("device"))),
        "companion_usb_arm" | "module_usb_claim" => (format!("USB device {} is ready for control.", subject("device")), format!("Could not prepare USB device {} for control.", subject("device"))),
        "companion_usb_disarm" | "module_usb_release" => (format!("USB device {} released.", subject("device")), format!("Could not release USB device {}.", subject("device"))),
        "companion_usb_forget" | "controller_forget" => (format!("Device {} forgotten.", subject("device")), format!("Could not forget device {}.", subject("device"))),
        "companion_usb_reconnect" | "reconnect" => (format!("Device {} reconnected.", subject("device")), format!("Could not reconnect device {}.", subject("device"))),
        "companion_usb_scan" => ("USB device scan started.".into(), "Could not start the USB device scan.".into()),
        "companion_usb_update_prepare" => (format!("Firmware update prepared for {}.", subject("device")), format!("Could not prepare the firmware update for {}.", subject("device"))),
        "companion_usb_provision" => (format!("USB device {} configured.", subject("device")), format!("Could not configure USB device {}.", subject("device"))),
        "companion_usb_flash" | "companion_firmware_flash" | "firmware_flash" | "flash" => (format!("Firmware update started for {}.", subject("device")), format!("Could not start the firmware update for {}.", subject("device"))),
        "companion_usb_flash_cancel" => (format!("Firmware update cancelled for {}.", subject("device")), format!("Could not cancel the firmware update for {}.", subject("device"))),
        "widget_save" => (format!("Widget {} saved.", subject("widget")), format!("Could not save widget {}.", subject("widget"))),
        "widget_library_merge" => ("Widget library updated.".into(), "Could not update the widget library.".into()),
        "designer_workspace_write" => ("Designer workspace saved.".into(), "Could not save the designer workspace.".into()),
        "ui_preferences_save"
        | "desktop_preferences_save"
        | "local_settings_save"
        | "profile_local_settings_save"
        | "profile_desktop_settings_save"
        | "profile_desktop_preferences_save"
        | "profile_progress_settings_save"
        | "activity_settings_save"
        | "persistence_ui_write" => ("Settings updated.".into(), "Could not update settings.".into()),
        "set_autostart" => ("Launch at startup setting updated.".into(), "Could not update the launch at startup setting.".into()),
        "set_tray_preferences" => ("System tray settings updated.".into(), "Could not update system tray settings.".into()),
        "profile_desktop_configure" | "configure" | "config_apply" | "config_apply_save" | "config_save" | "module_usb_config_apply" | "module_usb_config_apply_save" | "module_usb_config_save" => {
            (format!("Device configuration saved for {}.", subject("device")), format!("Could not save device configuration for {}.", subject("device")))
        }
        "config_discard" | "module_usb_config_discard" => (
            format!("Device configuration changes discarded for {}.", subject("device")),
            format!("Could not discard device configuration changes for {}.", subject("device")),
        ),
        "config_reset" | "module_usb_config_reset" => (format!("Device configuration reset for {}.", subject("device")), format!("Could not reset device configuration for {}.", subject("device"))),
        "connect" => (format!("Device {} connected.", subject("device")), format!("Could not connect device {}.", subject("device"))),
        "disconnect" => (format!("Device {} disconnected.", subject("device")), format!("Could not disconnect device {}.", subject("device"))),
        "start" => (format!("{} started.", subject("Service")), format!("Could not start {}.", subject("service"))),
        "stop" => (format!("{} stopped.", subject("Service")), format!("Could not stop {}.", subject("service"))),
        "apply" => (format!("Changes applied for {}.", subject("device")), format!("Could not apply changes for {}.", subject("device"))),
        "save" => (format!("Settings saved for {}.", subject("device")), format!("Could not save settings for {}.", subject("device"))),
        "update-profile" => (format!("Profile updated for {}.", subject("device")), format!("Could not update the profile for {}.", subject("device"))),
        "claim" => (format!("Device {} is ready for control.", subject("device")), format!("Could not prepare device {} for control.", subject("device"))),
        "release" => (format!("Device {} released.", subject("device")), format!("Could not release device {}.", subject("device"))),
        "forget" => (format!("Device {} forgotten.", subject("device")), format!("Could not forget device {}.", subject("device"))),
        "register" => (format!("Device {} registered.", subject("device")), format!("Could not register device {}.", subject("device"))),
        "unregister" => (format!("Device {} removed.", subject("device")), format!("Could not remove device {}.", subject("device"))),
        "enable" | "disable" => (format!("Device settings updated for {}.", subject("device")), format!("Could not update device settings for {}.", subject("device"))),
        "controller_update" | "controller_reassign" | "controller_rename" | "controller_visibility_apply" | "controller_visibility_launch" | "controller_visibility_restore" => {
            (format!("Controller settings updated for {}.", subject("device")), format!("Could not update controller settings for {}.", subject("device")))
        }
        "desktop_stop" | "profile_desktop_stop" => ("Desktop integration stopped.".into(), "Could not stop desktop integration.".into()),
        "desktop_recover" | "profile_desktop_recover" => ("Desktop integration recovery started.".into(), "Could not start desktop integration recovery.".into()),
        "mcp_settings_update" => ("MCP access settings updated.".into(), "Could not update MCP access settings.".into()),
        "mcp_credential_create" => ("MCP credential created.".into(), "Could not create the MCP credential.".into()),
        "mcp_credential_rotate" => ("MCP credential rotated.".into(), "Could not rotate the MCP credential.".into()),
        "mcp_credential_revoke" => ("MCP credential revoked.".into(), "Could not revoke the MCP credential.".into()),
        "storage_create" => ("Backup created.".into(), "Could not create the backup.".into()),
        "storage_remove" => ("Backup removed.".into(), "Could not remove the backup.".into()),
        "storage_restore" => ("Backup restored.".into(), "Could not restore the backup.".into()),
        "storage_restart" => ("Storage service restarted.".into(), "Could not restart the storage service.".into()),
        _ => {
            if failed {
                ("Operation failed.".into(), "Operation failed.".into())
            } else {
                ("Settings updated.".into(), "Settings updated.".into())
            }
        }
    };
    if failed {
        fallback
    } else {
        message
    }
}

pub(crate) fn transition(kind: &str, transition: &str, name: &str) -> String {
    if kind=="discovery" {return match transition {"started"=>"Game discovery started.","completed"=>"Game discovery finished.","failed"=>"Game discovery failed.",_=>"Game discovery updated."}.into();}
    let label = format!("“{}”", bounded(name, 128));
    if kind == "integration" {
        return match transition {
            "ready" | "completed" => format!("{} integration is ready.", bounded(name, 128)),
            "failed" => format!("{} integration could not be set up.", bounded(name, 128)),
            "degraded" | "needs_attention" => format!("{} integration needs attention.", bounded(name, 128)),
            _ => format!("{} integration status updated.", bounded(name, 128)),
        };
    }
    let (noun, phrase) = match (kind, transition) {
        ("profile_session", "activated") => ("Profile", "is active"),
        ("profile_session", "degraded") => ("Profile", "is degraded"),
        ("profile_session", "recovered") => ("Profile", "recovered"),
        ("profile_session", "failed") => ("Profile", "needs attention"),
        ("profile_session", "stopped") => ("Profile session", "stopped"),
        ("profile_session", "awaiting_game") => ("Profile", "is waiting for the game"),
        ("game_process", "launched") => ("Game", "started"),
        ("game_process", "exited") => ("Game", "closed"),
        ("game_session", _) => ("Game integration", "session updated"),
        ("discovery", "started") => ("Game discovery", "started"),
        ("discovery", "completed") => ("Game discovery", "finished"),
        ("discovery", "failed") => ("Game discovery", "failed"),
        ("game_vehicle", "changed") => ("Current vehicle", "changed"),
        ("game_vehicle", "cleared") => ("Current vehicle", "cleared"),
        ("gateway" | "module" | "controller", "online") => ("Device", "is online"),
        ("gateway" | "module" | "controller", "offline") => ("Device", "is offline"),
        ("companion", "online") => ("Companion", "is connected"),
        ("companion", "offline") => ("Companion", "disconnected"),
        ("profile", "created") => ("Profile", "created"),
        ("profile", "renamed") => ("Profile", "renamed"),
        ("profile", "removed") => ("Profile", "removed"),
        ("profile", "updated") => ("Profile", "updated"),
        ("game", "created") => ("Game", "added"),
        ("game", "renamed") => ("Game", "renamed"),
        ("game", "removed") => ("Game", "removed"),
        ("game", "updated") => ("Game", "updated"),
        (_, "online") => ("Device", "is online"),
        (_, "offline") => ("Device", "is offline"),
        (_, "failed") => ("Operation", "failed"),
        (_, "updated") => ("Settings", "updated"),
        _ => ("Item", "updated"),
    };
    format!("{noun} {label} {phrase}.")
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().filter(|ch| !ch.is_control()).take(limit).collect()
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;

//! Optimistic local-settings edits share native UI admission and persistence.
//! The protocol adapter cannot bypass profile ownership or provider validation.
use super::*;
use sha2::{Digest, Sha256};

fn revision(value: &Value) -> String {
    format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(value).expect("JSON value")))
}
fn fields(value: &Value, allowed: &[&str]) -> Result<(), String> {
    if value.as_object().is_none_or(|value|value.keys().any(|key|!allowed.contains(&key.as_str()))) {
        return Err("Unknown or unsupported local settings field.".into());
    }
    Ok(())
}
fn game(request: &Value) -> Result<&str, String> {
    request["gameId"].as_str().filter(|game| matches!(*game,
        "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally" |
        "assetto-corsa" | "assetto-corsa-competizione" | "assetto-corsa-evo"))
        .ok_or_else(||"Choose a supported game for local settings.".into())
}

impl GatewayManager {
    fn local_settings_snapshot(&self, request: &Value) -> Result<Value, String> {
        let scope = request["scope"].as_str().ok_or("Choose a local settings scope.")?;
        let (settings, revision) = match scope {
            "game-provider" => self.games.local_provider_settings(game(request)?),
            "desktop" => {
                let settings = if self.profile_native { crate::profile_desktop_settings::Settings::native() }
                    else { crate::profile_desktop_settings::Settings::open(None) };
                let revision = settings.revision();
                (json!({"enabled":settings.value.enabled,"scope":settings.value.scope,"readOnly":settings.error.is_some()}), revision)
            }
            "selection-mode" | "game-rule" | "game-preference" => {
                let captured = self.profile_automatic.settings_snapshot();
                let revision = revision(&captured);
                let saved = &captured["settings"];
                let settings = match scope {
                    "selection-mode" => json!({"mode":saved["mode"],"readOnly":captured["readOnly"]}),
                    "game-rule" => {
                        let game = game(request)?;
                        json!({"rule":saved["rules"].as_array().and_then(|rules|rules.iter().find(|rule|rule["identity"]["gameId"]==game)),"readOnly":captured["readOnly"]})
                    }
                    _ => json!({"profileId":saved["preferredProfiles"][game(request)?],"readOnly":captured["readOnly"]}),
                };
                (settings, revision)
            }
            _ => return Err("Unknown local settings scope.".into()),
        };
        Ok(json!({"scope":scope,"gameId":request.get("gameId"),"revision":revision,"settings":settings,
            "profileSession":self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).status()}))
    }

    /// Called only under Manager's shared profile/game admission locks. Every UI
    /// writer takes the same locks, so revision validation and edit are one turn.
    pub(super) fn local_settings_request(&self, request: &Value) -> Result<Value, String> {
        let save = request["command"] == "profile_local_settings_save";
        fields(request, if save { &["command","scope","gameId","expectedSettingsRevision","expectedActiveOwner","settings"] }
            else { &["command","scope","gameId"] })?;
        let scope = request["scope"].as_str().ok_or("Choose a local settings scope.")?;
        if matches!(scope,"desktop"|"selection-mode") && request.get("gameId").is_some() {
            return Err("This local settings scope does not accept a game identity.".into());
        }
        let current = self.local_settings_snapshot(request)?;
        if !save { return Ok(current); }
        if request["expectedSettingsRevision"].as_str() != current["revision"].as_str() {
            return Err("Local settings revision conflict. Read the current settings and review the change again.".into());
        }
        let expected = request.get("expectedActiveOwner").ok_or("Local settings writes require the exact observed active owner or null.")?;
        let expected: Option<crate::profile_session::RunOwner> = serde_json::from_value(expected.clone())
            .map_err(|_|"Provide the exact observed active owner or null.".to_owned())?;
        self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).check_expected_owner(&expected)?;
        // Reject stale settings/ownership without cancelling a newer native
        // activation. Accepted writes invalidate old preparations while the
        // shared admission locks still prevent their final takeover.
        self.invalidate_learning();
        let settings = &request["settings"];
        match scope {
            "game-provider" => {
                fields(settings,&["connection","userFolder","aircraftModule"])?;
                let mut native = settings.clone();
                native["command"] = json!("game_integration_settings_save");
                native["gameId"] = request["gameId"].clone();
                native["expectedSetupRevision"] = current["revision"].clone();
                self.games.request(&native)?;
            }
            "desktop" => {
                fields(settings,&["enabled","scope"])?;
                // Validate before desktop configuration's explicit revocation.
                if !settings["enabled"].is_boolean() { return Err("Desktop enablement must be a boolean.".into()); }
                serde_json::from_value::<crate::profile_desktop_settings::Scope>(settings["scope"].clone())
                    .map_err(|_|"Choose a supported desktop output scope.".to_owned())?;
                let mut native = settings.clone(); native["command"] = json!("profile_desktop_configure");
                native["expectedSettingsRevision"] = current["revision"].clone();
                self.desktop_request(&native)?;
            }
            "selection-mode" => {
                fields(settings,&["mode"])?;
                self.automatic_request(&json!({"command":"profile_selection_mode","mode":settings["mode"]}))?;
            }
            "game-rule" | "game-preference" => {
                let remove = settings.get("remove") == Some(&Value::Bool(true));
                fields(settings, if remove { &["remove"] } else if scope=="game-rule" { &["profileId","priority","enabled"] } else { &["profileId"] })?;
                let mut native = settings.clone(); native.as_object_mut().unwrap().remove("remove");
                native["gameId"] = request["gameId"].clone();
                if scope=="game-rule" {
                    native["command"] = json!(if remove { "profile_game_rule_remove" } else { "profile_game_rule_save" });
                    self.automatic_request(&native)?;
                } else {
                    native["command"] = json!(if remove { "profile_game_preference_remove" } else { "profile_game_preference_save" });
                    self.game_candidate_request(&native)?;
                }
            }
            _ => unreachable!(),
        }
        self.local_settings_snapshot(request)
    }
}

//! Launch choices are derived from saved profile contexts. A context is only a
//! candidate; the native watcher still supplies every live process proof.
use super::*;
use osg_profiles::activation::GameIdentity;
#[path = "profile_game_orchestration_runtime.rs"]
mod runtime;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchIntent {
    profile_id: String,
    game_id: Option<String>,
}
fn context_profiles<'a>(
    game: &str,
    profiles: &'a [osg_profiles::ProfileDocument],
) -> Vec<&'a osg_profiles::ProfileDocument> {
    let mut matches = profiles
        .iter()
        .filter(|profile| profile.eligibility.game_ids.iter().any(|id| id == game))
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    matches
}
pub(super) fn context_choice<'a>(
    settings: &Settings,
    game: &str,
    profiles: &'a [osg_profiles::ProfileDocument],
    explicit: Option<&str>,
) -> Result<Option<&'a osg_profiles::ProfileDocument>, String> {
    let matches = context_profiles(game, profiles);
    if let Some(id) = explicit {
        return matches
            .into_iter()
            .find(|profile| profile.id == id)
            .map(Some)
            .ok_or_else(|| {
                "The selected profile no longer matches this game's saved context. Choose again."
                    .into()
            });
    }
    if let Some(id) = settings.preferred_profiles.get(game) {
        if let Some(profile) = matches.iter().find(|profile| profile.id == *id) {
            return Ok(Some(*profile));
        }
    }
    match matches.as_slice() {
        [] => Ok(None),
        [profile] => Ok(Some(*profile)),
        _ => Err("Several profiles match this game. Choose a profile before launching, or remember a choice in game settings.".into()),
    }
}
fn launch_intent(
    settings: &Settings,
    requested: &GameIdentity,
    profiles: &[osg_profiles::ProfileDocument],
    explicit: Option<&str>,
) -> Result<LaunchIntent, String> {
    settings.validate()?;
    requested.validate()?;
    let selected = context_choice(settings, &requested.game_id, profiles, explicit)?;
    if selected.is_none()
        && !profiles
            .iter()
            .any(|profile| profile.id == osg_profiles::DEFAULT_PROFILE_ID)
    {
        return Err("The built-in fallback profile is unavailable.".into());
    }
    Ok(LaunchIntent {
        profile_id: selected
            .map(|profile| profile.id.as_str())
            .unwrap_or(osg_profiles::DEFAULT_PROFILE_ID)
            .into(),
        game_id: selected.map(|_| requested.game_id.clone()),
    })
}

impl GatewayManager {
    pub(super) fn context_documents(&self) -> Result<Vec<osg_profiles::ProfileDocument>, String> {
        Ok(self.profiles.lock().unwrap_or_else(|p| p.into_inner())
            .saved_documents().to_vec())
    }
    pub(crate) fn game_candidate_request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Choose a game profile operation.")?;
        let fields: &[&str] = match command {
            "profile_game_candidates" | "profile_game_preference_remove" => &["gameId"],
            "profile_game_preference_save" => &["gameId", "profileId"],
            _ => return Err("Unknown game profile choice operation.".into()),
        };
        if request
            .as_object()
            .ok_or("Game profile choice requires an object.")?
            .keys()
            .any(|key| key != "command" && !fields.contains(&key.as_str()))
        {
            return Err("Unknown field in game profile choice request.".into());
        }
        let game = request["gameId"]
            .as_str()
            .filter(|id| {
                matches!(
                    *id,
                    "dcs" | "assetto-corsa-rally" | "x-plane-11" | "x-plane-12" | "assetto-corsa" | "assetto-corsa-competizione" | "assetto-corsa-evo"
                )
            })
            .ok_or("Choose a supported game context.")?;
        let documents = self.context_documents()?;
        let matches = context_profiles(game, &documents);
        if command == "profile_game_preference_save" {
            let id = request["profileId"]
                .as_str()
                .ok_or("Choose a matching saved profile.")?;
            if !matches.iter().any(|profile| profile.id == id) {
                return Err("Choose a profile with this saved game context.".into());
            }
            self.profile_automatic.change(|settings| {
                settings.preferred_profiles.insert(game.into(), id.into());
                Ok(())
            })?;
        } else if command == "profile_game_preference_remove" {
            self.profile_automatic.change(|settings| {
                settings.preferred_profiles.remove(game);
                Ok(())
            })?;
        }
        let (settings, _, _) = self.profile_automatic.config();
        let preferred = settings
            .preferred_profiles
            .get(game)
            .filter(|id| matches.iter().any(|profile| profile.id == **id));
        let mut listed = self
            .profiles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .request(&json!({"command":"profile_list"}))?;
        let summaries = listed["profiles"]
            .as_array_mut()
            .ok_or("The saved profile list is unavailable.")?;
        summaries.retain(|summary| matches.iter().any(|profile| summary["id"] == profile.id));
        summaries.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
                .then(a["id"].as_str().cmp(&b["id"].as_str()))
        });
        Ok(json!({"gameId":game,"profiles":summaries,"preferredProfileId":preferred}))
    }
}

#[cfg(test)]
#[path = "profile_game_orchestration_tests.rs"]
mod tests;

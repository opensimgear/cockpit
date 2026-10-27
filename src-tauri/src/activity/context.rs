//! Mutation details use captured before values and acknowledged native state.
use super::changes;
use serde_json::Value;

pub(crate) struct Context {
    pub(crate) kind: &'static str,
    pub(crate) before: Value,
}

impl Context {
    pub(crate) fn details(&self, request: &Value, response: &Value) -> Vec<String> {
        if response["ok"] != true { return vec![]; }
        let command = request["command"].as_str().unwrap_or("");
        // Gateway may attach an already-safe canonical snapshot solely to its
        // private audit response. In particular, profiles are already projected.
        if let Some(after) = response["state"].get("activityChangeAfter") {
            if command == "ui_preferences_save" { return preference_details(request["key"].as_str().unwrap_or(""),&self.before,after); }
            if command == "activity_settings_save" {
                return scalar_change("Retention (days)",self.before.get("retentionDays").unwrap_or(&self.before),after.get("retentionDays").unwrap_or(after));
            }
            let mut after = after.clone();
            if self.kind=="game" && after.get("tagNames").is_none() {
                if let Some(tags)=self.before.get("tagNames") { after["tagNames"]=tags.clone(); }
            }
            if self.kind=="game" { after=changes::game_snapshot(&after); }
            return changes::describe(self.kind,Some(&self.before),&after);
        }
        if command == "ui_preferences_save" {
            let Some(after) = response["state"]["uiPreferences"].get("value") else { return vec![]; };
            return preference_details(request["key"].as_str().unwrap_or(""), &self.before, after);
        }
        if command == "activity_settings_save" {
            let Some(after) = response["state"]["activitySettings"].get("retentionDays") else { return vec![]; };
            let before = self.before.get("retentionDays").unwrap_or(&self.before);
            return scalar_change("Retention (days)", before, after);
        }
        match self.kind {
            "profile" => self.profile_details(request, response),
            "game" => self.game_details(request, response),
            "configuration" => {
                // Local settings responses carry the normalized, saved settings.
                let after = response["state"]["localSettings"].get("settings")
                    .or_else(||response["state"].get("desktopPreferences"));
                after.map(|after|changes::describe("configuration",Some(self.before.get("settings").unwrap_or(&self.before)),after)).unwrap_or_default()
            }
            "integration" => self.integration_details(request,response),
            "companion" | "device" | "controller" | "module" | "gateway" => {
                self.entity_details(request,response)
            }
            _ => vec![],
        }
    }

    fn profile_details(&self, request: &Value, response: &Value) -> Vec<String> {
        let profiles = &response["state"]["profiles"];
        let id = identity(request).or_else(||self.before["id"].as_str());
        let document = &profiles["document"];
        if request["command"] == "profile_delete" {
            if let (Some(id),Some(rows)) = (id,profiles["profiles"].as_array()) {
                if !rows.iter().any(|row|row["id"].as_str()==Some(id)) {
                    return vec![format!("Profile “{}” removed.",self.before["name"].as_str().unwrap_or(id))];
                }
            }
            return vec![];
        }
        if document.is_object() && id.is_none_or(|id|document["id"].as_str()==Some(id)) {
            let mut after = changes::profile_snapshot(document);
            // Assignments are local state, outside the portable document.
            if let Some(assignments) = profiles.get("assignments").or_else(||self.before.get("assignments")) { after["assignments"] = assignments.clone(); }
            return changes::describe("profile",Some(&self.before),&after);
        }
        if request["command"] == "profile_rename" {
            if let Some(row) = profiles["profiles"].as_array().into_iter().flatten().find(|row|row["id"].as_str()==id) {
                return scalar_change("Name",&self.before["name"],&row["name"]);
            }
            return rename_fallback(request,response,&self.before);
        }
        vec![]
    }

    fn game_details(&self, request: &Value, response: &Value) -> Vec<String> {
        let games = &response["state"]["games"];
        let id = request["gameId"].as_str().or_else(||games["addedGameId"].as_str())
            .or_else(||self.before["id"].as_str());
        let Some(rows) = games["catalogue"].as_array() else { return vec![]; };
        let Some(row) = rows.iter().find(|row|row["id"].as_str()==id) else {
            return if request["command"]=="game_remove" && id.is_some() {
                vec![format!("Game “{}” removed.",self.before["name"].as_str().or(id).unwrap_or("Game"))]
            } else { vec![] };
        };
        let mut after = row.clone();
        let tags = games.get("tags").filter(|tags|tags.is_object()).or_else(||self.before.get("tagNames"));
        if let Some(tags) = tags { after["tagNames"] = tags.clone(); }
        changes::describe("game",Some(&self.before),&changes::game_snapshot(&after))
    }

    fn integration_details(&self, request: &Value, response: &Value) -> Vec<String> {
        let id = request["gameId"].as_str().or_else(||self.before["id"].as_str());
        let game = response["state"]["games"]["catalogue"].as_array().into_iter().flatten()
            .find(|row|row["id"].as_str()==id);
        let after = game.and_then(|game|game.get("integrationSetup"))
            .or_else(||response["state"].get("integrationSetup"));
        after.map(|after|changes::describe("integration",Some(&self.before),after)).unwrap_or_default()
    }

    fn entity_details(&self, request: &Value, response: &Value) -> Vec<String> {
        let id = identity(request).or_else(||self.before["id"].as_str());
        let state = &response["state"];
        for key in ["companion","controller","module","gateway"] {
            if let Some(after) = state.get(key).filter(|value|value.is_object()) {
                if id.is_none_or(|id|identity(after)==Some(id)) {
                    return changes::describe(self.kind,Some(&self.before),after);
                }
            }
        }
        for key in ["companions","controllers","modules","sources"] {
            if let Some(after) = state[key].as_array().into_iter().flatten().find(|row|identity(row)==id) {
                return changes::describe(self.kind,Some(&self.before),after);
            }
        }
        if matches!(request["command"].as_str(),Some("companion_rename"|"controller_rename")) {
            rename_fallback(request,response,&self.before)
        } else { vec![] }
    }
}

fn identity(value: &Value) -> Option<&str> {
    ["profileId","id","companionId","sourceId","moduleId","gatewayId"].iter().find_map(|key|value[*key].as_str())
        .or_else(||value["document"]["id"].as_str())
}

fn rename_fallback(request: &Value, response: &Value, before: &Value) -> Vec<String> {
    // A narrow acknowledged rename can omit state; never infer graph/settings
    // edits from submitted documents or settings bodies.
    if response.get("state").is_none() && request["name"].is_string() {
        scalar_change("Name",&before["name"],&request["name"])
    } else { vec![] }
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("not set".into()),
        Value::String(value) => Some(format!("“{value}”")),
        Value::Bool(value) => Some(if *value {"enabled"}else{"disabled"}.into()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn scalar_change(label: &str, before: &Value, after: &Value) -> Vec<String> {
    if before == after { return vec![]; }
    match (scalar(before),scalar(after)) {
        (Some(before),Some(after)) => vec![format!("{label}: {before} → {after}")],
        _ => vec![],
    }
}

fn preference_details(key: &str, before: &Value, after: &Value) -> Vec<String> {
    if key=="remembered" { return vec![]; }
    if key=="theme" { return scalar_change("Dark theme",before,after); }
    if key=="devices-mode" { return scalar_change("Device view",before,after); }
    let fields: &[(&str,&str)] = match key {
        "canvas-navigation" => &[("wheelAction","Mouse wheel")],
        "canvas-styling" => &[("arrowStyle","Arrow style"),("cornerRadius","Corner radius")],
        "tray" => &[("closeToTray","Close to tray"),("minimizeToTray","Minimize to tray")],
        "progress" => &[("delivery","Notification delivery"),("position","Notification position")],
        "windows" => &[("x","Window X"),("y","Window Y"),("width","Window width"),("height","Window height"),("scaleFactor","Window scale"),("maximized","Window maximized")],
        _ => &[],
    };
    fields.iter().flat_map(|(key,label)|scalar_change(label,&before[*key],&after[*key])).collect()
}

#[cfg(test)]
#[path="context_tests.rs"]
mod tests;

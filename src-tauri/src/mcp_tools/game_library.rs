//! MCP adapter for discovery-first game installation and catalogue management.
use super::*;

impl Tools {
    pub(super) fn game_discovery_start(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let data = self.native(
            "game_discovery_start",
            select(args, &["paths"]),
            client,
            Some("games"),
        )?;
        let discovery = data.get("discovery").ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The native game discovery start returned no discovery snapshot.",
                false,
            )
        })?;
        let scan_id = discovery["scanId"].as_str().ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The discovery snapshot has no scan ID.",
                false,
            )
        })?;
        let status = discovery["status"].as_str().ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The discovery snapshot has no status.",
                false,
            )
        })?;
        let libraries = discovery["libraries"].as_array().ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The discovery snapshot has no library progress.",
                false,
            )
        })?;
        let diagnostics = discovery["diagnostics"].as_array().ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The discovery snapshot has no diagnostics.",
                false,
            )
        })?;
        Ok(json!({
            "scanId":scan_id,
            "status":status,
            "firstStart":discovery["firstStart"].as_bool().unwrap_or(false),
            "libraries":libraries,
            "diagnostics":diagnostics
        }))
    }

    pub(super) fn game_discovery_status(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let scan_id = args["scanId"].as_str().ok_or_else(|| {
            failure(
                "invalid_arguments",
                "scanId must identify a discovery scan.",
                false,
            )
        })?;
        let data = self.native("game_get_state", json!({}), client, Some("games"))?;
        let discovery = data.get("discovery").ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The native game state returned no discovery snapshot.",
                false,
            )
        })?;
        if discovery["scanId"].as_str() != Some(scan_id) {
            return Err(failure(
                "stale_discovery",
                "The requested discovery scan is no longer current. Start or inspect the current scan before applying changes.",
                false,
            ));
        }
        let entries = discovery["entries"].as_array().ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The native game discovery snapshot returned no entries.",
                false,
            )
        })?;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(32) as usize;
        let total = entries.len();
        let mut page = discovery.clone();
        let mut page_entries = entries
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        for entry in &mut page_entries {
            if let Some(object) = entry.as_object_mut() {
                let has_artwork = object.remove("image").is_some_and(|image| !image.is_null());
                if has_artwork {
                    object.insert("hasArtwork".into(), json!(true));
                }
            }
        }
        page["entries"] = json!(page_entries);
        page["offset"] = json!(offset);
        page["total"] = json!(total);
        let next = offset + page["entries"].as_array().map_or(0, Vec::len);
        page["nextOffset"] = if next < total {
            json!(next)
        } else {
            Value::Null
        };
        Ok(page)
    }

    pub(super) fn game_discovery_apply(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let data = self.native(
            "game_discovery_apply",
            select(args, &["scanId", "actions"]),
            client,
            Some("games"),
        )?;
        let discovery = data.get("discovery").ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The native game discovery apply returned no discovery snapshot.",
                false,
            )
        })?;
        let scan_id = discovery["scanId"].as_str().ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The discovery snapshot has no scan ID.",
                false,
            )
        })?;
        let status = discovery["status"].as_str().ok_or_else(|| {
            failure(
                "invalid_native_result",
                "The discovery snapshot has no status.",
                false,
            )
        })?;
        let game_count = data["catalogue"]
            .as_array()
            .ok_or_else(|| {
                failure(
                    "invalid_native_result",
                    "The native game catalogue is malformed.",
                    false,
                )
            })?
            .len();
        Ok(json!({"scanId":scan_id,"status":status,"gameCount":game_count}))
    }

    pub(super) fn game_background_save(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let data = self.native("game_background_save", select(args, &["gameId", "imageData", "crop", "path", "positionX", "positionY"]), client, Some("games"))?;
        metadata_receipt(&data, args["gameId"].as_str().unwrap_or(""), false)
    }

    pub(super) fn game_save(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let data = self.native(
            "game_save",
            select(args, &["gameId", "name", "installationId", "executable"]),
            client,
            Some("games"),
        )?;
        metadata_receipt(&data, args["gameId"].as_str().unwrap_or(""), false)
    }

    pub(super) fn game_remove(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let data = self.native(
            "game_remove",
            select(args, &["gameId"]),
            client,
            Some("games"),
        )?;
        metadata_receipt(&data, args["gameId"].as_str().unwrap_or(""), true)
    }

    pub(super) fn game_installation_select(
        &self,
        args: &Value,
        client: &str,
    ) -> Result<Value, Value> {
        let data = self.native(
            "game_installation_select",
            select(args, &["gameId", "installationId"]),
            client,
            Some("games"),
        )?;
        metadata_receipt(&data, args["gameId"].as_str().unwrap_or(""), false)
    }
}

fn metadata_receipt(data: &Value, game_id: &str, remove: bool) -> Result<Value, Value> {
    let catalogue = data["catalogue"].as_array().ok_or_else(|| {
        failure(
            "invalid_native_result",
            "The native game catalogue is malformed.",
            false,
        )
    })?;
    let game = catalogue
        .iter()
        .find(|game| game["id"].as_str() == Some(game_id));
    Ok(json!({
        "gameId":game_id,
        "name":game.and_then(|game|game.get("name")).cloned().unwrap_or(Value::Null),
        "selectedInstallationId":game.and_then(|game|game.get("selectedInstallationId")).cloned().unwrap_or(Value::Null),
        "removed":remove && game.is_none()
    }))
}

#[cfg(test)]
#[path = "game_library_tests.rs"]
mod tests;

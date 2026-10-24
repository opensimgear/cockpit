//! Registry edits preserve portable profile references. Native process/session
//! admission remains the authority for scoped output and automatic activation.
use super::*;
use std::fs;
use crate::game_installations::{is_custom_id, CustomGame};

/// Native-only proposal. File discovery and image decoding happen before the
/// manager takes profile/game admission; a proposal can be committed once.
pub(crate) struct PreparedMetadata {
    runtime: Arc<Inner>,
    captured: crate::game_installations::Settings,
    next: crate::game_installations::Settings,
    catalogue: Vec<Value>,
    library: Option<crate::game_library::Scan>,
    added_game_id: Option<String>,
    staging: crate::game_artwork::Staging,
}

fn fields(request: &Value, allowed: &[&str]) -> Result<(), String> {
    if !known(request, &[&["command", "clientId"][..], allowed].concat()) {
        return Err("Unknown game library request field.".into());
    }
    Ok(())
}
fn position(request: &Value, key: &str, existing: f64) -> Result<f64, String> {
    match request.get(key) {
        None => Ok(existing),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite() && (0.0..=100.0).contains(number))
            .ok_or_else(|| "Background crop positions must be between 0 and 100.".into()),
    }
}
fn selected_executable(
    runtime: &GamesRuntime,
    id: &str,
    value: &Value,
) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(path) => {
            runtime.inner.platform.validate(id, path)?;
            Ok(Some(
                fs::canonicalize(path)
                    .ok()
                    .and_then(|path| path.to_str().map(str::to_owned))
                    .unwrap_or_else(|| path.clone()),
            ))
        }
        _ => Err("Choose an executable path or explicit null for an unavailable game.".into()),
    }
}
pub(super) fn unique_executable(
    catalogue: &[Value],
    id: &str,
    executable: Option<&str>,
) -> Result<(), String> {
    if let Some(executable) = executable {
        let canonical = fs::canonicalize(executable).ok();
        let normalized = crate::game_discovery::normalized_path(
            canonical
                .as_deref()
                .and_then(Path::to_str)
                .unwrap_or(executable),
        );
        if catalogue.iter().any(|game| {
            game["id"] != id
                && game["executable"]
                    .as_str()
                    .is_some_and(|path| crate::game_discovery::normalized_path(path) == normalized)
        }) {
            return Err("This executable already belongs to a game in your library. Edit that game instead.".into());
        }
    }
    Ok(())
}
impl GamesRuntime {
    pub(super) fn metadata_request(&self, request: &Value) -> Result<(), String> {
        self.metadata_request_with_scan(request, crate::game_library::scan)
    }
    // A native scanner is supplied here so fixture tests exercise the same
    // reviewed import transaction without depending on this machine's clients.
    fn metadata_request_with_scan(
        &self,
        request: &Value,
        scan_library: impl FnOnce() -> crate::game_library::Scan,
    ) -> Result<(), String> {
        if !request.is_object()
            || serde_json::to_vec(request)
                .map_err(|error| error.to_string())?
                .len()
                > 65536
        {
            return Err("Invalid game metadata request.".into());
        }
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The game metadata runtime has closed.".into());
        }
        self.inner.guard_installation_metadata()?;
        let command = request["command"].as_str().unwrap_or("");
        if command == "game_library_scan" {
            fields(request, &[])?;
            let scan = scan_library();
            let mut settings = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            settings.library_candidates = scan.candidates;
            settings.library_diagnostics = scan.diagnostics;
            settings.added_game_id = None;
            return Ok(());
        }
        let prepared = self.prepare_metadata_with_scan(request, scan_library)?;
        self.commit_metadata(prepared)?;
        self.finish_metadata();
        Ok(())
    }
    pub(crate) fn prepare_metadata(&self, request: &Value) -> Result<PreparedMetadata, String> {
        self.prepare_metadata_with_scan(request, crate::game_library::scan)
    }
    fn prepare_metadata_with_scan(
        &self,
        request: &Value,
        scan_library: impl FnOnce() -> crate::game_library::Scan,
    ) -> Result<PreparedMetadata, String> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The game metadata runtime has closed.".into());
        }
        if !request.is_object()
            || serde_json::to_vec(request)
                .map_err(|error| error.to_string())?
                .len()
                > 65536
        {
            return Err("Invalid game metadata request.".into());
        }
        self.inner.guard_installation_metadata()?;
        self.guard_profile_takeover()?;
        let staging = crate::game_artwork::staging(self.inner.installations.root())?;
        let (captured, catalogue) = {
            let settings = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            (settings.saved.clone(), settings.catalogue.clone())
        };
        let mut next = captured.clone();
        let mut added = None;
        let mut library = None;
        let command = request["command"].as_str().unwrap_or("");
        match command {
            "game_categories_save" => {
                fields(request, &["categories", "expectedCategories"])?;
                if request["expectedCategories"] != json!(captured.categories) {
                    return Err("Categories changed elsewhere. Close this dialog and try again.".into());
                }
                next.categories = serde_json::from_value(request["categories"].clone())
                    .map_err(|_| "Choose a valid category list.")?;
                // Renaming preserves IDs; deleting a category leaves games uncategorized.
                next.game_categories.retain(|_, category| next.categories.contains_key(category));
            }
            "game_category_save" => {
                fields(request, &["gameId", "categoryId"])?;
                let id = request["gameId"].as_str()
                    .filter(|id| catalogue.iter().any(|game| game["id"] == *id))
                    .ok_or("Choose an existing game.")?;
                match request.get("categoryId") {
                    Some(Value::Null) => { next.game_categories.remove(id); }
                    Some(Value::String(category)) if next.categories.contains_key(category) => {
                        next.game_categories.insert(id.into(), category.clone());
                    }
                    _ => return Err("Choose an existing category or Uncategorized.".into()),
                }
            }
            "game_custom_save" => {
                fields(request, &["gameId", "name", "executable", "categoryId"])?;
                let id = match request.get("gameId") {
                    None => format!("custom-{}", uuid::Uuid::new_v4()),
                    Some(Value::String(id))
                        if is_custom_id(id) && next.custom_games.contains_key(id) =>
                    {
                        id.clone()
                    }
                    _ => return Err("Choose an existing custom game to edit.".into()),
                };
                let name = request["name"]
                    .as_str()
                    .ok_or("Give this game a name.")?
                    .trim()
                    .to_owned();
                let executable = match request.get("executable") {
                    Some(value) => selected_executable(self, &id, value)?,
                    None => next
                        .custom_games
                        .get(&id)
                        .ok_or("Choose an executable or explicit null.")?
                        .executable
                        .clone(),
                };
                unique_executable(&catalogue, &id, executable.as_deref())?;
                let previous = next.custom_games.get(&id).cloned();
                next.custom_games.insert(
                    id.clone(),
                    CustomGame {
                        name,
                        executable,
                        library: previous.as_ref().and_then(|game| game.library.clone()),
                        default_background: previous.and_then(|game| game.default_background),
                    },
                );
                if let Some(category) = request.get("categoryId") {
                    match category {
                        Value::Null => { next.game_categories.remove(&id); }
                        Value::String(category) if next.categories.contains_key(category) => {
                            next.game_categories.insert(id.clone(), category.clone());
                        }
                        _ => return Err("Choose an existing category or Uncategorized.".into()),
                    }
                }
                added = Some(id);
            }
            "game_custom_remove" => {
                fields(request, &["gameId"])?;
                let id = request["gameId"]
                    .as_str()
                    .ok_or("Choose a custom game to remove.")?;
                if next.custom_games.remove(id).is_none() {
                    return Err("Choose an existing custom game to remove.".into());
                }
                next.backgrounds.remove(id);
                next.default_backgrounds.remove(id);
                next.game_categories.remove(id);
            }
            "game_background_save" => {
                fields(request, &["gameId", "path", "positionX", "positionY", "imageData", "crop"])?;
                let id = request["gameId"]
                    .as_str()
                    .filter(|id| catalogue.iter().any(|game| game["id"] == *id))
                    .ok_or("Choose an existing game background.")?;
                if let Some(data) = request.get("imageData") {
                    if request.get("path").is_some() || request.get("positionX").is_some() || request.get("positionY").is_some() {
                        return Err("Choose a preview crop or a legacy image position, not both.".into());
                    }
                    let crop = serde_json::from_value(request["crop"].clone()).map_err(|_| "Choose a valid tile crop.")?;
                    let background = crate::game_artwork::import_preview(self.inner.installations.root(), data.as_str().ok_or("Load a valid image preview.")?, crop)?;
                    next.backgrounds.insert(id.into(), background);
                } else if request.get("crop").is_some() {
                    return Err("Load the image preview before saving its crop.".into());
                } else if request.get("path") == Some(&Value::Null) {
                    next.backgrounds.remove(id);
                } else {
                    let existing = next
                        .backgrounds
                        .get(id)
                        .or_else(|| next.default_backgrounds.get(id))
                        .or_else(|| {
                            next.custom_games
                                .get(id)
                                .and_then(|game| game.default_background.as_ref())
                        });
                    let x = position(
                        request,
                        "positionX",
                        existing.map_or(50.0, |background| background.position_x),
                    )?;
                    let y = position(
                        request,
                        "positionY",
                        existing.map_or(50.0, |background| background.position_y),
                    )?;
                    let background=match request.get("path") {
                        Some(Value::String(path))=>crate::game_artwork::import(self.inner.installations.root(),path,x,y)?,
                        None=>{let mut background=existing.cloned().ok_or("Choose a background image before setting its crop.")?;background.position_x=x;background.position_y=y;background.crop=None;background},
                        _=>return Err("Choose a local image path, omit it to adjust crop, or reset with null.".into()),
                    };
                    next.backgrounds.insert(id.into(), background);
                }
            }
            "game_library_import" => {
                fields(request, &["candidateId", "executable"])?;
                let candidate_id = request["candidateId"]
                    .as_str()
                    .ok_or("Choose an installed library game.")?;
                // Re-read native manifests rather than trusting the browser's
                // cached executable candidates or a removed installation.
                let scan = scan_library();
                let candidate = scan
                    .candidates
                    .iter()
                    .find(|candidate| candidate.id == candidate_id)
                    .ok_or("The library game is no longer installed. Scan again.")?;
                let mut id = candidate
                    .game_id
                    .clone()
                    .or_else(|| {
                        next.custom_games
                            .iter()
                            .find(|(_, game)| {
                                game.library.as_ref().is_some_and(|source| {
                                    source.client == candidate.client
                                        && source.app_id == candidate.app_id
                                })
                            })
                            .map(|(id, _)| id.clone())
                    })
                    .unwrap_or_else(|| format!("custom-{}", uuid::Uuid::new_v4()));
                let selected = match request.get("executable") {
                    Some(value) => selected_executable(self, &id, value)?,
                    None if candidate.executable_candidates.len() == 1 => {
                        selected_executable(self, &id, &json!(candidate.executable_candidates[0]))?
                    }
                    None => None,
                };
                if candidate.game_id.is_none() && !next.custom_games.contains_key(&id) {
                    if let Some(existing) = selected.as_deref().and_then(|path| {
                        catalogue.iter().find(|game| {
                            game["custom"] == true
                                && game["executable"].as_str().is_some_and(|old| {
                                    crate::game_discovery::normalized_path(old)
                                        == crate::game_discovery::normalized_path(path)
                                })
                        })
                    }) {
                        id = existing["id"].as_str().unwrap().to_owned();
                    }
                }
                if is_custom_id(&id) {
                    unique_executable(&catalogue, &id, selected.as_deref())?;
                    let previous = next.custom_games.get(&id).cloned();
                    let executable = if request.get("executable").is_some() {
                        selected
                    } else {
                        selected
                            .or_else(|| previous.as_ref().and_then(|game| game.executable.clone()))
                    };
                    next.custom_games.insert(
                        id.clone(),
                        CustomGame {
                            name: previous
                                .as_ref()
                                .map(|game| game.name.clone())
                                .unwrap_or_else(|| candidate.name.clone()),
                            executable,
                            library: Some(candidate.source()),
                            default_background: previous.and_then(|game| game.default_background),
                        },
                    );
                } else if let Some(selected) = selected {
                    unique_executable(&catalogue, &id, Some(&selected))?;
                    next.manual_installations.insert(id.clone(), selected);
                }
                if let Some(artwork) = &candidate.artwork {
                    if let Some(path) = artwork.to_str() {
                        if let Ok(background) = crate::game_artwork::import(
                            self.inner.installations.root(),
                            path,
                            50.0,
                            50.0,
                        ) {
                            next.default_backgrounds.insert(id.clone(), background);
                        }
                    }
                }
                library = Some(scan);
                added = Some(id);
            }
            _ => return Err("Unknown game library operation.".into()),
        }
        next.validate()?;
        let catalogue =
            crate::game_discovery::merged_catalogue(self.inner.platform.as_ref(), &next);
        Ok(PreparedMetadata {
            runtime: self.inner.clone(),
            captured,
            next,
            catalogue,
            library,
            added_game_id: added,
            staging,
        })
    }
    pub(crate) fn commit_metadata(&self, prepared: PreparedMetadata) -> Result<(), String> {
        if !Arc::ptr_eq(&prepared.runtime, &self.inner) {
            return Err("The prepared game metadata belongs to another native runtime.".into());
        }
        let _admission = self.admission.try_lock().map_err(|_| {
            "Game metadata is busy. Retry this change after the current operation finishes."
                .to_owned()
        })?;
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The game metadata runtime has closed.".into());
        }
        self.inner.guard_installation_metadata()?;
        self.guard_profile_takeover()?;
        let mut settings = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if settings.saved != prepared.captured {
            return Err("Game registrations changed while this update was being prepared. Refresh and retry.".into());
        }
        self.inner.installations.save(&prepared.next)?;
        settings.saved = prepared.next;
        settings.catalogue = prepared.catalogue;
        if let Some(scan) = prepared.library {
            settings.library_candidates = scan.candidates;
            settings.library_diagnostics = scan.diagnostics;
        }
        settings.added_game_id = prepared.added_game_id;
        settings.error = None;
        drop(settings);
        drop(prepared.staging);
        Ok(())
    }
    /// Called after manager admission has been released. Active staging leases
    /// protect proposed artwork from collection by a concurrent commit.
    pub(crate) fn finish_metadata(&self) {
        // A later commit cannot replace these references between the snapshot
        // and collection. This runtime-only gate never delays profile Stop;
        // manager admission has already been released.
        let Ok(_admission) = self.admission.try_lock() else {
            return;
        };
        let references = {
            let settings = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            settings
                .saved
                .backgrounds
                .values()
                .chain(settings.saved.default_backgrounds.values())
                .chain(
                    settings
                        .saved
                        .custom_games
                        .values()
                        .filter_map(|game| game.default_background.as_ref()),
                )
                .cloned()
                .collect::<Vec<_>>()
        };
        crate::game_artwork::retain(self.inner.installations.root(), references.iter());
    }
}

#[cfg(test)]
#[path = "game_metadata_api_tests.rs"]
mod tests;

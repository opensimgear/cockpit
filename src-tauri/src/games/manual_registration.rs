//! Atomic registration of one folder explicitly chosen by the user.
use super::*;
use crate::game_installations::{GameRecord, Installation};
use std::collections::BTreeMap;

pub(super) fn register(
    saved: &mut crate::game_installations::Settings,
    catalogue: &[Value],
    request: &Value,
) -> Result<String, String> {
    let name = request["name"].as_str().ok_or("Choose a game name.")?.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("Use a game name between 1 and 80 characters.".into());
    }
    let path = request["installPath"].as_str().filter(|path| crate::game_installations::executable(path))
        .ok_or("Choose an existing absolute game folder.")?;
    let root = fs::canonicalize(path).map_err(|_| "Choose an existing absolute game folder.")?;
    if !root.is_dir() { return Err("Choose an existing absolute game folder.".into()); }
    let executable = crate::game_discovery::validate_executable(
        request["executable"].as_str().ok_or("Choose a game executable.")?,
    )?;
    if !executable.starts_with(&root) { return Err("Choose an executable inside the selected game folder.".into()); }
    let root_text = root.to_str().ok_or("Invalid game folder path.")?;
    if saved.games.values().flat_map(|game| game.installations.values())
        .any(|installation| discovery_review::same_path(&installation.root, root_text)) {
        return Err("This game folder is already in your library. Open its game to edit it.".into());
    }
    let candidate = crate::game_library::manual_candidate(&root, &executable, name)?;
    let id = candidate.game_id.clone().unwrap_or_else(|| discovery_review::key("game", &format!("disk:{}", candidate.app_id)));
    unique_executable(catalogue, &id, candidate.preferred_executable.as_deref())?;
    let installation_id = discovery_review::key("installation", &crate::game_discovery::normalized_path(root_text));
    let source = candidate.source();
    let integration = candidate.game_id.filter(|id| crate::game_discovery::integration_provider(id).is_some());
    let game = saved.games.entry(id.clone()).or_insert_with(|| GameRecord {
        name: name.to_owned(), installations: BTreeMap::new(), selected_installation: None,
        integration, default_background: None,
    });
    game.installations.insert(installation_id.clone(), Installation {
        root: root_text.into(), executable: candidate.preferred_executable,
        executables: candidate.executable_candidates, sources: vec![source],
    });
    if game.selected_installation.is_none() { game.selected_installation = Some(installation_id); }
    Ok(id)
}

//! User-owned game taxonomy and atomic, optimistic tag-set edits.
use super::Settings;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub(crate) mod platform;

pub(crate) fn defaults() -> BTreeMap<String, String> {
    [
        ("action", "Action"), ("adventure", "Adventure"), ("racing", "Racing"),
        ("flight", "Flight"), ("simulation", "Simulation"), ("fps", "FPS"),
        ("third-person-shooter", "Third-person Shooter"), ("shooter", "Shooter"),
        ("rpg", "RPG"), ("action-rpg", "Action RPG"), ("mmorpg", "MMORPG"),
        ("rts", "RTS"), ("turn-based-strategy", "Turn-based Strategy"),
        ("grand-strategy", "Grand Strategy"), ("4x", "4X"), ("strategy", "Strategy"),
        ("tactics", "Tactics"), ("moba", "MOBA"), ("battle-royale", "Battle Royale"),
        ("sports", "Sports"), ("fighting", "Fighting"), ("beat-em-up", "Beat 'em Up"),
        ("platformer", "Platformer"), ("metroidvania", "Metroidvania"),
        ("puzzle", "Puzzle"), ("arcade", "Arcade"), ("rhythm", "Rhythm"),
        ("roguelike", "Roguelike"), ("roguelite", "Roguelite"), ("soulslike", "Soulslike"),
        ("survival", "Survival"), ("sandbox", "Sandbox"), ("open-world", "Open World"),
        ("horror", "Horror"), ("stealth", "Stealth"), ("city-builder", "City Builder"),
        ("management", "Management"), ("tycoon", "Tycoon"), ("tower-defense", "Tower Defense"),
        ("card-game", "Card Game"), ("board-game", "Board Game"), ("visual-novel", "Visual Novel"),
        ("party", "Party"), ("casual", "Casual"), ("educational", "Educational"),
        ("vr", "VR"), ("space", "Space"),
    ].into_iter().map(|(id, name)| (id.into(), name.into())).collect()
}

fn metadata_label(label: &str) -> Option<String> {
    if label.chars().count() > 128 || label.chars().any(char::is_control) { return None; }
    let normalized = label.chars().flat_map(char::to_lowercase)
        .map(|character| if character.is_alphanumeric() { character } else { ' ' })
        .collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

/// Platform labels suggest existing taxonomy entries; they never create tags.
/// Starter identities remain meaningful after users rename their display labels.
pub(crate) fn from_metadata(tags: &BTreeMap<String, String>, labels: &[String]) -> Vec<String> {
    let mut names = BTreeMap::<String, BTreeSet<String>>::new();
    for (id, name) in tags {
        if let Some(name) = metadata_label(name) { names.entry(name).or_default().insert(id.clone()); }
    }
    for (id, name) in defaults() {
        if !tags.contains_key(&id) { continue; }
        for label in [&id, &name] {
            if let Some(label) = metadata_label(label) { names.entry(label).or_default().insert(id.clone()); }
        }
    }
    for (label, id) in [
        ("first person shooter", "fps"), ("third person shooter", "third-person-shooter"),
        ("role playing", "rpg"), ("role playing game", "rpg"), ("role playing games", "rpg"),
        ("action role playing", "action-rpg"), ("action role playing game", "action-rpg"),
        ("massively multiplayer online role playing", "mmorpg"),
        ("massively multiplayer online role playing game", "mmorpg"),
        ("real time strategy", "rts"),
        ("multiplayer online battle arena", "moba"), ("virtual reality", "vr"),
        ("flight simulator", "flight"), ("flight simulation", "flight"),
        ("simulator", "simulation"), ("sports game", "sports"),
    ] {
        if tags.contains_key(id) { names.entry(label.into()).or_default().insert(id.into()); }
    }
    // Normalization may collapse distinct user labels. Ambiguous labels should
    // not assign multiple unrelated tags merely because punctuation differs.
    labels.iter().filter_map(|label| metadata_label(label))
        .filter_map(|label| names.get(&label).filter(|ids| ids.len() == 1).and_then(|ids| ids.first()))
        .cloned().collect::<BTreeSet<_>>().into_iter().take(32).collect()
}

pub(crate) fn validate_definitions(tags: &BTreeMap<String, String>) -> Result<(), String> {
    let mut names = BTreeSet::new();
    if tags.len() > 128 || tags.iter().any(|(id, name)| {
        !super::valid_game_id(id) || name.trim() != name || name.is_empty()
            || name.chars().count() > 48 || name.chars().any(char::is_control)
            || !names.insert(name.to_lowercase())
    }) {
        return Err("Use up to 128 distinct tags with valid identities and names of 1 to 48 characters.".into());
    }
    Ok(())
}

fn validate_set(ids: &[String]) -> Result<BTreeSet<&str>, String> {
    let mut distinct = BTreeSet::new();
    if ids.len() > 32 || ids.iter().any(|id| !super::valid_game_id(id) || !distinct.insert(id.as_str())) {
        return Err("Choose up to 32 distinct valid tag identities.".into());
    }
    Ok(distinct)
}

pub(crate) fn validate_assignment(ids: &[String], tags: &BTreeMap<String, String>) -> Result<(), String> {
    if validate_set(ids)?.iter().any(|id| !tags.contains_key(*id)) {
        return Err("Choose existing tags for this game.".into());
    }
    Ok(())
}

pub(crate) fn save_definitions(settings: &mut Settings, request: &Value) -> Result<(), String> {
    let expected: BTreeMap<String, String> = serde_json::from_value(request["expectedTags"].clone())
        .map_err(|_| "Provide the expected tag dictionary.")?;
    if expected != settings.tags {
        return Err("Tags changed elsewhere. Refresh and try again.".into());
    }
    let tags = serde_json::from_value(request["tags"].clone()).map_err(|_| "Choose a valid tag dictionary.")?;
    validate_definitions(&tags)?;
    for ids in settings.game_tags.values_mut() { ids.retain(|id| tags.contains_key(id)); }
    settings.tags = tags;
    Ok(())
}

pub(crate) fn save_assignment(settings: &mut Settings, request: &Value) -> Result<(), String> {
    let id = request["gameId"].as_str().filter(|id| settings.games.contains_key(*id))
        .ok_or("Choose a discovered game for its tags.")?;
    let ids: Vec<String> = serde_json::from_value(request["tagIds"].clone()).map_err(|_| "Choose a valid tag set.")?;
    let expected: Vec<String> = serde_json::from_value(request["expectedTagIds"].clone()).map_err(|_| "Provide the expected tag set.")?;
    validate_assignment(&ids, &settings.tags)?;
    let expected = validate_set(&expected)?;
    let current = settings.game_tags.get(id).map(Vec::as_slice).unwrap_or_default();
    if expected != validate_set(current)? { return Err("Game tags changed elsewhere. Refresh and try again.".into()); }
    let mut ids = ids;
    ids.sort();
    settings.game_tags.insert(id.into(), ids);
    Ok(())
}

#[cfg(test)]
mod tests;

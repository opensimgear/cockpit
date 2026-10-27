use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(super) struct Target {
    pub app_id: u32,
    pub game: Option<usize>,
    pub entry: Option<usize>,
}

fn app_id(value: &Value) -> Option<u32> {
    value.as_str()?.parse::<u32>().ok().filter(|id| *id > 0)
}
fn path(value: &Value) -> String {
    value
        .as_str()
        .map(crate::game_discovery::normalized_path)
        .unwrap_or_default()
}

/// Only exact installation source IDs may request artwork; title names never identify a store app.
pub(super) fn collect(state: &Value) -> Vec<Target> {
    let mut roots = BTreeMap::new();
    let mut titles = BTreeMap::new();
    let mut existing_art = std::collections::BTreeSet::new();
    let mut targets = Vec::new();
    for observation in state["discovery"]["observations"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if observation["client"] == "steam" {
            if let Some(id) = app_id(&observation["appId"]) {
                roots.insert(path(&observation["installPath"]), id);
            }
        }
    }
    for (index, game) in state["catalogue"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if game["background"]["image"].is_string() || game["defaultBackground"]["image"].is_string()
        {
            if let Some(title) = game["id"].as_str() {
                existing_art.insert(title.to_owned());
            }
        }
        let mut title_app = None;
        for installation in game["installations"].as_array().into_iter().flatten() {
            for source in installation["sources"].as_array().into_iter().flatten() {
                if source["client"] == "steam" {
                    if let Some(id) = app_id(&source["appId"]) {
                        roots.insert(path(&installation["installPath"]), id);
                        title_app.get_or_insert(id);
                    }
                }
            }
        }
        if let Some(id) = title_app {
            if let Some(title) = game["id"].as_str() {
                titles.insert(title.to_owned(), id);
            }
            if !game["background"]["image"].is_string()
                && !game["defaultBackground"]["image"].is_string()
            {
                targets.push(Target {
                    app_id: id,
                    game: Some(index),
                    entry: None,
                });
            }
        }
    }
    for (index, entry) in state["discovery"]["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if entry["image"].is_string()
            || entry["gameId"]
                .as_str()
                .is_some_and(|title| existing_art.contains(title))
        {
            continue;
        }
        let id = roots
            .get(&path(&entry["installPath"]))
            .or_else(|| entry["gameId"].as_str().and_then(|title| titles.get(title)));
        if let Some(id) = id {
            targets.push(Target {
                app_id: *id,
                game: None,
                entry: Some(index),
            });
        }
    }
    targets
}

//! Passive artwork browsing uses only sources already attached to this title.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::OnceLock, time::SystemTime};

type Thumbnails = BTreeMap<PathBuf, (SystemTime, u64, String)>;
fn thumbnail(path: &Path) -> Result<String, String> {
    static CACHE: OnceLock<Mutex<Thumbnails>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    let modified = metadata.modified().map_err(|error| error.to_string())?;
    if let Some((stamp, size, image)) = cache.lock().unwrap_or_else(|p| p.into_inner()).get(path) {
        if *stamp == modified && *size == metadata.len() { return Ok(image.clone()); }
    }
    let image = crate::game_artwork::preview(path.to_str(), None)?;
    let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    if cache.len() >= 256 { cache.clear(); }
    cache.insert(path.to_owned(), (modified, metadata.len(), image.clone()));
    Ok(image)
}

fn identity(path: &Path) -> String {
    format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))
}

impl GamesRuntime {
    pub(crate) fn artwork_options(&self, request: &Value) -> Result<Value, String> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The game runtime has closed.".into());
        }
        let preview = request["command"] == "game_artwork_preview";
        let allowed = if preview { vec!["command", "clientId", "gameId", "artworkId"] }
            else { vec!["command", "clientId", "gameId", "offset", "limit"] };
        if !known(request, &allowed) { return Err("Unknown game artwork request field.".into()); }
        let id = game_id(&request["gameId"])?;
        let sources = {
            let settings = self.inner.settings.lock().unwrap_or_else(|p| p.into_inner());
            let game = settings.saved.games.get(id).ok_or("Choose a discovered game.")?;
            game.installations.values().flat_map(|installation| installation.sources.clone()).collect::<Vec<_>>()
        };
        let choices = crate::game_library::artwork_choices(&sources);
        if preview {
            let artwork_id = request["artworkId"].as_str().filter(|id| id.len() == 64)
                .ok_or("Choose an available library image.")?;
            let choice = choices.iter().find(|choice| identity(&choice.path) == artwork_id)
                .ok_or("This library image is no longer available. Refresh the image choices.")?;
            let image = crate::game_artwork::discovery_preview(&choice.path)?;
            return Ok(json!({"gameId":id,"artworkId":artwork_id,"image":image}));
        }
        let number = |key: &str, default: u64, min: u64, max: u64| -> Result<usize, String> {
            match request.get(key) {
                None => Ok(default as usize),
                Some(value) => value.as_u64().filter(|value| (min..=max).contains(value))
                    .map(|value| value as usize).ok_or_else(|| "Invalid artwork page bounds.".into()),
            }
        };
        let offset = number("offset", 0, 0, 64)?;
        let limit = number("limit", 24, 1, 64)?;
        let items = choices.iter().skip(offset).take(limit).filter_map(|choice| {
            thumbnail(&choice.path).ok()
                .map(|image| json!({"id":identity(&choice.path),"label":choice.label,"image":image}))
        }).collect::<Vec<_>>();
        let next = offset.saturating_add(limit);
        Ok(json!({"gameId":id,"items":items,"total":choices.len(),
            "nextOffset":if next < choices.len() { Some(next) } else { None }}))
    }
}

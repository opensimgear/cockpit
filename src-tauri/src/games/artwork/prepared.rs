//! Local artwork is prepared by its owner; readers only project immutable bytes.
use super::*;
use std::{collections::BTreeSet, sync::Arc};

const MAX_INSTALLATIONS: usize = crate::game_installations::MAX_GAMES;

#[derive(Default)]
struct Images {
    managed: BTreeMap<String, Arc<str>>,
    installations: BTreeMap<String, Arc<str>>,
}

/// A clone shares image bytes and has no filesystem or global-cache dependency.
#[derive(Clone, Default)]
pub(crate) struct PreparedLocalArtwork(Arc<Images>);

struct Scope {
    references: BTreeSet<String>,
    installations: BTreeMap<String, String>,
}

fn installation_identity(path: &str) -> Option<String> {
    if path.trim().is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return None;
    }
    Some(crate::game_discovery::normalized_path(path).trim_end_matches('\\').to_owned())
}

fn scope(catalogue: &[Value]) -> Result<Scope, String> {
    if catalogue.len() > MAX_INSTALLATIONS {
        return Err("Local artwork catalogue exceeds the game library bound.".into());
    }
    let mut references = BTreeSet::new();
    let mut installations = BTreeMap::new();
    for game in catalogue {
        for key in ["background", "defaultBackground"] {
            if let Some(reference) = game[key]["image"].as_str().filter(|value| asset_name(value).is_ok()) {
                references.insert(reference.to_owned());
            }
        }
        if let Some(path) = game["installPath"].as_str() {
            if let Some(identity) = installation_identity(path) {
                installations.entry(identity).or_insert_with(|| path.to_owned());
            }
        }
    }
    // Each of at most MAX_GAMES rows contributes two managed references and
    // one selected installation, so this also bounds both maps without eviction.
    Ok(Scope { references, installations })
}

fn installation_filename(identity: &str) -> String {
    format!("{:x}.jpg", Sha256::digest(identity.as_bytes()))
}

/// Pure construction: the source owner resolves missing directories and watches
/// their nearest existing ancestors. Missing exact files remain in this scope.
pub(crate) fn local_source_paths(root: &Path, catalogue: &[Value]) -> Result<Vec<PathBuf>, String> {
    let scope = scope(catalogue)?;
    let mut paths = BTreeSet::from([root.to_owned()]);
    for reference in scope.references {
        let path = root.join("game-artwork").join(asset_name(&reference)?);
        paths.insert(path.parent().expect("artwork has a parent").to_owned());
        paths.insert(path);
    }
    for identity in scope.installations.keys() {
        let path = root.join("game-platform-artwork").join(installation_filename(identity));
        paths.insert(path.parent().expect("artwork has a parent").to_owned());
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}

fn managed_image(directory: &Path, reference: &str) -> Option<Arc<str>> {
    let name = asset_name(reference).ok()?;
    let path = directory.join(name);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if redirected(&metadata) || !metadata.is_file() || metadata.len() > DISCOVERY_PREVIEW_LIMIT as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(&path).ok()?.take(DISCOVERY_PREVIEW_LIMIT as u64 + 1)
        .read_to_end(&mut bytes).ok()?;
    if bytes.len() > DISCOVERY_PREVIEW_LIMIT || format!("{:x}.jpg", Sha256::digest(&bytes)) != name {
        return None;
    }
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)).into())
}

fn intern(images: &mut BTreeSet<Arc<str>>, image: Arc<str>) -> Arc<str> {
    if let Some(existing) = images.get(image.as_ref()) {
        return existing.clone();
    }
    images.insert(image.clone());
    image
}

/// All storage validation and file reads happen here, outside reader/admission
/// locks. Invalid or missing individual images keep the existing absent-image behavior.
pub(crate) fn prepare_local(root: &Path, catalogue: &[Value]) -> Result<PreparedLocalArtwork, String> {
    let scope = scope(catalogue)?;
    let mut images = Images::default();
    let mut shared = BTreeSet::new();
    if let Ok(directory) = storage(root) {
        for reference in scope.references {
            // Never reuse resolved()'s path cache: source notifications must see
            // deletion and content changes even when the reference is unchanged.
            if let Some(image) = managed_image(&directory, &reference) {
                images.managed.insert(reference, intern(&mut shared, image));
            }
        }
        for (identity, path) in scope.installations {
            if let Some(image) = installation_preview(root, &path) {
                images.installations.insert(identity, intern(&mut shared, image.into()));
            }
        }
    }
    Ok(PreparedLocalArtwork(Arc::new(images)))
}

impl PreparedLocalArtwork {
    pub(crate) fn same_images(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || (self.0.managed == other.0.managed && self.0.installations == other.0.installations)
    }
    /// Memory-only projection of the exact captured reference/installation scope.
    pub(crate) fn decorate(&self, catalogue: &mut [Value]) {
        self.decorate_with(catalogue, &mut |image| Ok(Value::String(image.to_string())))
            .expect("String artwork projection is infallible");
    }
    pub(crate) fn decorate_with(&self, catalogue: &mut [Value], resolve: &mut impl FnMut(&Arc<str>) -> Result<Value, String>) -> Result<(), String> {
        for game in catalogue {
            for key in ["background", "defaultBackground"] {
                let Some(reference) = game[key]["image"].as_str() else { continue; };
                if let Some(image) = self.0.managed.get(reference) {
                    game[key]["image"] = resolve(image)?;
                } else if let Some(object) = game.as_object_mut() {
                    object.remove(key);
                }
            }
            if game["background"]["image"].is_null() {
                let image = game["installPath"].as_str().and_then(installation_identity)
                    .and_then(|identity| self.0.installations.get(&identity));
                if let Some(image) = image {
                    game["defaultBackground"] = serde_json::json!({"image":resolve(image)?,"positionX":50,"positionY":50});
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "prepared_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "prepared_asset_tests.rs"]
mod asset_tests;

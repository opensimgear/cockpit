use crate::game_installations::LibrarySource;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const MAX_CHOICES: usize = 64;
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct ArtworkChoice {
    pub label: String,
    pub path: PathBuf,
}

/// Read-only artwork suggestions for the exact client application identities
/// represented by an imported installation. Never reads account databases.
pub(crate) fn artwork_choices(sources: &[LibrarySource]) -> Vec<ArtworkChoice> {
    let steam_roots = crate::game_discovery::library_steam_roots();
    let battle_roots = super::battlenet::artwork_cache_roots();
    artwork_choices_from_roots(sources, &steam_roots, &battle_roots)
}

fn artwork_choices_from_roots(
    sources: &[LibrarySource],
    steam_roots: &[PathBuf],
    battle_roots: &[PathBuf],
) -> Vec<ArtworkChoice> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for source in sources.iter().take(16) {
        if source.validate().is_err() {
            continue;
        }
        let install = Path::new(&source.install_path);
        let mut candidates = super::artwork::local_choices(install)
            .into_iter()
            .map(|path| (format!("Install: {}", file_label(&path)), path))
            .collect::<Vec<_>>();
        match source.client.as_str() {
            "steam" => {
                let Ok(app_id) = source.app_id.parse::<u32>() else {
                    continue;
                };
                let mut roots = steam_roots.iter().take(16).cloned().collect::<Vec<_>>();
                if let Some(launcher) = source
                    .launcher
                    .as_deref()
                    .map(Path::new)
                    .filter(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.eq_ignore_ascii_case("steam.exe"))
                    })
                    .and_then(Path::parent)
                {
                    roots.insert(0, launcher.to_owned());
                }
                roots.sort();
                roots.dedup();
                for root in roots.into_iter().take(16) {
                    for path in super::steam::steam_art_choices(&root, app_id) {
                        candidates.push((format!("Steam: {}", file_label(&path)), path));
                    }
                }
            }
            "battlenet" => {
                for (label, path) in super::battlenet::artwork_choices(&source.app_id, battle_roots)
                {
                    candidates.push((format!("Battle.net: {label}"), path));
                }
            }
            _ => {}
        }
        for (label, path) in candidates {
            if !valid_raster(&path) {
                continue;
            }
            let identity = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if seen.insert(identity) {
                output.push(ArtworkChoice { label, path });
                if output.len() >= MAX_CHOICES {
                    return output;
                }
            }
        }
    }
    output
}

pub(super) fn valid_raster(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES
    {
        return false;
    }
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        if !["jpg", "jpeg", "png", "webp"]
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            return false;
        }
    }
    image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.into_dimensions().ok())
        .is_some_and(|(width, height)| width > 0 && height > 0 && width <= 8192 && height <= 8192)
}

fn file_label(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Artwork");
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .replace(['_', '-'], " ")
}

#[cfg(test)]
#[path = "artwork_choices_tests.rs"]
mod tests;

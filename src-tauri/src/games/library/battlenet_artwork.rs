//! Battle.net's cached catalogue maps product/type IDs to hashed local images.
//! Read only public catalogue fragments and raster assets, never account stores.
use super::super::{Candidate, regular, sorted_children};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 8192;
const MAX_FRAGMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CATALOGUE_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Battle.net/Cache"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let root = PathBuf::from(profile).join("AppData/Local/Battle.net/Cache");
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

pub(super) fn assign(candidates: &mut [Candidate], roots: &[PathBuf]) {
    let codes = candidates
        .iter()
        .map(|game| game.app_id.to_ascii_lowercase())
        .collect();
    let images = lookup(roots, &codes);
    for candidate in candidates {
        if candidate.artwork.is_none() {
            candidate.artwork = images.get(&candidate.app_id.to_ascii_lowercase()).cloned();
        }
    }
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn fragment(path: &Path, remaining: &mut usize) -> Option<Value> {
    let mut file = fs::File::open(path).ok()?;
    let mut first = [0];
    file.read_exact(&mut first).ok()?;
    if first[0] != b'{' {
        return None;
    }
    let limit = (*remaining).min(MAX_FRAGMENT_BYTES);
    let mut bytes = vec![first[0]];
    file.take(limit as u64).read_to_end(&mut bytes).ok()?;
    *remaining = remaining.saturating_sub(bytes.len());
    if bytes.len() > limit {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn raster(root: &Path, hash: &str) -> Option<PathBuf> {
    if !hex(hash, 32) && !hex(hash, 64) {
        return None;
    }
    let first = root.join(&hash[..2]);
    let second = first.join(&hash[2..4]);
    let image = second.join(hash);
    if !regular(&first, true) || !regular(&second, true) || !regular(&image, false) {
        return None;
    }
    let file = fs::File::open(&image).ok()?;
    if file.metadata().ok()?.len() > 16 * 1024 * 1024 {
        return None;
    }
    let mut header = Vec::new();
    file.take(12).read_to_end(&mut header).ok()?;
    (header.starts_with(b"\x89PNG\r\n\x1a\n")
        || header.starts_with(b"\xff\xd8\xff")
        || (header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP")))
    .then_some(image)
}

fn product_matches(product: &Value, code: &str) -> bool {
    product["id"]
        .as_str()
        .is_some_and(|id| id.eq_ignore_ascii_case(code))
}

// A `default` asset can still be Chinese: the cache contains an OSI China
// override with zhCN filenames. Check the actual resource as well as its bucket.
fn suitable_resource(reference: &str, asset: &Value) -> bool {
    let reference = reference.to_ascii_lowercase();
    let name = asset["name"].as_str().unwrap_or("").to_ascii_lowercase();
    let tokens = reference
        .split(|c: char| !c.is_ascii_alphanumeric())
        .chain(name.split(|c: char| !c.is_ascii_alphanumeric()));
    !tokens.into_iter().any(|token| {
        matches!(
            token,
            "cn" | "zhcn"
                | "zhtw"
                | "jajp"
                | "kokr"
                | "dede"
                | "eses"
                | "esmx"
                | "frfr"
                | "itit"
                | "plpl"
                | "ptbr"
                | "ruru"
                | "thth"
                | "x2"
                | "expansion"
        )
    })
}

fn usable_dimensions(path: &Path) -> bool {
    image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.into_dimensions().ok())
        .is_some_and(|(width, height)| {
            width >= 432 && height >= 240 && width <= 8192 && height <= 8192
        })
}

type Rank = (usize, usize, u64);
fn collect(
    root: &Path,
    data: &Value,
    codes: &BTreeSet<String>,
    found: &mut BTreeMap<String, (Rank, PathBuf)>,
) {
    let Some(products) = data["products"].as_array() else {
        return;
    };
    let Some(files) = data["files"].as_object() else {
        return;
    };
    let version = data["version"].as_u64().unwrap_or(0);
    for product in products.iter().take(512) {
        for code in codes.iter().filter(|code| product_matches(product, code)) {
            // Full install art is identifiable and has enough pixels for a card.
            // Logos/icons and decorative launcher gradients are not game pictures.
            for (priority, field) in ["install_background", "key_art"].iter().enumerate() {
                let Some(reference) = product["base"][*field].as_str() else {
                    continue;
                };
                for (locale_priority, locale) in ["enUS", "enGB", "default"].iter().enumerate() {
                    let Some(assets) = files.get(*locale) else {
                        continue;
                    };
                    let asset = &assets[reference];
                    if !suitable_resource(reference, asset) {
                        continue;
                    }
                    let Some(hash) = asset["hash"].as_str() else {
                        continue;
                    };
                    let Some(image) = raster(root, hash) else {
                        continue;
                    };
                    if !usable_dimensions(&image) {
                        continue;
                    }
                    let rank = (usize::MAX - locale_priority, usize::MAX - priority, version);
                    if found.get(code).is_none_or(|(previous, _)| rank > *previous) {
                        found.insert(code.clone(), (rank, image));
                    }
                }
            }
        }
    }
}

fn lookup(roots: &[PathBuf], codes: &BTreeSet<String>) -> BTreeMap<String, PathBuf> {
    let mut found = BTreeMap::new();
    if codes.is_empty() {
        return BTreeMap::new();
    }
    let mut inspected = 0;
    let mut remaining = MAX_CATALOGUE_BYTES;
    'roots: for root in roots.iter().take(4).filter(|root| regular(root, true)) {
        for first in sorted_children(root).into_iter().filter(|path| {
            regular(path, true)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| hex(name, 2))
        }) {
            for second in sorted_children(&first).into_iter().filter(|path| {
                regular(path, true)
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| hex(name, 2))
            }) {
                for path in sorted_children(&second) {
                    inspected += 1;
                    if inspected > MAX_FILES || remaining == 0 {
                        break 'roots;
                    }
                    if regular(&path, false)
                        && path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| hex(name, 32) || hex(name, 64))
                    {
                        if let Some(data) = fragment(&path, &mut remaining) {
                            collect(root, &data, codes, &mut found);
                        }
                    }
                }
            }
        }
    }
    found
        .into_iter()
        .map(|(code, (_, path))| (code, path))
        .collect()
}

/// Enumerate cached image assets named by the exact Battle.net product id.
/// Unlike automatic artwork ranking, this includes every catalogue field and
/// locale so a user can choose logos, portraits, and alternate key art too.
pub(super) fn choices(roots: &[PathBuf], code: &str) -> Vec<(String, PathBuf)> {
    if code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Vec::new();
    }
    let mut found = BTreeMap::<PathBuf, String>::new();
    let mut inspected = 0;
    let mut remaining = MAX_CATALOGUE_BYTES;
    'roots: for root in roots.iter().take(4).filter(|root| regular(root, true)) {
        for first in sorted_children(root).into_iter().filter(|path| {
            regular(path, true)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| hex(name, 2))
        }) {
            for second in sorted_children(&first).into_iter().filter(|path| {
                regular(path, true)
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| hex(name, 2))
            }) {
                for path in sorted_children(&second) {
                    inspected += 1;
                    if inspected > MAX_FILES || remaining == 0 || found.len() >= 64 {
                        break 'roots;
                    }
                    if !regular(&path, false)
                        || !path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| hex(name, 32) || hex(name, 64))
                    {
                        continue;
                    }
                    let Some(data) = fragment(&path, &mut remaining) else {
                        continue;
                    };
                    let Some(products) = data["products"].as_array() else {
                        continue;
                    };
                    let Some(files) = data["files"].as_object() else {
                        continue;
                    };
                    for product in products
                        .iter()
                        .take(512)
                        .filter(|product| product_matches(product, code))
                    {
                        let Some(fields) = product["base"].as_object() else {
                            continue;
                        };
                        for (field, reference) in fields {
                            if field.len() > 64 || field.chars().any(char::is_control) {
                                continue;
                            }
                            let Some(reference) = reference.as_str() else {
                                continue;
                            };
                            for (locale, assets) in files {
                                if locale.len() > 16 || !locale.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                                    continue;
                                }
                                let asset = &assets[reference];
                                let Some(hash) = asset["hash"].as_str() else {
                                    continue;
                                };
                                let Some(image) = raster(root, hash) else {
                                    continue;
                                };
                                if !usable_choice_dimensions(&image) {
                                    continue;
                                }
                                let locale_name = locale_label(locale);
                                let label = if locale_name.is_empty() {
                                    pretty_field(field)
                                } else {
                                    format!("{} ({locale_name})", pretty_field(field))
                                };
                                found.entry(image).or_insert(label);
                                if found.len() >= 64 {
                                    break 'roots;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    found
        .into_iter()
        .map(|(path, label)| (label, path))
        .collect()
}

fn usable_choice_dimensions(path: &Path) -> bool {
    image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.into_dimensions().ok())
        .is_some_and(|(width, height)| {
            width >= 32 && height >= 32 && width <= 8192 && height <= 8192
        })
}

fn pretty_field(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn locale_label(value: &str) -> &str {
    match value {
        "enUS" => "English (US)",
        "enGB" => "English (UK)",
        "default" => "Default",
        _ => value,
    }
}

#[cfg(test)]
#[path = "battlenet_artwork_tests.rs"]
mod tests;

//! Bounded, content-addressed game backgrounds. Only generated JPEG thumbnails
//! are exposed to the UI; persisted settings never contain arbitrary file URLs.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const SOURCE_LIMIT: usize = 16 * 1024 * 1024;
const THUMBNAIL_LIMIT: usize = 12 * 1024;
const MAX_ASSETS: usize = 1024;

mod model;
mod installation_cache;
pub(crate) use installation_cache::{cache_installation_preview, installation_preview};
use model::*;
pub(crate) use model::{Background,Crop};
fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
fn storage(root: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("Managed game artwork needs an absolute application directory.".into());
    }
    let directory = root.join("game-artwork");
    let mut current = PathBuf::new();
    for part in directory.components() {
        current.push(part.as_os_str());
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if redirected(&metadata) || !metadata.is_dir() => {
                return Err(
                    "Managed game artwork cannot use redirected or non-directory storage.".into(),
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Cannot inspect game artwork storage: {error}")),
        }
    }
    Ok(directory)
}
fn decoded(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("Cannot read background image: {error}"))?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
    ) {
        return Err("Choose a PNG, JPEG or WebP image.".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("Cannot decode background image: {error}"))?;
    Ok(image)
}
fn thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image=decoded(bytes)?;
    let mut size = 640;
    loop {
        let scaled = image.thumbnail(size, size).to_rgb8();
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 68)
            .encode_image(&image::DynamicImage::ImageRgb8(scaled))
            .map_err(|error| format!("Cannot prepare game background: {error}"))?;
        if bytes.len() <= THUMBNAIL_LIMIT {
            return Ok(bytes);
        }
        if size <= 64 {
            return Err("The background image cannot fit its storage budget.".into());
        }
        size = (size * 3 / 4).max(64);
    }
}
type Cache = BTreeMap<PathBuf, String>;
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
fn stages() -> &'static Mutex<BTreeMap<PathBuf, usize>> {
    static STAGES: OnceLock<Mutex<BTreeMap<PathBuf, usize>>> = OnceLock::new();
    STAGES.get_or_init(Default::default)
}
/// A proposed registry may reference an imported image before its commit.
/// Collection waits until no proposal for this directory is outstanding.
pub(crate) struct Staging(PathBuf, fs::File);
fn asset_lock(root: &Path) -> Result<fs::File, String> {
    storage(root)?;
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let path = root.join("game-artwork.lock");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if redirected(&metadata) || !metadata.is_file() {
            return Err("Artwork lock must be a regular local file.".into());
        }
    }
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())
}
pub(crate) fn staging(root: &Path) -> Result<Staging, String> {
    let directory = storage(root)?;
    let lease = asset_lock(root)?;
    lease
        .try_lock_shared()
        .map_err(|_| "Artwork collection is busy; retry the edit.")?;
    *stages()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .entry(directory.clone())
        .or_default() += 1;
    Ok(Staging(directory, lease))
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = self.1.unlock();
        let mut stages = stages().lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(count) = stages.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                stages.remove(&self.0);
            }
        }
    }
}
mod sources;
pub(crate) use sources::{cached_discovery_preview, discovery_preview, preview};
use sources::*;

pub(crate) fn import_preview(root: &Path, data: &str, crop: Crop) -> Result<Background, String> {
    crop.validate()?;
    let bytes = discovery_preview_bytes(data)?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
        .map_err(|error| error.to_string())?;
    let ratio = crop.width * image.width() as f64 / (crop.height * image.height() as f64);
    if (ratio - 16.0 / 9.0).abs() > 0.001 {
        return Err("The crop must preserve the tile's 16:9 aspect ratio.".into());
    }
    store_thumbnail(root, bytes, 50.0, 50.0, Some(crop))
}

pub(crate) fn import(
    root: &Path,
    path: &str,
    position_x: f64,
    position_y: f64,
) -> Result<Background, String> {
    store_thumbnail(
        root,
        thumbnail(&local_bytes(path)?)?,
        position_x,
        position_y,
        None,
    )
}
fn store_thumbnail(
    root: &Path,
    bytes: Vec<u8>,
    position_x: f64,
    position_y: f64,
    crop: Option<Crop>,
) -> Result<Background, String> {
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let background = Background {
        image: format!("artwork:{hash}.jpg"),
        position_x,
        position_y,
        crop,
    };
    background.validate()?;
    let directory = storage(root)?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let destination = directory.join(format!("{hash}.jpg"));
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if redirected(&metadata) || !metadata.is_file() {
            return Err("Managed artwork asset is redirected or invalid.".into());
        }
        if fs::read(&destination).map_err(|error| error.to_string())? != bytes {
            return Err("Managed artwork asset has changed.".into());
        }
    } else {
        if fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .take(MAX_ASSETS + 1)
            .count()
            >= MAX_ASSETS
        {
            return Err("Managed artwork storage is full. Remove unused artwork before adding another image.".into());
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        temporary
            .persist_noclobber(&destination)
            .map_err(|error| error.to_string())?;
    }
    let mut cached = cache().lock().unwrap_or_else(|poison| poison.into_inner());
    if cached.len() >= MAX_ASSETS {
        cached.clear();
    }
    cached.insert(
        destination,
        format!("data:image/jpeg;base64,{}", STANDARD.encode(&bytes)),
    );
    Ok(background)
}
fn resolved(root: &Path, reference: &str) -> Result<String, String> {
    let path = storage(root)?.join(asset_name(reference)?);
    let mut cached = cache().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(value) = cached.get(&path) {
        return Ok(value.clone());
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if redirected(&metadata) || !metadata.is_file() || metadata.len() > DISCOVERY_PREVIEW_LIMIT as u64 {
        return Err("Managed artwork asset is unavailable.".into());
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    if format!("{:x}.jpg", Sha256::digest(&bytes)) != asset_name(reference)? {
        return Err("Managed artwork asset has changed.".into());
    }
    let result = format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes));
    if cached.len() >= MAX_ASSETS {
        cached.clear();
    }
    cached.insert(path, result.clone());
    Ok(result)
}
pub(crate) fn decorate(root: &Path, catalogue: &mut [Value]) {
    for game in catalogue {
        for key in ["background", "defaultBackground"] {
            let Some(reference) = game[key]["image"].as_str() else {
                continue;
            };
            match resolved(root, reference) {
                Ok(image) => game[key]["image"] = Value::String(image),
                Err(_) => {
                    game.as_object_mut().map(|object| object.remove(key));
                }
            }
        }
        if game["background"]["image"].as_str().is_none() {
            if let Some(image) = game["installPath"].as_str()
                .and_then(|installation| installation_preview(root, installation)) {
                game["defaultBackground"] = serde_json::json!({"image":image,"positionX":50,"positionY":50});
            }
        }
    }
}

/// Collect generated assets only after the owning registry has been committed.
pub(crate) fn retain<'a>(root: &Path, references: impl Iterator<Item = &'a Background>) {
    let Ok(directory) = storage(root) else {
        return;
    };
    let active_stages = stages().lock().unwrap_or_else(|poison| poison.into_inner());
    if active_stages.contains_key(&directory) {
        return;
    }
    let Ok(lease) = asset_lock(root) else {
        return;
    };
    if lease.try_lock().is_err() {
        return;
    }
    // The caller's cache can be older than another process's commit. Read the
    // authority under the cross-process asset lease before deleting any bytes.
    let Ok(database) =
        osg_persistence::Database::for_namespace(&root.join("game-library.json"))
    else {
        return;
    };
    let Ok(record) = database.read("games", "library") else {
        return;
    };
    let retained = if let Some(record) = record {
        let Ok(settings) =
            serde_json::from_slice::<crate::game_installations::Settings>(&record.payload)
        else {
            return;
        };
        if settings.validate().is_err() {
            return;
        }
        settings
            .backgrounds
            .values()
            .chain(settings.default_backgrounds.values())
            .chain(
                settings
                    .games
                    .values()
                    .filter_map(|game| game.default_background.as_ref()),
            )
            .filter_map(|background| asset_name(&background.image).ok().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>()
    } else {
        references
            .filter_map(|background| asset_name(&background.image).ok().map(str::to_owned))
            .collect()
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.take(MAX_ASSETS + 1).filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if retained.contains(&name) || asset_name(&format!("artwork:{name}")).is_err() {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !redirected(&metadata) && metadata.is_file() && fs::remove_file(entry.path()).is_ok() {
            cache()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remove(&entry.path());
        }
    }
}

#[cfg(test)]
mod tests;

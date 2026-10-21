//! Bounded, content-addressed game backgrounds. Only generated JPEG thumbnails
//! are exposed to the UI; persisted settings never contain arbitrary file URLs.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Background {
    pub image: String,
    pub position_x: f64,
    pub position_y: f64,
}
impl Background {
    pub(crate) fn validate(&self) -> Result<(), String> {
        asset_name(&self.image)?;
        if !self.position_x.is_finite()
            || !self.position_y.is_finite()
            || !(0.0..=100.0).contains(&self.position_x)
            || !(0.0..=100.0).contains(&self.position_y)
        {
            return Err("Background position must be between 0 and 100 percent.".into());
        }
        Ok(())
    }
}
fn asset_name(reference: &str) -> Result<&str, String> {
    let name = reference
        .strip_prefix("artwork:")
        .ok_or("Unknown managed game background.")?;
    let hash = name
        .strip_suffix(".jpg")
        .ok_or("Unknown managed game background format.")?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Invalid managed game background identity.".into());
    }
    Ok(name)
}
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
fn thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
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
pub(crate) struct Staging(PathBuf);
pub(crate) fn staging(root: &Path) -> Result<Staging, String> {
    let directory = storage(root)?;
    *stages()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .entry(directory.clone())
        .or_default() += 1;
    Ok(Staging(directory))
}
impl Drop for Staging {
    fn drop(&mut self) {
        let mut stages = stages().lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(count) = stages.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                stages.remove(&self.0);
            }
        }
    }
}
pub(crate) fn import(
    root: &Path,
    path: &str,
    position_x: f64,
    position_y: f64,
) -> Result<Background, String> {
    let source = Path::new(path);
    if !source.is_absolute() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err("Choose an absolute image file path.".into());
    }
    let file =
        fs::File::open(source).map_err(|error| format!("Cannot open background image: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Choose a regular image file.".into());
    }
    let mut original = Vec::new();
    file.take(SOURCE_LIMIT as u64 + 1)
        .read_to_end(&mut original)
        .map_err(|error| error.to_string())?;
    if original.len() > SOURCE_LIMIT {
        return Err("Background images must be no larger than 16 MiB.".into());
    }
    let bytes = thumbnail(&original)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let background = Background {
        image: format!("artwork:{hash}.jpg"),
        position_x,
        position_y,
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
    if redirected(&metadata) || !metadata.is_file() || metadata.len() > THUMBNAIL_LIMIT as u64 {
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
    let retained = references
        .filter_map(|background| asset_name(&background.image).ok().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
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
mod tests {
    use super::*;
    fn sample(root: &Path) -> PathBuf {
        let source = root.join("source.png");
        image::RgbImage::from_pixel(110, 80, image::Rgb([35, 70, 120]))
            .save(&source)
            .unwrap();
        source
    }
    #[test]
    fn imported_image_is_managed_deduplicated_and_survives_source_removal() {
        let temporary = crate::test_directory();
        let root = temporary.path();
        let source = sample(root);
        let background = import(root, source.to_str().unwrap(), 20.0, 80.0).unwrap();
        assert_eq!(
            background,
            import(root, source.to_str().unwrap(), 20.0, 80.0).unwrap()
        );
        fs::remove_file(&source).unwrap();
        let mut catalogue = vec![serde_json::json!({"background":background})];
        decorate(root, &mut catalogue);
        assert!(catalogue[0]["background"]["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/jpeg;base64,"));
        assert_eq!(catalogue[0]["background"]["positionX"], 20.0);
        assert_eq!(fs::read_dir(root.join("game-artwork")).unwrap().count(), 1);
    }
    #[test]
    fn invalid_images_positions_and_managed_paths_are_rejected() {
        let temporary = crate::test_directory();
        let root = temporary.path();
        let source = sample(root);
        assert!(import(root, source.to_str().unwrap(), f64::NAN, 50.0).is_err());
        assert!(import(root, source.to_str().unwrap(), 50.0, 101.0).is_err());
        fs::write(&source, b"not an image").unwrap();
        assert!(import(root, source.to_str().unwrap(), 50.0, 50.0).is_err());
        assert!(asset_name("artwork:../../file.jpg").is_err());
        let mut catalogue =
            vec![serde_json::json!({"background":{"image":"artwork:../../file.jpg"}})];
        decorate(root, &mut catalogue);
        assert!(catalogue[0].get("background").is_none());
    }
    #[test]
    fn dimensions_and_encoded_thumbnail_are_bounded() {
        let mut source = Vec::new();
        image::RgbImage::from_fn(1200, 900, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 251) as u8, ((x * y) % 251) as u8])
        })
        .write_to(&mut Cursor::new(&mut source), image::ImageFormat::Png)
        .unwrap();
        let encoded = thumbnail(&source).unwrap();
        assert!(encoded.len() <= THUMBNAIL_LIMIT);
        assert!(image::load_from_memory(&encoded).unwrap().width() <= 640);
    }
    #[test]
    fn collection_preserves_shared_assets_and_unrelated_files() {
        let temporary = crate::test_directory();
        let root = temporary.path();
        let source = sample(root);
        let background = import(root, source.to_str().unwrap(), 50.0, 50.0).unwrap();
        fs::write(root.join("game-artwork/notes.txt"), b"unrelated").unwrap();
        retain(root, [&background, &background].into_iter());
        assert!(resolved(root, &background.image).is_ok());
        retain(root, std::iter::empty());
        assert!(!root
            .join("game-artwork")
            .join(asset_name(&background.image).unwrap())
            .exists());
        assert!(root.join("game-artwork/notes.txt").exists());
    }
    #[test]
    fn collection_waits_for_uncommitted_image_proposals() {
        let temporary = crate::test_directory();
        let root = temporary.path();
        let first = staging(root).unwrap();
        let second = staging(root).unwrap();
        let source = sample(root);
        let background = import(root, source.to_str().unwrap(), 50.0, 50.0).unwrap();
        retain(root, std::iter::empty());
        assert!(resolved(root, &background.image).is_ok());
        drop(first);
        retain(root, std::iter::empty());
        assert!(resolved(root, &background.image).is_ok());
        drop(second);
        retain(root, std::iter::empty());
        assert!(resolved(root, &background.image).is_err());
    }
}

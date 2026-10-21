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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Crop {
    fn validate(&self) -> Result<(), String> {
        if ![self.x, self.y, self.width, self.height].iter().all(|value| value.is_finite())
            || self.x < 0.0 || self.y < 0.0 || self.width <= 0.0 || self.height <= 0.0
            || self.x + self.width > 1.0 + 1e-9 || self.y + self.height > 1.0 + 1e-9
        {
            return Err("Keep the entire tile crop inside the image bounds.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Background {
    pub image: String,
    pub position_x: f64,
    pub position_y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
}
impl Background {
    pub(crate) fn validate(&self) -> Result<(), String> {
        asset_name(&self.image)?;
        if let Some(crop) = self.crop { crop.validate()?; }
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
fn local_bytes(path: &str) -> Result<Vec<u8>, String> {
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
    Ok(original)
}

fn remote_bytes(url: &str) -> Result<Vec<u8>, String> {
    if url.len() > 4096 || url.chars().any(char::is_control) { return Err("Enter a bounded image URL without control characters.".into()); }
    let mut url = tauri::Url::parse(url).map_err(|_| "Enter a complete HTTP or HTTPS image URL.")?;
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15))
        .timeout_connect(std::time::Duration::from_secs(4)).redirects(0).build();
    for redirects in 0..=3 {
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
            || !url.username().is_empty() || url.password().is_some() || url.as_str().len() > 4096 {
            return Err("Use an HTTP or HTTPS image URL without embedded credentials.".into());
        }
        let response = match agent.get(url.as_str()).set("Accept", "image/png,image/jpeg,image/webp").call() {
            Ok(response) | Err(ureq::Error::Status(300..=399, response)) => response,
            Err(error) => return Err(format!("Cannot download background image: {error}")),
        };
        if (300..400).contains(&response.status()) {
            if redirects == 3 { return Err("The image URL redirects too many times.".into()); }
            url = url.join(response.header("Location").ok_or("The image URL returned an invalid redirect.")?)
                .map_err(|_| "The image URL returned an invalid redirect.")?;
            continue;
        }
        if response.header("Content-Length").and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|size| size > SOURCE_LIMIT as u64) {
            return Err("Background images must be no larger than 16 MiB.".into());
        }
        let mut bytes = Vec::new();
        response.into_reader().take(SOURCE_LIMIT as u64 + 1).read_to_end(&mut bytes)
            .map_err(|error| format!("Cannot download background image: {error}"))?;
        if bytes.len() > SOURCE_LIMIT { return Err("Background images must be no larger than 16 MiB.".into()); }
        return Ok(bytes);
    }
    unreachable!()
}

/// Preview is transient: cancelling the editor never publishes artwork.
pub(crate) fn preview(path: Option<&str>, url: Option<&str>) -> Result<String, String> {
    let source = match (path, url) {
        (Some(path), None) => local_bytes(path)?,
        (None, Some(url)) => remote_bytes(url)?,
        _ => return Err("Choose one image source: a disk path or a URL.".into()),
    };
    Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(thumbnail(&source)?)))
}

fn preview_bytes(data: &str) -> Result<Vec<u8>, String> {
    let encoded = data.strip_prefix("data:image/jpeg;base64,").ok_or("The background preview is invalid. Load the image again.")?;
    if encoded.len() > THUMBNAIL_LIMIT * 4 / 3 + 4 { return Err("The background preview exceeds its size bound.".into()); }
    let bytes = STANDARD.decode(encoded).map_err(|_| "The background preview is invalid.")?;
    if bytes.len() > THUMBNAIL_LIMIT { return Err("The background preview exceeds its size bound.".into()); }
    let (width,height) = image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Jpeg)
        .into_dimensions().map_err(|_| "The background preview is not a JPEG image.")?;
    if width == 0 || height == 0 || width > 640 || height > 640 { return Err("The background preview exceeds its dimension bound.".into()); }
    Ok(bytes)
}

pub(crate) fn import_preview(root: &Path, data: &str, crop: Crop) -> Result<Background, String> {
    crop.validate()?;
    let bytes = preview_bytes(data)?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).map_err(|error| error.to_string())?;
    let ratio = crop.width * image.width() as f64 / (crop.height * image.height() as f64);
    if (ratio - 16.0 / 9.0).abs() > 0.001 { return Err("The crop must preserve the tile's 16:9 aspect ratio.".into()); }
    store_thumbnail(root, bytes, 50.0, 50.0, Some(crop))
}

pub(crate) fn import(root: &Path, path: &str, position_x: f64, position_y: f64) -> Result<Background, String> {
    store_thumbnail(root, thumbnail(&local_bytes(path)?)?, position_x, position_y, None)
}
fn store_thumbnail(root: &Path, bytes: Vec<u8>, position_x: f64, position_y: f64, crop: Option<Crop>) -> Result<Background, String> {
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
    #[test]
    fn transient_previews_and_crops_preserve_bounds_shape_and_exact_managed_bytes() {
        let directory = crate::test_directory();
        let source = sample(directory.path());
        let data = preview(Some(source.to_str().unwrap()),None).unwrap();
        assert!(!directory.path().join("game-artwork").exists());
        let decoded = image::load_from_memory(&preview_bytes(&data).unwrap()).unwrap();
        let crop = Crop{x:0.0,y:0.1,width:1.0,height:decoded.width() as f64/decoded.height() as f64/(16.0/9.0)};
        let background = import_preview(directory.path(),&data,crop).unwrap();
        assert_eq!(background.crop,Some(crop));
        assert_eq!(resolved(directory.path(),&background.image).unwrap(),data);
        for invalid in [Crop{x:0.1,..crop},Crop{height:1.0,..crop},Crop{x:f64::NAN,..crop},Crop{width:0.0,..crop}] {
            assert!(import_preview(directory.path(),&data,invalid).is_err());
        }
        assert!(import_preview(directory.path(),"data:image/jpeg;base64,AAAA",crop).is_err());
        assert!(preview(Some(source.to_str().unwrap()),Some("https://example.com/image.jpg")).is_err());
        assert!(preview(None,Some("file:///C:/secret.jpg")).is_err());
        assert!(preview(None,Some("https://user:secret@example.com/image.jpg")).is_err());
    }
    #[test]
    fn image_urls_follow_bounded_http_redirects_and_reject_large_or_non_image_responses() {
        use std::net::TcpListener;
        let directory = crate::test_directory();
        let source = sample(directory.path());
        let bytes = fs::read(source).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut image_response = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).into_bytes();
            image_response.extend(bytes);
            let responses = [b"HTTP/1.1 302 Found\r\nLocation: /image\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),image_response,
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",SOURCE_LIMIT+1).into_bytes(),
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntext".to_vec(),
                b"HTTP/1.1 302 Found\r\nLocation: file:///C:/secret.jpg\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()];
            for response in responses { let (mut stream,_) = listener.accept().unwrap(); stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap(); let mut request=[0;4096];let _=stream.read(&mut request);stream.write_all(&response).unwrap(); }
        });
        assert!(preview(None,Some(&format!("http://{address}/redirect"))).unwrap().starts_with("data:image/jpeg;base64,"));
        assert!(preview(None,Some(&format!("http://{address}/large"))).unwrap_err().contains("16 MiB"));
        assert!(preview(None,Some(&format!("http://{address}/text"))).is_err());
        assert!(preview(None,Some(&format!("http://{address}/file-redirect"))).is_err());
        server.join().unwrap();
    }
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

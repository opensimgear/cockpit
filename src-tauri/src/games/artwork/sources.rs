use super::*;
#[path = "discovery_cache.rs"]
pub(super) mod discovery_cache;

pub(super) const DISCOVERY_PREVIEW_LIMIT: usize = 128 * 1024;
pub(super) const DISCOVERY_PREVIEW_EDGE: u32 = 960;

pub(super) fn local_bytes(path: &str) -> Result<Vec<u8>, String> {
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

pub(super) fn remote_bytes(url: &str) -> Result<Vec<u8>, String> {
    if url.len() > 4096 || url.chars().any(char::is_control) {
        return Err("Enter a bounded image URL without control characters.".into());
    }
    let mut url =
        tauri::Url::parse(url).map_err(|_| "Enter a complete HTTP or HTTPS image URL.")?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .timeout_connect(std::time::Duration::from_secs(4))
        .redirects(0)
        .build();
    for redirects in 0..=3 {
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.as_str().len() > 4096
        {
            return Err("Use an HTTP or HTTPS image URL without embedded credentials.".into());
        }
        let response = match agent
            .get(url.as_str())
            .set("Accept", "image/png,image/jpeg,image/webp")
            .call()
        {
            Ok(response) | Err(ureq::Error::Status(300..=399, response)) => response,
            Err(error) => return Err(format!("Cannot download background image: {error}")),
        };
        if (300..400).contains(&response.status()) {
            if redirects == 3 {
                return Err("The image URL redirects too many times.".into());
            }
            url = url
                .join(
                    response
                        .header("Location")
                        .ok_or("The image URL returned an invalid redirect.")?,
                )
                .map_err(|_| "The image URL returned an invalid redirect.")?;
            continue;
        }
        if response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|size| size > SOURCE_LIMIT as u64)
        {
            return Err("Background images must be no larger than 16 MiB.".into());
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(SOURCE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Cannot download background image: {error}"))?;
        if bytes.len() > SOURCE_LIMIT {
            return Err("Background images must be no larger than 16 MiB.".into());
        }
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
    Ok(format!(
        "data:image/jpeg;base64,{}",
        STANDARD.encode(thumbnail(&source)?)
    ))
}

/// Returns only an already-prepared image for the source's current file stamp.
pub(crate) fn cached_discovery_preview(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    discovery_cache::get(path, metadata.modified().ok()?, metadata.len())
}

/// Bounded transient previews retain enough pixels for cards on high DPI displays.
pub(crate) fn discovery_preview(path: &Path) -> Result<String, String> {
    if let Ok(metadata) = fs::metadata(path) {
        if let Ok(modified) = metadata.modified() {
            return discovery_cache::get_or_prepare(path, modified, metadata.len(), || {
                prepare_discovery_preview(path)
            });
        }
    }
    prepare_discovery_preview(path)
}

fn prepare_discovery_preview(path: &Path) -> Result<String, String> {
    discovery_bytes(&local_bytes(path.to_str().ok_or("Invalid artwork path.")?)?)
}

pub(super) fn discovery_bytes(source: &[u8]) -> Result<String, String> {
    let image = decoded(source)?;
    for size in [DISCOVERY_PREVIEW_EDGE, 800, 640, 480, 320] {
        let scaled = if image.width() > size || image.height() > size {
            image.thumbnail(size, size).to_rgb8()
        } else {
            image.to_rgb8()
        };
        // Preserve display pixels before reducing dimensions for detailed artwork.
        for quality in [90, 82] {
            let mut bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
                .encode_image(&scaled)
                .map_err(|error| error.to_string())?;
            if bytes.len() <= DISCOVERY_PREVIEW_LIMIT {
                return Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)));
            }
        }
    }
    Err("Discovery artwork exceeds the thumbnail budget.".into())
}

pub(super) fn preview_bytes(data: &str) -> Result<Vec<u8>, String> {
    bounded_preview_bytes(data, THUMBNAIL_LIMIT, 640)
}

pub(super) fn discovery_preview_bytes(data: &str) -> Result<Vec<u8>, String> {
    bounded_preview_bytes(data, DISCOVERY_PREVIEW_LIMIT, DISCOVERY_PREVIEW_EDGE)
}

fn bounded_preview_bytes(data: &str, byte_limit: usize, edge: u32) -> Result<Vec<u8>, String> {
    let encoded = data
        .strip_prefix("data:image/jpeg;base64,")
        .ok_or("The background preview is invalid. Load the image again.")?;
    if encoded.len() > byte_limit * 4 / 3 + 4 {
        return Err("The background preview exceeds its size bound.".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "The background preview is invalid.")?;
    if bytes.len() > byte_limit {
        return Err("The background preview exceeds its size bound.".into());
    }
    let (width, height) =
        image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Jpeg)
            .into_dimensions()
            .map_err(|_| "The background preview is not a JPEG image.")?;
    if width == 0 || height == 0 || width > edge || height > edge {
        return Err("The background preview exceeds its dimension bound.".into());
    }
    Ok(bytes)
}

use super::*;
#[path = "discovery_cache.rs"]
mod discovery_cache;

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

/// Small transient thumbnails keep scan snapshots bounded without publishing assets.
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
    let image = decoded(&local_bytes(path.to_str().ok_or("Invalid artwork path.")?)?)?;
    for size in [384, 320, 256, 192, 128] {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
            .encode_image(&image.thumbnail(size, size).to_rgb8())
            .map_err(|error| error.to_string())?;
        if bytes.len() <= THUMBNAIL_LIMIT {
            return Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)));
        }
    }
    Err("Discovery artwork exceeds the thumbnail budget.".into())
}

pub(super) fn preview_bytes(data: &str) -> Result<Vec<u8>, String> {
    let encoded = data
        .strip_prefix("data:image/jpeg;base64,")
        .ok_or("The background preview is invalid. Load the image again.")?;
    if encoded.len() > THUMBNAIL_LIMIT * 4 / 3 + 4 {
        return Err("The background preview exceeds its size bound.".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "The background preview is invalid.")?;
    if bytes.len() > THUMBNAIL_LIMIT {
        return Err("The background preview exceeds its size bound.".into());
    }
    let (width, height) =
        image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Jpeg)
            .into_dimensions()
            .map_err(|_| "The background preview is not a JPEG image.")?;
    if width == 0 || height == 0 || width > 640 || height > 640 {
        return Err("The background preview exceeds its dimension bound.".into());
    }
    Ok(bytes)
}

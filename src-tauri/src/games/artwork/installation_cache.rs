//! Replaceable platform artwork, separate from the user's saved game metadata.
use super::*;

const MAX_FILES: usize = 512;

fn directory(root: &Path) -> Result<PathBuf, String> {
    storage(root)?; // Validate the application directory and all existing ancestors.
    let path = root.join("game-platform-artwork");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if redirected(&metadata) || !metadata.is_dir() => {
            Err("Invalid platform artwork cache directory.".into())
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
        _ => Ok(path),
    }
}

fn filename(installation: &str) -> Result<String, String> {
    if installation.trim().is_empty()
        || installation.len() > 4096
        || installation.chars().any(char::is_control)
    {
        return Err("Platform artwork needs an installation path.".into());
    }
    let identity = crate::game_discovery::normalized_path(installation);
    Ok(format!("{:x}.jpg", Sha256::digest(identity.trim_end_matches('\\').as_bytes())))
}

fn valid_file(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if redirected(&metadata) || !metadata.is_file() => {
            Err("Invalid platform artwork cache file.".into())
        }
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn cache_installation_preview(
    app_root: &Path,
    installation: &str,
    image: &str,
) -> Result<(), String> {
    let bytes = discovery_preview_bytes(image)?;
    static WRITES: Mutex<()> = Mutex::new(());
    let _write = WRITES.lock().unwrap_or_else(|poison| poison.into_inner());
    let directory = directory(app_root)?;
    let path = directory.join(filename(installation)?);
    let existing = valid_file(&path)?.is_some();
    if installation_preview(app_root, installation).as_deref() == Some(image) {
        return Ok(());
    }
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    // Recheck after creating the directory before writing inside it.
    self::directory(app_root)?;
    if !existing
        && fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .take(MAX_FILES + 1)
            .count()
            >= MAX_FILES
    {
        return Err("Platform artwork cache reached its image limit.".into());
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
    temporary
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    valid_file(&path)?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn installation_preview(app_root: &Path, installation: &str) -> Option<String> {
    let path = directory(app_root).ok()?.join(filename(installation).ok()?);
    let metadata = valid_file(&path).ok()??;
    if metadata.len() > DISCOVERY_PREVIEW_LIMIT as u64 {
        return None;
    }
    let modified = metadata.modified().ok()?;
    sources::discovery_cache::get_or_prepare(&path, modified, metadata.len(), || {
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .map_err(|error| error.to_string())?
            .take(DISCOVERY_PREVIEW_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let image = format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes));
        discovery_preview_bytes(&image)?; // Byte and JPEG header bounds; no pixel decoding.
        Ok(image)
    })
    .ok()
}

#[cfg(test)]
#[path = "installation_cache_tests.rs"]
mod tests;

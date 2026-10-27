//! Bounded Steam Store genre enrichment keyed only by an exact Steam app ID.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{mpsc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAX_APP_IDS: usize = 512;
const MAX_LABELS: usize = 64;
const MAX_LABEL_CHARS: usize = 96;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_CACHE_BYTES: u64 = 16 * 1024;
const MAX_CACHE_ENTRIES: usize = 512;
const WORKERS: usize = 4;
const TOTAL_DEADLINE: Duration = Duration::from_secs(8);
const MAX_REQUEST: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CachedGenres {
    app_id: u32,
    genres: Vec<String>,
}

/// Return store genre labels for exact Steam app IDs. Unavailable metadata is
/// deliberately omitted so it cannot prevent local game discovery.
pub(crate) fn load(
    root: &Path,
    app_ids: &[u32],
    cancelled: impl Fn() -> bool + Sync,
) -> BTreeMap<u32, Vec<String>> {
    load_with(root, app_ids, &cancelled, fetch)
}

fn load_with<F, C>(
    root: &Path,
    app_ids: &[u32],
    cancelled: &C,
    fetch_metadata: F,
) -> BTreeMap<u32, Vec<String>>
where
    F: Fn(u32, Duration) -> Result<Vec<u8>, String> + Sync,
    C: Fn() -> bool + Sync,
{
    load_using(root,app_ids,cancelled,fetch_metadata,false)
}

pub(crate) fn refresh(root:&Path,app_ids:&[u32],cancelled:impl Fn()->bool+Sync)->BTreeMap<u32,Vec<String>> {
    load_using(root,app_ids,&cancelled,fetch,true)
}

fn load_using<F,C>(root:&Path,app_ids:&[u32],cancelled:&C,fetch_metadata:F,refresh:bool)->BTreeMap<u32,Vec<String>>
where F:Fn(u32,Duration)->Result<Vec<u8>,String>+Sync,C:Fn()->bool+Sync {
    let started = Instant::now();
    let directory = cache_directory(root, true);
    let ids = app_ids
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .take(MAX_APP_IDS)
        .collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    let mut missing = VecDeque::new();
    for app_id in ids {
        if cancelled() || started.elapsed() >= TOTAL_DEADLINE {
            break;
        }
        if let Some(genres) = directory
            .as_deref()
            .and_then(|directory| read_cache(directory, app_id))
        {
            result.insert(app_id, genres);
            if refresh {missing.push_back(app_id);}
        } else {
            missing.push_back(app_id);
        }
    }
    if missing.is_empty() || cancelled() || started.elapsed() >= TOTAL_DEADLINE {
        return result;
    }

    let jobs = Mutex::new(missing);
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            let sender = sender.clone();
            let jobs = &jobs;
            let fetch_metadata = &fetch_metadata;
            let cancelled = cancelled;
            scope.spawn(move || loop {
                if cancelled() {
                    break;
                }
                let Some(app_id) = jobs.lock().unwrap_or_else(|p| p.into_inner()).pop_front()
                else {
                    break;
                };
                let remaining = TOTAL_DEADLINE.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    break;
                }
                let timeout = MAX_REQUEST.min(remaining);
                let genres = fetch_metadata(app_id, timeout)
                    .ok()
                    .and_then(|bytes| parse_response(app_id, &bytes));
                if let Some(genres) = genres {
                    let _ = sender.send((app_id, genres));
                }
            });
        }
    });
    drop(sender);

    for (app_id, genres) in receiver {
        if let Some(directory) = directory.as_deref() {
            let _ = write_cache(directory, app_id, &genres);
        }
        result.insert(app_id, genres);
    }
    result
}

fn fetch(app_id: u32, timeout: Duration) -> Result<Vec<u8>, String> {
    if app_id == 0 || timeout.is_zero() {
        return Err("Invalid Steam metadata request.".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .timeout_connect(Duration::from_secs(2).min(timeout))
        .timeout_read(timeout)
        .redirects(0)
        .build();
    let url = metadata_url(app_id);
    let response = agent
        .get(&url)
        .set("User-Agent", "OpenSimGear/0.1 platform metadata")
        .call()
        .map_err(|error| format!("Steam metadata request failed: {error}"))?;
    read_bounded(response.into_reader(), MAX_RESPONSE_BYTES)
}

fn metadata_url(app_id: u32) -> String {
    format!(
        "https://store.steampowered.com/api/appdetails?appids={app_id}&filters=basic,genres,categories&l=english"
    )
}

fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("Steam metadata response exceeds its size limit.".into());
    }
    Ok(bytes)
}

fn parse_response(app_id: u32, bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return None;
    }
    let metadata: Value = serde_json::from_slice(bytes).ok()?;
    let entry = metadata.get(app_id.to_string())?;
    if entry["success"] != true {
        return None;
    }
    let data = entry.get("data")?;
    if data["steam_appid"].as_u64() != Some(u64::from(app_id)) {
        return None;
    }
    let Some(genres) = data.get("genres") else {
        return Some(Vec::new());
    };
    let genres = genres.as_array()?;
    if genres.len() > MAX_LABELS {
        return None;
    }
    let mut labels = BTreeSet::new();
    for genre in genres {
        let description = genre.get("description")?.as_str()?.trim();
        if description.is_empty()
            || description.chars().count() > MAX_LABEL_CHARS
            || description.chars().any(char::is_control)
        {
            return None;
        }
        labels.insert(description.to_owned());
    }
    Some(labels.into_iter().collect())
}

fn cache_directory(root: &Path, create: bool) -> Option<PathBuf> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || !safe_directory_chain(root, false)
    {
        return None;
    }
    let directory = root.join("platform-tags");
    if create && !directory.exists() {
        fs::create_dir(&directory).ok()?;
    }
    safe_directory_chain(&directory, false).then_some(directory)
}

fn safe_directory_chain(path: &Path, allow_missing: bool) -> bool {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if redirected(&metadata) || !metadata.is_dir() => return false,
            Ok(_) => {}
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {
                return true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
            Err(_) => return false,
        }
    }
    true
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

fn read_cache(directory: &Path, app_id: u32) -> Option<Vec<String>> {
    let path = directory.join(format!("{app_id}.json"));
    let metadata = fs::symlink_metadata(&path).ok()?;
    if redirected(&metadata) || !metadata.is_file() || metadata.len() > MAX_CACHE_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(path)
        .ok()?
        .take(MAX_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return None;
    }
    let cached: CachedGenres = serde_json::from_slice(&bytes).ok()?;
    (cached.app_id == app_id && valid_labels(&cached.genres)).then_some(cached.genres)
}

fn write_cache(directory: &Path, app_id: u32, genres: &[String]) -> bool {
    if !valid_labels(genres) || !safe_directory_chain(directory, false) {
        return false;
    }
    let _write_guard = cache_write_lock().lock().unwrap_or_else(|p| p.into_inner());
    let path = directory.join(format!("{app_id}.json"));
    let exists = match fs::symlink_metadata(&path) {
        Ok(metadata) if !redirected(&metadata) && metadata.is_file() => true,
        Ok(_) => return false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return false,
    };
    if !exists && !has_cache_capacity(directory) {
        return false;
    }
    let Ok(bytes) = serde_json::to_vec(&CachedGenres {
        app_id,
        genres: genres.to_vec(),
    }) else {
        return false;
    };
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return false;
    }
    let Ok(mut temporary) = tempfile::NamedTempFile::new_in(directory) else {
        return false;
    };
    if temporary.write_all(&bytes).is_err() {
        return false;
    }
    temporary.persist(path).is_ok()
}

fn cache_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn has_cache_capacity(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    let mut count = 0;
    for entry in entries.take(MAX_CACHE_ENTRIES + 1) {
        if entry.is_err() {
            return false;
        }
        count += 1;
    }
    count < MAX_CACHE_ENTRIES
}

fn valid_labels(labels: &[String]) -> bool {
    labels.len() <= MAX_LABELS
        && labels.iter().all(|label| {
            !label.trim().is_empty()
                && label.trim() == label
                && label.chars().count() <= MAX_LABEL_CHARS
                && !label.chars().any(char::is_control)
        })
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod tests;

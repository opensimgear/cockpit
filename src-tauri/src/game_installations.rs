//! Fresh installation metadata only. No profile, binding or runtime owner is
//! accepted here; callers supply this store's new explicit path.
use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub(crate) const MAX_BYTES: usize = 40 * 1024;
pub(crate) const MAX_EXECUTABLE_BYTES: usize = 4096;
const GAMES: [&str; 7] = ["dcs", "assetto-corsa-rally", "x-plane-11", "x-plane-12", "assetto-corsa", "assetto-corsa-competizione", "assetto-corsa-evo"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub schema_version: u32,
    #[serde(deserialize_with = "distinct_installations")]
    pub manual_installations: BTreeMap<String, String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            manual_installations: BTreeMap::new(),
        }
    }
}
fn distinct_installations<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Installations;
    impl<'de> Visitor<'de> for Installations {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("at most seven distinct native game installations")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = BTreeMap::new();
            while let Some((id, path)) = map.next_entry::<String, String>()? {
                if entries.len() == GAMES.len() || entries.insert(id, path).is_some() {
                    return Err(de::Error::custom(
                        "Duplicate or excessive native game installation",
                    ));
                }
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Installations)
}
fn executable(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_EXECUTABLE_BYTES
        && !path.chars().any(char::is_control)
        && (Path::new(path).is_absolute()
            || (path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && path.as_bytes().get(1) == Some(&b':')
                && matches!(path.as_bytes().get(2), Some(b'\\' | b'/'))))
}
impl Settings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.manual_installations.len() > GAMES.len()
            || self
                .manual_installations
                .iter()
                .any(|(id, path)| !GAMES.contains(&id.as_str()) || !executable(path))
        {
            return Err("Installation metadata requires supported games and bounded absolute executable paths.".into());
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_BYTES {
            return Err("Installation metadata exceeds its native size bound.".into());
        }
        Ok(())
    }
}

pub(crate) struct Store {
    path: PathBuf,
}
fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    false
}
fn check_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(
            "Choose an absolute native installation metadata file without parent traversal.".into(),
        );
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        // A bare verbatim drive prefix is not an openable Windows path.
        // Inspect it only once the following root component has been added.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if redirected(&metadata) => {
                return Err("Installation metadata cannot use a symlink or reparse point.".into())
            }
            Ok(metadata) if current != path && !metadata.is_dir() => {
                return Err("An installation metadata parent is not a directory.".into())
            }
            Ok(metadata) if current == path && !metadata.is_file() => {
                return Err("Installation metadata must be a regular file.".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
impl Store {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub(crate) fn load(&self) -> Result<Option<Settings>, String> {
        check_path(&self.path)?;
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err("Installation metadata exceeds its native size bound.".into());
        }
        let settings: Settings = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid installation metadata: {e}"))?;
        settings.validate()?;
        Ok(Some(settings))
    }
    pub(crate) fn save(&self, settings: &Settings) -> Result<(), String> {
        self.save_with(settings, |temporary, path| {
            temporary
                .persist(path)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }
    fn save_with(
        &self,
        settings: &Settings,
        replace: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<(), String>,
    ) -> Result<(), String> {
        settings.validate()?;
        check_path(&self.path)?;
        let parent = self
            .path
            .parent()
            .ok_or("Installation metadata has no parent directory.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        check_path(&self.path)?;
        let bytes = serde_json::to_vec(settings).map_err(|e| e.to_string())?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        temporary.write_all(&bytes).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let directory = fs::File::open(parent).map_err(|e| e.to_string())?;
        replace(temporary, &self.path)?;
        // Replacement is the commit boundary. A best-effort directory flush
        // must not report rollback after the new file has already committed.
        #[cfg(unix)]
        let _ = directory.sync_all();
        Ok(())
    }
}

#[cfg(test)]
#[path = "game_installations_tests.rs"]
mod tests;

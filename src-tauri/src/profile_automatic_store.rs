//! Machine-local activation rules, separate from portable profile graphs and
//! transient process proofs. Missing configuration is Manual/Default; malformed
//! configuration is preserved and must never enable background activation.
use osg_profiles::{
    activation::{validate_rules, Rule},
    session::SelectionMode,
    DEFAULT_PROFILE_ID,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
const MAX_BYTES: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    pub schema_version: u32,
    pub mode: SelectionMode,
    pub manual_profile_id: String,
    pub rules: Vec<Rule>,
    /// User choice among profiles sharing a saved game context. Contexts are
    /// revalidated against profile storage on every read and launch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preferred_profiles: BTreeMap<String, String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mode: SelectionMode::Manual,
            manual_profile_id: DEFAULT_PROFILE_ID.into(),
            rules: vec![],
            preferred_profiles: BTreeMap::new(),
        }
    }
}
impl Settings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !uuid::Uuid::parse_str(&self.manual_profile_id)
                .is_ok_and(|id| id.to_string() == self.manual_profile_id)
        {
            return Err("Invalid native automatic profile settings identity/version.".into());
        }
        validate_rules(&self.rules)?;
        if self.preferred_profiles.len() > osg_profiles::activation::MAX_RULES
            || self.preferred_profiles.iter().any(|(game, profile)| {
                !osg_profiles::activation::valid_game_id(game)
                    || !uuid::Uuid::parse_str(profile).is_ok_and(|id| id.to_string() == *profile)
            })
        {
            return Err(
                "Remembered launch choices need valid games and saved profile identities."
                    .into(),
            );
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_BYTES {
            return Err("Automatic profile settings exceed their native storage bound.".into());
        }
        Ok(())
    }
}
pub(crate) struct Store(Option<PathBuf>);
impl Store {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self(path)
    }
    pub(crate) fn load(&self) -> Result<Option<Settings>, String> {
        let Some(path) = &self.0 else {
            return Ok(None);
        };
        check(path, false)?;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = vec![];
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err("Automatic profile settings exceed their native storage bound.".into());
        }
        let settings: Settings = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Automatic profile settings are invalid: {e}"))?;
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
        let Some(path) = &self.0 else {
            return Ok(());
        };
        check(path, true)?;
        let parent = path
            .parent()
            .ok_or("Invalid automatic profile settings path.")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        temporary
            .write_all(&serde_json::to_vec(settings).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        check(path, false)?;
        replace(temporary, path)?;
        #[cfg(unix)]
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    }
}
fn reject_link(metadata: &std::fs::Metadata) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Automatic profile settings cannot use a reparse point.".into());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err("Automatic profile settings cannot follow a symbolic link.".into());
    }
    Ok(())
}
fn check(path: &Path, create: bool) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(
            "Automatic profile settings require an absolute native path without traversal.".into(),
        );
    }
    let parent = path
        .parent()
        .ok_or("Invalid automatic profile settings parent.")?;
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                reject_link(&metadata)?;
                if !metadata.is_dir() {
                    return Err("Automatic profile settings parent is not a directory.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if create {
                    std::fs::create_dir(&current).map_err(|e| e.to_string())?;
                    reject_link(&std::fs::symlink_metadata(&current).map_err(|e| e.to_string())?)?;
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            reject_link(&metadata)?;
            if !metadata.is_file() {
                return Err("Automatic profile settings are not a regular file.".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}
#[cfg(test)]
#[path = "profile_automatic_store_tests.rs"]
mod tests;

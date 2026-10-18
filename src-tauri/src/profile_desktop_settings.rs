//! Machine-local desktop enablement and application selection. Loading or saving
//! these preferences never acquires an injector or starts a profile.
use serde::{Deserialize, Serialize};
use std::{io::{Read, Write}, path::{Path, PathBuf}};

pub(crate) fn qualified() -> bool {
    cfg!(all(windows, debug_assertions)) && option_env!("OSG_DEV_DESKTOP_INPUT") == Some("1")
}
pub(crate) const QUALIFICATION: &str = "Keyboard and mouse output is awaiting Windows qualification. Use an explicit OSG_DEV_DESKTOP_INPUT=1 debug build for development acceptance.";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Scope {
    #[default]
    Application,
    DesktopWide,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Preferences {
    schema: u32,
    pub enabled: bool,
    pub scope: Scope,
    /// Selected through native observation, never accepted as an IPC path/PID.
    pub application: Option<serde_json::Value>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self { schema: 1, enabled: false, scope: Scope::Application, application: None }
    }
}

pub(crate) struct Settings {
    path: Option<PathBuf>,
    pub value: Preferences,
    pub error: Option<String>,
}
impl Settings {
    pub(crate) fn open(path: Option<PathBuf>) -> Self {
        let loaded = path.as_ref().map_or_else(|| Ok(Preferences::default()), |path| load(path));
        match loaded {
            Ok(value) => Self { path, value, error: None },
            Err(error) => Self { path, value: Preferences::default(), error: Some(error) },
        }
    }
    pub(crate) fn native() -> Self {
        Self::open(Some(crate::config_paths::config_path().with_file_name("desktop-input-v1.json")))
    }
    pub(crate) fn configure(&mut self, enabled: bool, scope: Scope) -> Result<(), String> {
        let mut next = self.value.clone();
        next.enabled = enabled;
        next.scope = scope;
        self.save(next)
    }
    pub(crate) fn application(&mut self, identity: serde_json::Value) -> Result<(), String> {
        let mut next = self.value.clone();
        next.scope = Scope::Application;
        next.application = Some(identity);
        self.save(next)
    }
    fn save(&mut self, next: Preferences) -> Result<(), String> {
        if self.error.is_some() {
            return Err("Existing desktop input settings are invalid; preserve and repair the file before changing enablement.".into());
        }
        validate(&next)?;
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or("Missing desktop settings directory.")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            temporary.write_all(&serde_json::to_vec(&next).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            temporary.as_file().sync_all().map_err(|e| e.to_string())?;
            temporary.persist(path).map_err(|e| e.to_string())?;
        }
        self.value = next;
        Ok(())
    }
}
fn validate(value: &Preferences) -> Result<(), String> {
    if value.schema != 1 { return Err("Unsupported desktop input settings schema.".into()); }
    if let Some(identity) = &value.application {
        let process = crate::game_discovery::ProcessIdentity::parse(identity)?;
        if process.value() != *identity { return Err("Desktop application identity has unknown fields.".into()); }
    }
    Ok(())
}
fn load(path: &Path) -> Result<Preferences, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(e) => return Err(e.to_string()),
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => return Err("Desktop input settings must be a regular local file.".into()),
        _ => {}
    }
    let mut bytes = vec![];
    std::fs::File::open(path).map_err(|e| e.to_string())?.take(16_385).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 16_384 { return Err("Desktop input settings exceed 16 KiB.".into()); }
    let value = serde_json::from_slice(&bytes).map_err(|e| format!("Invalid desktop input settings: {e}"))?;
    validate(&value)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enablement_is_explicit_and_invalid_settings_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop.json");
        let mut settings = Settings::open(Some(path.clone()));
        assert!(!settings.value.enabled);
        assert_eq!(settings.value.scope, Scope::Application);
        settings.configure(true, Scope::DesktopWide).unwrap();
        let restored = Settings::open(Some(path.clone()));
        assert!(restored.value.enabled);
        assert_eq!(restored.value.scope, Scope::DesktopWide);
        std::fs::write(&path, b"{invalid").unwrap();
        let mut invalid = Settings::open(Some(path.clone()));
        assert!(invalid.error.is_some());
        assert!(!invalid.value.enabled);
        assert!(invalid.configure(true, Scope::DesktopWide).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"{invalid");
    }
}

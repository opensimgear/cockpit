//! Machine-local desktop enablement and application selection. Loading or saving
//! these preferences never acquires an injector or starts a profile.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::{Read, Write}, path::{Path, PathBuf}};

pub(crate) fn qualified() -> bool {
    osg_desktop_output::qualification_gate()
}
pub(crate) const QUALIFICATION: &str = "Native keyboard and mouse output is available on Windows only.";

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
    /// Immutable native observations, referenced by individual saved endpoints.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applications: BTreeMap<String, serde_json::Value>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self { schema: 1, enabled: false, scope: Scope::Application, application: None, applications: BTreeMap::new() }
    }
}

pub(crate) struct Settings {
    path: Option<PathBuf>,
    pub value: Preferences,
    pub error: Option<String>,
}
impl Settings {
    pub(crate) fn revision(&self) -> String {
        use sha2::{Digest, Sha256};
        let capture = serde_json::json!({"preferences":self.value,"error":self.error});
        format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&capture).expect("JSON value")))
    }
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
    pub(crate) fn capture_application(&mut self, identity: serde_json::Value) -> Result<String, String> {
        if let Some((reference, _)) = self.value.applications.iter().find(|(_, saved)| **saved == identity) {
            return Ok(reference.clone());
        }
        if self.value.applications.len() >= 256 { return Err("The captured application library is full. Remove unused desktop settings before capturing more applications.".into()); }
        let mut next = self.value.clone();
        let reference = uuid::Uuid::new_v4().to_string();
        next.applications.insert(reference.clone(), identity);
        self.save(next)?;
        Ok(reference)
    }
    fn save(&mut self, next: Preferences) -> Result<(), String> {
        if self.error.is_some() {
            return Err("Existing desktop input settings are invalid; preserve and repair the file before changing enablement.".into());
        }
        validate(&next)?;
        let bytes = serde_json::to_vec(&next).map_err(|e| e.to_string())?;
        if bytes.len() > 524_288 { return Err("Desktop input settings exceed 512 KiB.".into()); }
        if let Some(path) = &self.path {
            let current = load(path)?;
            if serde_json::to_value(&current).map_err(|e|e.to_string())? != serde_json::to_value(&self.value).map_err(|e|e.to_string())? {
                return Err("Desktop input settings changed on disk. Reload their current settings before saving.".into());
            }
            let parent = path.parent().ok_or("Missing desktop settings directory.")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            temporary.write_all(&bytes).map_err(|e| e.to_string())?;
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
    if value.applications.len() > 256 { return Err("Too many captured desktop applications.".into()); }
    for (reference, identity) in &value.applications {
        if uuid::Uuid::parse_str(reference).ok().map(|id|id.to_string()).as_deref() != Some(reference.as_str()) {
            return Err("Invalid captured desktop application reference.".into());
        }
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
    std::fs::File::open(path).map_err(|e| e.to_string())?.take(524_289).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 524_288 { return Err("Desktop input settings exceed 512 KiB.".into()); }
    let value = serde_json::from_slice(&bytes).map_err(|e| format!("Invalid desktop input settings: {e}"))?;
    validate(&value)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_desktop_preferences_preserve_external_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop.json");
        let mut first = Settings::open(Some(path.clone()));
        let mut second = Settings::open(Some(path.clone()));
        let revision = first.revision();
        first.configure(true, Scope::DesktopWide).unwrap();
        assert_ne!(first.revision(),revision);
        assert!(second.configure(false, Scope::Application).unwrap_err().contains("changed on disk"));
        let restored = Settings::open(Some(path));
        assert!(restored.value.enabled);
        assert_eq!(restored.value.scope,Scope::DesktopWide);
        assert_eq!(restored.revision(),first.revision());
    }
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
    #[test]
    fn captured_application_references_are_immutable_local_and_survive_reload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop.json");
        let mut settings = Settings::open(Some(path.clone()));
        let first = serde_json::json!({"pid":42,"path":"C:\\Fixture\\First.exe","epoch":10});
        let second = serde_json::json!({"pid":43,"path":"C:\\Fixture\\Second.exe","epoch":11});
        let first_reference = settings.capture_application(first.clone()).unwrap();
        assert_eq!(settings.capture_application(first.clone()).unwrap(), first_reference);
        let second_reference = settings.capture_application(second.clone()).unwrap();
        assert_ne!(first_reference, second_reference);
        let restored = Settings::open(Some(path));
        assert_eq!(restored.value.applications[&first_reference], first);
        assert_eq!(restored.value.applications[&second_reference], second);
        assert!(restored.value.application.is_none());
        assert!(!restored.value.enabled, "capturing an application does not enable output");
    }
}

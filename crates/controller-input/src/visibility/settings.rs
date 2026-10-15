//! Local intent is separate from applied driver policy. Saving never hides anything.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Intent {
    #[default]
    Visible,
    HideAll,
    BlockOriginalInput,
    HideFromProcess {
        executable: String,
    },
}
impl Intent {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::HideFromProcess { executable } = self {
            let b = executable.as_bytes();
            if b.len() < 7
                || b.len() > 4096
                || !b[0].is_ascii_alphabetic()
                || b[1] != b':'
                || b[2] != b'\\'
                || !executable.to_lowercase().ends_with(".exe")
                || executable.chars().any(char::is_control)
                || executable
                    .split('\\')
                    .any(|part| part == ".." || part == ".")
            {
                return Err("Choose an absolute local executable path; process IDs and relative paths are not saved.".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub schema: u32,
    pub enabled: bool,
    pub devices: BTreeMap<String, Intent>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema: 1,
            enabled: false,
            devices: BTreeMap::new(),
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || self.devices.len() > 256 {
            return Err("Unsupported visibility settings.".into());
        }
        for (source, intent) in &self.devices {
            if uuid::Uuid::parse_str(source)
                .ok()
                .map(|u| u.to_string())
                .as_deref()
                != Some(source)
            {
                return Err("Unknown local controller identity.".into());
            }
            intent.validate()?;
        }
        Ok(())
    }
}
fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(m) if !m.is_file() || m.file_type().is_symlink() || m.len() > 1_048_576 => {
            return Err("Visibility settings must be a bounded regular file.".into())
        }
        Ok(_) => {}
    }
    let mut bytes = vec![];
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("Visibility settings are oversized.".into());
    }
    Ok(Some(bytes))
}
pub struct Settings {
    pub value: Preferences,
    pub error: Option<String>,
    path: Option<PathBuf>,
    expected: Option<Vec<u8>>,
}
impl Settings {
    pub fn new(path: Option<PathBuf>) -> Self {
        let loaded = path
            .as_ref()
            .map_or(Ok(None), |p| read(p))
            .and_then(|bytes| {
                let value = bytes.as_ref().map_or(Ok(Preferences::default()), |b| {
                    serde_json::from_slice::<Preferences>(b).map_err(|e| e.to_string())
                })?;
                value.validate()?;
                Ok((value, bytes))
            });
        match loaded {
            Ok((value, expected)) => Self {
                value,
                error: None,
                path,
                expected,
            },
            Err(error) => Self {
                value: Preferences::default(),
                error: Some(error),
                path,
                expected: None,
            },
        }
    }
    pub fn save(&mut self, value: Preferences) -> Result<(), String> {
        value.validate()?;
        if self.error.is_some() {
            return Err(
                "Repair existing visibility settings before saving; they were preserved.".into(),
            );
        }
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or("Invalid settings path.")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let lock_path = path.with_extension("lock");
            if std::fs::symlink_metadata(&lock_path)
                .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
            {
                return Err("Invalid settings lock file.".into());
            }
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .read(true)
                .open(lock_path)
                .map_err(|e| e.to_string())?;
            lock.try_lock()
                .map_err(|_| "Visibility settings are being edited in another Cockpit.")?;
            if read(path)? != self.expected {
                return Err("Visibility settings changed elsewhere; reload before editing.".into());
            }
            let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
            if bytes.len() > 1_048_576 {
                return Err("Visibility settings exceed their bound.".into());
            }
            let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.as_file().sync_all().map_err(|e| e.to_string())?;
            file.persist(path).map_err(|e| e.to_string())?;
            self.expected = Some(bytes);
        }
        self.value = value;
        Ok(())
    }
}
#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

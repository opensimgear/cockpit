//! Local intent is separate from applied driver policy. Saving never hides anything.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
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
pub struct Settings {
    pub value:Preferences,
    pub error:Option<String>,
    store:Option<osg_persistence::Repository<Preferences>>,
}
impl Settings {
    pub fn new(path:Option<PathBuf>)->Self {
        let storage=path.map(|path| {
            let db=osg_persistence::Database::for_source(&path)?;
            osg_persistence::Repository::new(db,"controllers","visibility",1,Some(path),1_048_576,Preferences::validate)
        }).transpose();
        let loaded=storage.as_ref().map_err(Clone::clone).and_then(|store|store.as_ref().map_or(Ok(Preferences::default()),|store|store.load().map(|value|value.unwrap_or_default())));
        match loaded {
            Ok(value)=>Self {value,error:None,store:storage.ok().flatten()},
            Err(error)=>Self {value:Preferences::default(),error:Some(error),store:storage.ok().flatten()},
        }
    }
    pub fn save(&mut self,value:Preferences)->Result<(),String> {
        value.validate()?;
        if self.error.is_some() {return Err("Repair existing visibility settings before saving; they were preserved.".into());}
        if let Some(store)=&self.store {store.save(&value)?;}
        self.value=value;Ok(())
    }
}
#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

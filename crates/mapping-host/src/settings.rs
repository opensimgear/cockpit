//! Optional integration enablement and slot assignments are local, never portable.
use osg_mapping::profile::Profile;
use osg_virtual_controller::vjoy::Binding;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::PathBuf,
};
const LIMIT: u64 = 524_288;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    schema: u32,
    pub virtual_devices: bool,
    pub device_hiding: bool,
    pub targets: BTreeMap<String, Vec<Binding>>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema: 1,
            virtual_devices: false,
            device_hiding: false,
            targets: BTreeMap::new(),
        }
    }
}
pub struct Settings {
    path: Option<PathBuf>,
    pub value: Preferences,
    pub error: Option<String>,
}
impl Settings {
    pub fn open(path: Option<PathBuf>) -> Self {
        let result = (|| {
            let Some(path) = &path else {
                return Ok(Preferences::default());
            };
            match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Preferences::default())
                }
                Err(e) => return Err(e.to_string()),
                Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                    return Err("Mapping settings must be a regular local file.".into())
                }
                _ => {}
            }
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > LIMIT {
                return Err("Mapping settings exceed 512 KiB.".into());
            }
            let value: Preferences = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            validate(&value)?;
            Ok(value)
        })();
        match result {
            Ok(value) => Self {
                path,
                value,
                error: None,
            },
            Err(error) => Self {
                path,
                value: Preferences::default(),
                error: Some(error),
            },
        }
    }
    fn save(&mut self, value: Preferences) -> Result<(), String> {
        if self.error.is_some() {
            return Err("Existing mapping settings are invalid; preserve and repair them before changing preferences.".into());
        }
        validate(&value)?;
        let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LIMIT {
            return Err("Mapping settings exceed 512 KiB.".into());
        }
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or("Missing settings directory.")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            temp.write_all(&bytes).map_err(|e| e.to_string())?;
            temp.as_file().sync_all().map_err(|e| e.to_string())?;
            temp.persist(path).map_err(|e| e.to_string())?;
        }
        self.value = value;
        Ok(())
    }
    pub fn enable_virtual(&mut self, enabled: bool) -> Result<(), String> {
        let mut next = self.value.clone();
        next.virtual_devices = enabled;
        self.save(next)
    }
    pub fn assign_targets(
        &mut self,
        profile: &Profile,
        bindings: Vec<Binding>,
    ) -> Result<(), String> {
        validate_bindings(profile, &bindings)?;
        let mut next = self.value.clone();
        next.targets.insert(profile.id.clone(), bindings);
        self.save(next)
    }
}
pub fn validate_bindings(profile: &Profile, bindings: &[Binding]) -> Result<(), String> {
    profile.compile()?;
    if bindings.len() != profile.targets.len() {
        return Err("Assign exactly one local slot to each logical target.".into());
    }
    let mut targets = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for binding in bindings {
        binding.validate()?;
        if !targets.insert(&binding.target) || !slots.insert(binding.slot) {
            return Err("Targets and vJoy slots must be unique.".into());
        }
        let target = profile
            .targets
            .iter()
            .find(|t| t.id == binding.target)
            .ok_or("Unknown logical target.")?;
        if serde_json::to_value(&target.layout).unwrap()
            != serde_json::to_value(&binding.layout).unwrap()
        {
            return Err(
                "Slot assignment must preserve the profile's exact layout and fallback.".into(),
            );
        }
    }
    Ok(())
}
fn validate(value: &Preferences) -> Result<(), String> {
    if value.schema != 1 || value.targets.len() > 128 {
        return Err("Invalid local mapping settings schema or profile limit.".into());
    }
    for (id, bindings) in &value.targets {
        if uuid::Uuid::parse_str(id)
            .ok()
            .map(|u| u.to_string())
            .as_deref()
            != Some(id)
            || bindings.is_empty()
            || bindings.len() > 8
        {
            return Err("Invalid mapping target assignment.".into());
        }
        let mut targets = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for binding in bindings {
            binding.validate()?;
            if !targets.insert(&binding.target) || !slots.insert(binding.slot) {
                return Err("Targets and vJoy slots must be unique.".into());
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Profile {
        Profile::parse(include_bytes!("../../mapping/fixtures/direct-profile.json")).unwrap()
    }
    fn bindings(p: &Profile) -> Vec<Binding> {
        p.targets
            .iter()
            .enumerate()
            .map(|(i, t)| Binding {
                target: t.id.clone(),
                slot: (i + 1) as u8,
                axes: vec![0x30; t.layout.axes.len()],
                layout: t.layout.clone(),
            })
            .collect()
    }
    #[test]
    fn preferences_and_targets_persist_independently_without_activation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local.json");
        let p = profile();
        let mut settings = Settings::open(Some(path.clone()));
        assert!(!settings.value.virtual_devices);
        assert!(!settings.value.device_hiding);
        settings.enable_virtual(true).unwrap();
        settings.assign_targets(&p, bindings(&p)).unwrap();
        let restored = Settings::open(Some(path));
        assert!(restored.value.virtual_devices);
        assert!(!restored.value.device_hiding);
        assert_eq!(restored.value.targets[&p.id].len(), 2);
        settings.enable_virtual(false).unwrap();
        assert_eq!(
            settings.value.targets[&p.id][0].layout.axes[0].fallback,
            -123
        );
        assert!(!p.export().unwrap().windows(4).any(|x| x == b"slot"));
        let mut memory = Settings::open(None);
        memory.enable_virtual(true).unwrap();
        assert!(memory.value.virtual_devices);
    }
    #[test]
    fn corrupt_existing_settings_are_preserved_and_bad_shapes_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local.json");
        for bytes in [b"broken".to_vec(), vec![b' '; LIMIT as usize + 1]] {
            std::fs::write(&path, &bytes).unwrap();
            let mut settings = Settings::open(Some(path.clone()));
            assert!(settings.error.is_some());
            assert!(settings.enable_virtual(true).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        assert!(Settings::open(Some(dir.path().to_owned())).error.is_some());
        let mut s = Settings::open(None);
        let p = profile();
        let b = bindings(&p);
        assert!(s.assign_targets(&p, vec![]).is_err());
        let mut invalid = b.clone();
        invalid[0].target = "unknown".into();
        assert!(s.assign_targets(&p, invalid).is_err());
        let mut invalid = b.clone();
        invalid[0].layout.buttons += 1;
        assert!(s.assign_targets(&p, invalid).is_err());
        let mut invalid = b.clone();
        invalid[1].slot = invalid[0].slot;
        assert!(s.assign_targets(&p, invalid).is_err());
        let mut pref = Preferences::default();
        pref.schema = 2;
        assert!(validate(&pref).is_err());
        pref.schema = 1;
        pref.targets.insert("bad".into(), b);
        assert!(validate(&pref).is_err());
        pref.targets.clear();
        pref.targets.insert(p.id.clone(), vec![]);
        assert!(validate(&pref).is_err());
    }
}

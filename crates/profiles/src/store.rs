//! The new profile library has one bounded, atomically replaced envelope. It
//! never scans legacy profile locations or mutates driver-owned configuration.
use crate::{default_document, validate_document, ProfileDocument, DEFAULT_PROFILE_ID};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub use crate::{MAX_LIBRARY_BYTES, MAX_PROFILES};
pub const LIBRARY_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalAssignment {
    pub resource_id: String,
    pub source_id: String,
    pub ports: Vec<crate::resources::PortExpectation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Library {
    pub schema_version: u32,
    pub profiles: Vec<ProfileDocument>,
    pub local_assignments: BTreeMap<String, Vec<LocalAssignment>>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: LIBRARY_SCHEMA_VERSION,
            profiles: vec![default_document()],
            local_assignments: BTreeMap::new(),
        }
    }
}

impl Library {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != LIBRARY_SCHEMA_VERSION {
            return Err("Unsupported profile library schema version.".into());
        }
        if self.profiles.is_empty() || self.profiles.len() > MAX_PROFILES {
            return Err("A profile library must contain 1–32 profiles.".into());
        }
        let mut ids = BTreeSet::new();
        for document in &self.profiles {
            validate_document(document)?;
            if !ids.insert(document.id.as_str()) {
                return Err("Duplicate profile identity in the library.".into());
            }
        }
        if !ids.contains(DEFAULT_PROFILE_ID) {
            return Err("The profile library is missing its protected Default profile.".into());
        }
        if self
            .profiles
            .iter()
            .any(|document| document.id == DEFAULT_PROFILE_ID && document.name != "Default")
        {
            return Err("The built-in Default profile keeps its protected name.".into());
        }
        if self.local_assignments.len() > self.profiles.len() {
            return Err("Too many local assignment groups.".into());
        }
        for (id, assignments) in &self.local_assignments {
            let document = self
                .profiles
                .iter()
                .find(|p| &p.id == id)
                .ok_or("Local assignments reference a profile that does not exist.")?;
            validate_assignments(document, assignments)?;
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_LIBRARY_BYTES {
            return Err("The profile library exceeds 17 MiB; remove an unused profile or reduce its saved graph and local assignment data.".into());
        }
        Ok(bytes)
    }
}


/// Validate exact local pins without requiring a semantically valid saved graph.
pub fn validate_assignments(document: &ProfileDocument, assignments: &[LocalAssignment]) -> Result<(), String> {
    if assignments.len() > document.resources.len() {
        return Err("Too many local resource assignments.".into());
    }
    let mut resources = BTreeSet::new();
    for assignment in assignments {
        if !resources.insert(assignment.resource_id.as_str())
            || !document
                .resources
                .iter()
                .any(|r| r.id == assignment.resource_id)
        {
            return Err(
                "Local assignments must reference distinct profile resources.".into(),
            );
        }
        let resource = document
            .resources
            .iter()
            .find(|r| r.id == assignment.resource_id)
            .unwrap();
        if assignment.ports.len() != resource.ports.len() {
            return Err("Local assignment must pin every logical port exactly.".into());
        }
        let mut logical = BTreeSet::new();
        let mut observed = BTreeSet::new();
        for port in &assignment.ports {
            if !crate::validation::port_identifier(&port.profile_port_id)
                || !resource.ports.iter().any(|p| p.id == port.profile_port_id)
                || !logical.insert(&port.profile_port_id)
                || !observed.insert(&port.observed_port_id)
                || crate::resources_ports::text(&port.observed_port_id, 256).is_err()
                || !port
                    .expected_contract_hash
                    .strip_prefix("sha256:")
                    .is_some_and(|v| {
                        v.len() == 64
                            && v.bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
            {
                return Err("Local port assignments require unique exact observed IDs and contract hashes.".into());
            }
        }
        if assignment.source_id.is_empty()
            || assignment.source_id.len() > 1024
            || assignment.source_id.chars().any(char::is_control)
        {
            return Err("Invalid local source identity.".into());
        }
    }

    Ok(())
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn load(&self) -> Result<Option<Library>, String> {
        self.check_path(false)?;
        let path = self.root.join("library.json");
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot open profile library: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(MAX_LIBRARY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Cannot read profile library: {e}"))?;
        if bytes.len() > MAX_LIBRARY_BYTES {
            return Err("The profile library exceeds 17 MiB; remove an unused profile or reduce its saved graph and local assignment data.".into());
        }
        let library: Library = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Cannot parse profile library: {e}"))?;
        library.validate()?;
        Ok(Some(library))
    }

    pub fn save(&self, library: &Library) -> Result<(), String> {
        self.save_with(library, |temporary, target| {
            temporary
                .persist(target)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    fn save_with(
        &self,
        library: &Library,
        replace: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<(), String>,
    ) -> Result<(), String> {
        let bytes = library.encode()?;
        self.check_path(true)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(|e| format!("Cannot create temporary profile library: {e}"))?;
        temporary.write_all(&bytes).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let directory = std::fs::File::open(&self.root)
            .map_err(|e| format!("Cannot open profile directory before commit: {e}"))?;
        // Recheck the destination immediately before the atomic replacement.
        self.check_path(false)?;
        replace(temporary, &self.root.join("library.json"))?;
        // Atomic replacement is the commit boundary. Directory syncing is best
        // effort after that boundary: reporting an error here would incorrectly
        // leave the caller's memory behind a write that already committed.
        #[cfg(unix)]
        let _ = directory.sync_all();
        Ok(())
    }

    fn check_path(&self, create: bool) -> Result<(), String> {
        if !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(
                "Profile storage requires an absolute path without parent traversal.".into(),
            );
        }
        let mut checked = PathBuf::new();
        for component in self.root.components() {
            checked.push(component.as_os_str());
            match std::fs::symlink_metadata(&checked) {
                Ok(metadata) => {
                    reject_link(&metadata)?;
                    if !metadata.is_dir() {
                        return Err("A profile storage parent is not a directory.".into());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if create {
                        std::fs::create_dir(&checked).map_err(|e| e.to_string())?;
                        reject_link(
                            &std::fs::symlink_metadata(&checked).map_err(|e| e.to_string())?,
                        )?;
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        match std::fs::symlink_metadata(self.root.join("library.json")) {
            Ok(metadata) => {
                reject_link(&metadata)?;
                if !metadata.is_file() {
                    return Err("The profile library is not a regular file.".into());
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

fn reject_link(metadata: &std::fs::Metadata) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Profile storage does not follow symlinks or reparse points.".into());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err("Profile storage does not follow symlinks.".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

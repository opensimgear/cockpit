//! The profile library has one bounded, revisioned SQLite document. It
//! never scans legacy profile locations or mutates driver-owned configuration.
use crate::{default_document, validate_document, ProfileDocument, DEFAULT_PROFILE_ID};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub use crate::{MAX_LIBRARY_BYTES, MAX_PROFILES};

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
    pub profiles: Vec<ProfileDocument>,
    #[serde(default)]
    pub local_virtual_slots: BTreeMap<String, BTreeMap<String, u8>>,
    pub local_assignments: BTreeMap<String, Vec<LocalAssignment>>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            profiles: vec![default_document()],
            local_virtual_slots: BTreeMap::new(),
            local_assignments: BTreeMap::new(),
        }
    }
}

impl Library {
    pub fn validate(&self) -> Result<(), String> {
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
        for (id, slots) in &self.local_virtual_slots {
            let document = self.profiles.iter().find(|p| &p.id == id)
                .ok_or("Local virtual slots reference a missing profile.")?;
            crate::service::virtual_slots::validate(document, slots)?;
        }
        Ok(())
    }

    pub fn prune_virtual_slots(&mut self) {
        self.local_virtual_slots.retain(|id, slots| {
            let Some(document) = self.profiles.iter().find(|p| &p.id == id) else { return false; };
            slots.retain(|node, _| document.virtual_outputs.iter().any(|output| &output.id == node));
            !slots.is_empty()
        });
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
pub fn validate_assignments(
    document: &ProfileDocument,
    assignments: &[LocalAssignment],
) -> Result<(), String> {
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
            return Err("Local assignments must reference distinct profile resources.".into());
        }
        let resource = document
            .resources
            .iter()
            .find(|r| r.id == assignment.resource_id)
            .unwrap();
        if resource.kind == crate::ResourceKind::DebugController { return Err("Debug controllers do not use local assignments.".into()); }
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
                return Err(
                    "Local port assignments require unique exact observed IDs and contract hashes."
                        .into(),
                );
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
    repository: std::sync::OnceLock<osg_persistence::Repository<Library>>,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            repository: std::sync::OnceLock::new(),
        }
    }
    fn repository(&self) -> Result<&osg_persistence::Repository<Library>, String> {
        if self.repository.get().is_none() {
            let source = self.root.join("library.json");
            let database = osg_persistence::Database::for_namespace(&source)?;
            let repository = osg_persistence::Repository::new(
                database,
                "profiles",
                "library",
                MAX_LIBRARY_BYTES,
                Library::validate,
            )?;
            let _ = self.repository.set(repository);
        }
        Ok(self.repository.get().expect("repository initialized"))
    }
    pub fn load(&self) -> Result<Option<Library>, String> {
        self.repository()?.load()
    }
    pub fn save(&self, library: &Library) -> Result<(), String> {
        library.encode()?;
        self.repository()?.save(library)
    }
    pub fn reset(&self, library: &Library) -> Result<(), String> {
        library.encode()?;
        self.repository()?.reset(library)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

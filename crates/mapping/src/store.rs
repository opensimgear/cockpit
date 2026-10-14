//! Local profile persistence is independent of activation and backend enablement.
use crate::{
    engine::{validate_assignments, Assignment},
    profile::{Profile, DOCUMENT_LIMIT},
};
use serde::{Deserialize, Serialize};
const ASSIGNMENT_LIMIT: usize = 1_048_576;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalAssignments {
    schema: u32,
    profile_id: String,
    assignments: Vec<Assignment>,
}
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub const MAX_SAVED: usize = 128;
pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        let parsed = uuid::Uuid::parse_str(id).map_err(|_| "A profile UUID is required.")?;
        if parsed.to_string() != id {
            return Err("Profile UUID must use canonical lowercase form.".into());
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    pub fn load(&self, id: &str) -> Result<Profile, String> {
        let path = self.path(id)?;
        if std::fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Profile symlinks are not followed.".into());
        }
        let profile = read(&path)?;
        if profile.id != id {
            return Err("Stored profile identity does not match its filename.".into());
        }
        Ok(profile)
    }
    pub fn ids(&self) -> Result<Vec<String>, String> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.to_string()),
        };
        let mut ids = Vec::new();
        for (count, entry) in entries.enumerate() {
            if count >= 1024 {
                return Err("Profile directory exceeds its inspection limit.".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            if let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|id| self.path(id).is_ok())
            {
                ids.push(id.to_owned());
            }
            if ids.len() > MAX_SAVED {
                return Err("Profile storage exceeds 128 documents.".into());
            }
        }
        ids.sort();
        Ok(ids)
    }
    pub fn save(&self, profile: &Profile) -> Result<(), String> {
        let bytes = profile.export()?;
        let path = self.path(&profile.id)?;
        if !path.exists() && self.ids()?.len() >= MAX_SAVED {
            return Err("Delete an unused mapping before saving another.".into());
        }
        self.persist(&path, &bytes)
    }
    fn persist(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(&self.root).map_err(|e| e.to_string())?;
        temporary.write_all(&bytes).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        temporary.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn save_assignments(&self, id: &str, assignments: &[Assignment]) -> Result<(), String> {
        let profile = self.load(id)?;
        validate_assignments(&profile, assignments)?;
        let bytes = serde_json::to_vec(&LocalAssignments {
            schema: 1,
            profile_id: id.into(),
            assignments: assignments.to_vec(),
        })
        .map_err(|e| e.to_string())?;
        if bytes.len() > ASSIGNMENT_LIMIT {
            return Err("Local assignments exceed 1 MiB.".into());
        }
        self.persist(&self.path(id)?.with_extension("assignments.json"), &bytes)
    }
    pub fn load_assignments(&self, id: &str) -> Result<Vec<Assignment>, String> {
        let profile = self.load(id)?;
        let path = self.path(id)?.with_extension("assignments.json");
        if std::fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Assignment symlinks are not followed.".into());
        }
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(ASSIGNMENT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > ASSIGNMENT_LIMIT {
            return Err("Local assignments exceed 1 MiB.".into());
        }
        let local: LocalAssignments = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if local.schema != 1 || local.profile_id != id {
            return Err("Invalid local assignment schema or profile identity.".into());
        }
        validate_assignments(&profile, &local.assignments)?;
        Ok(local.assignments)
    }
    pub fn import(&self, bytes: &[u8]) -> Result<Profile, String> {
        let profile = Profile::parse(bytes)?;
        self.save(&profile)?;
        Ok(profile)
    }
    pub fn export(&self, id: &str) -> Result<Vec<u8>, String> {
        self.load(id)?.export()
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        std::fs::remove_file(self.path(id)?).map_err(|e| e.to_string())
    }
}
fn read(path: &Path) -> Result<Profile, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(DOCUMENT_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Profile::parse(&bytes)
}
#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

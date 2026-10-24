//! A single bounded write-ahead manifest. Existing adapter journals remain the
//! owners of their exact hardware/driver restoration contracts.
use super::OwnedRun;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    path::{Component, Path, PathBuf},
};
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECEIPTS: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Transition {
    #[serde(deserialize_with = "super::required_option")]
    pub incoming: Option<OwnedRun>,
    #[serde(deserialize_with = "super::required_option")]
    pub outgoing: Option<OwnedRun>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Manifest {
    #[serde(deserialize_with = "super::required_option")]
    pub active: Option<OwnedRun>,
    #[serde(deserialize_with = "super::required_option")]
    pub pending: Option<Transition>,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            active: None,
            pending: None,
        }
    }
}
fn uuid(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|id2| id2.to_string() == id)
}
fn hash(id: &str) -> bool {
    id.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
impl OwnedRun {
    fn validate(&self) -> Result<(), String> {
        if !uuid(&self.owner.session)
            || !uuid(&self.owner.profile_id)
            || self.owner.generation == 0
            || self.owner.profile_revision == 0
            || self.owner.profile_revision > osg_profiles::MAX_REVISION
            || self.snapshot.profile_id != self.owner.profile_id
            || self.snapshot.revision != self.owner.profile_revision
            || !hash(&self.snapshot.semantic_hash)
            || !hash(&self.program_hash)
            || !hash(&self.capture_fingerprint)
            || self.receipts.len() > MAX_RECEIPTS
            || self.owner.game_id.as_ref().is_some_and(|id| !osg_profiles::activation::valid_game_id(id))
            || self.attempted.len() > super::APPLY_PHASES.len()
            || self
                .attempted
                .iter()
                .zip(super::APPLY_PHASES)
                .any(|(a, b)| *a != b)
        {
            return Err("Invalid profile session recovery identity or phase.".into());
        }
        let mut ids = BTreeSet::new();
        for receipt in &self.receipts {
            if !text(&receipt.kind, 80)
                || !text(&receipt.resource_id, 256)
                || !receipt.state.is_object()
                || !ids.insert((&receipt.kind, &receipt.resource_id))
                || serde_json::to_vec(&receipt.state)
                    .map_err(|e| e.to_string())?
                    .len()
                    > 128 * 1024
            {
                return Err("Invalid or duplicate native profile recovery receipt.".into());
            }
        }
        Ok(())
    }
}
impl Manifest {
    fn validate(&self) -> Result<(), String> {
        if let Some(active) = &self.active {
            active.validate()?;
            if active.attempted != super::APPLY_PHASES {
                return Err("An active profile run is missing activation phases.".into());
            }
        }
        if let Some(pending) = &self.pending {
            if pending.incoming.is_none() && pending.outgoing.is_none() {
                return Err("Empty profile recovery intent.".into());
            }
            if let Some(run) = &pending.incoming {
                run.validate()?;
            }
            if let Some(run) = &pending.outgoing {
                run.validate()?;
            }
            if pending.outgoing.as_ref().map(|run| &run.owner)
                != self.active.as_ref().map(|run| &run.owner)
            {
                return Err("Profile transition does not match its previous owner.".into());
            }
        }
        Ok(())
    }
}

pub(super) struct Store {
    path: Option<PathBuf>,
    lock: Option<File>,
    evidence: Option<osg_persistence::Evidence>,
    error: Option<String>,
    #[cfg(test)]
    pub(super) fail_save: bool,
    #[cfg(test)]
    pub(super) fail_save_phase: Option<super::ApplyPhase>,
}
impl Drop for Store {
    fn drop(&mut self) {
        if let Some(lock) = &self.lock {
            let _ = lock.unlock();
        }
    }
}
impl Store {
    pub(super) fn open(path: Option<PathBuf>) -> (Self, Manifest, Option<String>) {
        let mut store = Self {
            path,
            lock: None,
            evidence: None,
            error: None,
            #[cfg(test)]
            fail_save: false,
            #[cfg(test)]
            fail_save_phase: None,
        };
        let loaded = (|| {
            let Some(path) = &store.path else {
                return Ok(Manifest::default());
            };
            safe(path)?;
            let parent = path
                .parent()
                .ok_or("Profile journal needs an explicit storage directory.")?;
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            safe(path)?;
            let lock_path = path.with_extension("lock");
            safe(&lock_path)?;
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(lock_path)
                .map_err(|e| e.to_string())?;
            lock.try_lock()
                .map_err(|_| "Another Cockpit process owns the profile session journal.")?;
            store.lock = Some(lock);
            let evidence=osg_persistence::Evidence::open(path,"recovery-profile",MAX_BYTES)?;
            let manifest: Manifest = evidence
                .payload()
                .map(|data| {
                    serde_json::from_slice(data)
                        .map_err(|e| format!("Invalid profile recovery journal: {e}"))
                })
                .transpose()?
                .unwrap_or_default();
            manifest.validate()?;
            store.evidence=Some(evidence);
            Ok::<_, String>(manifest)
        })();
        match loaded {
            Ok(manifest) => (store, manifest, None),
            Err(error) => {
                store.error = Some(error.clone());
                (store, Manifest::default(), Some(error))
            }
        }
    }
    pub(super) fn save(&mut self, value: &Manifest) -> Result<(), String> {
        self.verify()?;
        value.validate()?;
        let data = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if data.len() > MAX_BYTES {
            return Err("Profile recovery journal exceeds 2 MiB.".into());
        }
        #[cfg(test)]
        if self.fail_save {
            return Err("Injected profile journal failure.".into());
        }
        #[cfg(test)]
        if self.fail_save_phase.is_some()
            && value
                .pending
                .as_ref()
                .and_then(|pending| pending.incoming.as_ref())
                .and_then(|run| run.attempted.last())
                .copied()
                == self.fail_save_phase
        {
            self.fail_save_phase = None;
            return Err("Injected profile journal failure before phase admission.".into());
        }
        // Repeated shutdown must not mint a new evidence revision after a
        // stopped runtime has queued a restore. Verification above still rejects
        // external edits, including equal bytes at a different revision.
        if let Some(evidence)=&mut self.evidence {
            if evidence.payload()!=Some(data.as_slice()){evidence.write(&data)?;}
        }
        Ok(())
    }
    pub(super) fn verify(&self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(format!("Profile recovery storage is read-only: {error}"));
        }
        if let Some(evidence)=&self.evidence {evidence.verify()?;}
        Ok(())
    }
}
fn safe(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("Profile journal requires an absolute, non-traversing path.".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                let link = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let link = metadata.file_type().is_symlink();
                if link
                    || (ancestor == path && !metadata.is_file())
                    || (ancestor != path && !metadata.is_dir())
                {
                    return Err(
                        "Profile recovery paths must not follow links or special files.".into(),
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path="profile_session_journal_tests.rs"]
mod tests;

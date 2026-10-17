//! A single bounded write-ahead manifest. Existing adapter journals remain the
//! owners of their exact hardware/driver restoration contracts.
use super::OwnedRun;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read,
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
    pub schema_version: u32,
    pub paused: bool,
    #[serde(deserialize_with = "super::required_option")]
    pub active: Option<OwnedRun>,
    #[serde(deserialize_with = "super::required_option")]
    pub pending: Option<Transition>,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            paused: false,
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
            || self.owner.game_id.as_ref().is_some_and(|id| {
                !matches!(
                    id.as_str(),
                    "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally"
                )
            })
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
        if self.schema_version != 1 {
            return Err("Unsupported profile session journal version.".into());
        }
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
    expected: Option<Vec<u8>>,
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
            expected: None,
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
            store.expected = read(path)?;
            let manifest: Manifest = store
                .expected
                .as_ref()
                .map(|data| {
                    serde_json::from_slice(data)
                        .map_err(|e| format!("Invalid profile recovery journal: {e}"))
                })
                .transpose()?
                .unwrap_or_default();
            manifest.validate()?;
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
        if let Some(path) = &self.path {
            crate::dcs_setup::replace(path, &data)?;
            // Replacement is the commit boundary. A post-commit directory sync
            // cannot be reported as if the old manifest still owned the disk.
            self.expected = Some(data);
            #[cfg(unix)]
            if let Some(parent) = path.parent() {
                let _ = File::open(parent).and_then(|f| f.sync_all());
            }
        }
        Ok(())
    }
    pub(super) fn verify(&self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(format!("Profile recovery storage is read-only: {error}"));
        }
        if let Some(path) = &self.path {
            if read(path)? != self.expected {
                return Err("Profile recovery journal changed externally; preserving it.".into());
            }
        }
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
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    safe(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut data = vec![];
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > MAX_BYTES {
        return Err("Profile recovery journal exceeds 2 MiB.".into());
    }
    Ok(Some(data))
}

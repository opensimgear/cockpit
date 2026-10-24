//! The mutable backup catalogue and restart intent live in SQLite. Completed
//! bundles are immutable user exports; no loose file is a settings authority.
use osg_persistence::{Change, Database, NamespaceLease};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
mod model;
use model::*;
mod assets;
mod startup;
mod trust;
pub(crate) use startup::{bind_restart, initialize, release_namespace};
#[cfg(test)]
mod tests;
const INDEX_LIMIT: usize = 64 * 1024;
const MANIFEST_LIMIT: u64 = 512 * 1024;
pub(crate) struct Runtime {
    database: Result<Database, String>,
    root: PathBuf,
    controls: Mutex<()>,
}
impl Runtime {
    #[cfg(test)]
    pub(crate) fn fixture(database: Database, root: PathBuf) -> Self {
        Self {
            database: Ok(database),
            root,
            controls: Mutex::new(()),
        }
    }
    pub fn new(native: bool) -> Self {
        let anchor = crate::config_paths::config_path();
        Self {
            database: if native {
                Database::for_namespace(&anchor)
            } else {
                Err("Backups require the native Cockpit.".into())
            },
            root: anchor.parent().unwrap_or(Path::new("")).to_owned(),
            controls: Mutex::new(()),
        }
    }
    fn database(&self) -> Result<&Database, String> {
        self.database.as_ref().map_err(Clone::clone)
    }
    fn index(&self) -> Result<(u64, Index), String> {
        let (revision, record) = self.database()?.read_with_revision("storage", "backups")?;
        let index = if let Some(record) = record {
            if record.payload.len() > INDEX_LIMIT {
                return Err("Backup inventory exceeds its byte budget.".into());
            }
            serde_json::from_slice(&record.payload)
                .map_err(|_| "Invalid backup inventory. Preserve the database.")?
        } else {
            Index::default()
        };
        index.validate()?;
        Ok((revision, index))
    }
    fn pending(&self) -> Result<Option<Pending>, String> {
        let value = self
            .database()?
            .read("storage", "pending-restore")?
            .map(|r| -> Result<Pending, String> {
                if r.payload.len() > 4096 {
                    return Err("Invalid pending restore.".into());
                }
                let value: Pending =
                    serde_json::from_slice(&r.payload).map_err(|_| "Invalid pending restore.")?;
                value.validate()?;
                Ok(value)
            })
            .transpose()?;
        Ok(value)
    }
    fn directory(&self, id: &str) -> Result<PathBuf, String> {
        if !model::id(id) {
            return Err("Choose a saved backup identity.".into());
        }
        let directory = self.root.join("backups").join(id);
        osg_persistence::check_path(&directory.join("manifest.json"), false)?;
        Ok(directory)
    }
    pub fn restart(&self) -> Result<Value, String> {
        if self.pending()?.is_none() {
            return Err("Queue a restore before restarting through storage controls.".into());
        }
        startup::restart()
    }
    pub fn status(&self) -> Result<Value, String> {
        let (revision, index) = self.index()?;
        Ok(
            json!({"revision":revision,"epoch":self.database()?.epoch(),"backups":index.entries,"pendingRestore":self.pending()?.map(|p|p.id),"maximumBackups":MAX_BACKUPS-1,"restartRequired":self.pending()?.is_some()}),
        )
    }
    fn exact(&self, expected: u64) -> Result<Index, String> {
        let (revision, index) = self.index()?;
        if revision != expected {
            return Err("Backup inventory revision conflict. Reload before continuing.".into());
        }
        Ok(index)
    }
    fn save(&self, expected: u64, index: &Index) -> Result<(), String> {
        index.validate()?;
        let bytes = serde_json::to_vec(index).map_err(|_| "Cannot encode backup inventory.")?;
        self.database()?.commit(&[Change {
            domain: "storage",
            key: "backups",
            expected_revision: expected,
            payload: Some(&bytes),
        }])?;
        Ok(())
    }
    fn create_locked(&self, expected: u64) -> Result<Entry, String> {
        let mut index = self.exact(expected)?;
        if index.entries.len() >= MAX_BACKUPS {
            return Err("Backup storage is full. Remove a backup before creating another.".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let directory = self.directory(&id)?;
        osg_persistence::check_path(&directory.join("snapshot.sqlite3"), true)?;
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "The system clock is unavailable.")?
            .as_millis() as u64;
        // The artwork owner's process lock spans snapshot and referenced-file capture.
        let artwork_lock_path = self.root.join("game-artwork.lock");
        osg_persistence::check_path(&artwork_lock_path, true)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(artwork_lock_path)
            .map_err(|_| "Cannot lock managed artwork for backup.")?;
        lock.try_lock()
            .map_err(|_| "Managed artwork is busy. Retry the backup.")?;
        let snapshot = directory.join("snapshot.sqlite3");
        self.database()?.backup(&snapshot)?;
        let captured = Database::open_existing(&snapshot)?;
        captured.integrity_check()?;
        let assets = assets::collect(&captured, &self.root, &directory)?;
        let manifest = Manifest {
            id: id.clone(),
            created_at,
            database_hash: osg_persistence::snapshot_hash(&snapshot)?,
            assets,
        };
        manifest.validate()?;
        let bytes = serde_json::to_vec(&manifest).map_err(|_| "Cannot encode backup manifest.")?;
        if bytes.len() as u64 > MANIFEST_LIMIT {
            return Err("Backup manifest exceeds its byte budget.".into());
        }
        assets::write_new(&directory.join("manifest.json"), &bytes)?;
        let entry = Entry {
            id,
            created_at,
            manifest_hash: assets::digest(&bytes),
        };
        index.entries.insert(0, entry.clone());
        self.save(expected, &index)?;
        Ok(entry)
    }
    pub fn create(&self, expected: u64) -> Result<Value, String> {
        let _lock = self
            .controls
            .lock()
            .map_err(|_| "Backup controls are unavailable.")?;
        if self.pending()?.is_some() {
            return Err("Restart Cockpit to finish the queued restore first.".into());
        }
        if self.index()?.1.entries.len() >= MAX_BACKUPS - 1 {
            return Err("Remove a backup before creating another; one slot is reserved for restore rollback.".into());
        }
        let entry = self.create_locked(expected)?;
        Ok(json!({"backup":entry,"status":self.status()?}))
    }
    fn verified(&self, id: &str, expected: u64) -> Result<(PathBuf, Manifest), String> {
        let index = self.exact(expected)?;
        let entry = index
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or("The saved backup no longer exists.")?;
        let directory = self.directory(id)?;
        let bytes = assets::read(&directory.join("manifest.json"), MANIFEST_LIMIT)?;
        if assets::digest(&bytes) != entry.manifest_hash {
            return Err("The backup manifest changed. Existing data was preserved.".into());
        }
        let manifest: Manifest =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid backup manifest.")?;
        manifest.validate()?;
        if manifest.id != id
            || manifest.created_at != entry.created_at
            || osg_persistence::snapshot_hash(&directory.join("snapshot.sqlite3"))?
                != manifest.database_hash
        {
            return Err(
                "The backup database failed its hash check. Existing data was preserved.".into(),
            );
        }
        Database::open_existing(directory.join("snapshot.sqlite3"))?.integrity_check()?;
        assets::verify(&directory, &manifest)?;
        Ok((directory, manifest))
    }
    pub fn verify(&self, id: &str, expected: u64) -> Result<(), String> {
        let _lock = self
            .controls
            .lock()
            .map_err(|_| "Backup controls are unavailable.")?;
        self.verified(id, expected)?;
        if self.index()?.1.entries.len() >= MAX_BACKUPS {
            return Err("Remove a backup to reserve space for the rollback snapshot.".into());
        }
        if self.pending()?.is_some() {
            return Err("A restore is already queued. Restart Cockpit.".into());
        }
        Ok(())
    }
    pub fn queue(&self, id: &str, expected: u64) -> Result<Value, String> {
        let _lock = self
            .controls
            .lock()
            .map_err(|_| "Backup controls are unavailable.")?;
        self.verified(id, expected)?;
        if self.pending()?.is_some() {
            return Err("A restore is already queued. Restart Cockpit.".into());
        }
        let database = self.database()?;
        let pending = Pending {
            id: id.into(),
            epoch: database.epoch().into(),
            rollback: None,
            recovery_fingerprint: database.recovery_fingerprint()?,
        };
        let bytes = serde_json::to_vec(&pending).map_err(|_| "Cannot encode restore intent.")?;
        database.commit_checked(
            &[Change {
                domain: "storage",
                key: "pending-restore",
                expected_revision: database.revision("storage", "pending-restore")?,
                payload: Some(&bytes),
            }],
            &[osg_persistence::RevisionCheck {
                domain: "storage",
                key: "backups",
                expected_revision: expected,
            }],
        )?;
        Ok(
            json!({"restartRequired":true,"backupId":id,"message":"Runtime stopped. Restart Cockpit to restore this backup. Credentials must be reissued and companions paired again."}),
        )
    }
    pub fn remove(&self, id: &str, expected: u64) -> Result<Value, String> {
        let _lock = self
            .controls
            .lock()
            .map_err(|_| "Backup controls are unavailable.")?;
        if self.pending()?.is_some() {
            return Err("Restart Cockpit to finish the queued restore first.".into());
        }
        let mut index = self.exact(expected)?;
        if !index.entries.iter().any(|entry| entry.id == id) {
            return Err("The saved backup no longer exists.".into());
        }
        index.entries.retain(|entry| entry.id != id);
        self.save(expected, &index)?;
        // Retire authority before physical cleanup. If cleanup fails, the unreferenced
        // bundle remains inert for inspection. No recursive delete follows a link.
        let directory = self.directory(id)?;
        let removed = if directory.exists() {
            remove_bundle(&directory).is_ok()
        } else {
            true
        };
        Ok(json!({"status":self.status()?,"filesRemoved":removed}))
    }
}
fn remove_bundle(directory: &Path) -> Result<(), String> {
    osg_persistence::check_path(&directory.join("manifest.json"), false)?;
    for entry in fs::read_dir(directory).map_err(|_| "Cannot inspect retired backup.")? {
        let path = entry.map_err(|_| "Cannot inspect retired backup.")?.path();
        if path.file_name().is_some_and(|name| name == "assets") {
            osg_persistence::check_path(&path.join(".check"), false)?;
            for entry in fs::read_dir(&path).map_err(|_| "Cannot inspect retired assets.")? {
                let file = entry.map_err(|_| "Cannot inspect retired assets.")?.path();
                osg_persistence::check_path(&file, false)?;
                fs::remove_file(file).map_err(|_| "Cannot remove retired asset.")?;
            }
            fs::remove_dir(path).map_err(|_| "Cannot remove retired assets.")?;
        } else {
            osg_persistence::check_path(&path, false)?;
            fs::remove_file(path).map_err(|_| "Cannot remove retired backup file.")?;
        }
    }
    fs::remove_dir(directory).map_err(|_| "Cannot remove retired backup directory.".into())
}

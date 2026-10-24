//! Runs with an OS namespace lease before constructing any native worker.
use super::*;
use std::sync::OnceLock;
static LEASE: OnceLock<Mutex<Option<NamespaceLease>>> = OnceLock::new();
pub(crate) fn release_namespace() {
    if let Some(held) = LEASE.get() {
        if let Ok(mut held) = held.lock() {
            *held = None;
        }
    }
}
pub(crate) fn initialize() -> Result<(), String> {
    let mut held = LEASE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "Namespace ownership is unavailable.")?;
    if held.is_some() {
        return Ok(());
    }
    let path = crate::config_paths::database_path();
    let anchor = crate::config_paths::config_path();
    let root = anchor
        .parent()
        .ok_or("Invalid native configuration namespace.")?;
    let lease = boot(&path, root)?;
    *held = Some(lease);
    Ok(())
}
fn boot(path: &Path, root: &Path) -> Result<NamespaceLease, String> {
    let lease = NamespaceLease::acquire(path)?;
    let database = Database::open(path)?;
    let runtime = Runtime {
        database: Ok(database),
        root: root.to_owned(),
        controls: Mutex::new(()),
    };
    runtime.finish_pending(&lease)?;
    Ok(lease)
}

impl Runtime {
    pub(super) fn finish_pending(
        &self,
        lease: &NamespaceLease,
    ) -> Result<Option<Database>, String> {
        let Some(mut pending) = self.pending()? else {
            return Ok(None);
        };
        if pending.epoch != self.database()?.epoch() {
            return Err("The queued restore belongs to another database incarnation. Preserve the database for inspection.".into());
        }
        if pending.recovery_fingerprint != self.database()?.recovery_fingerprint()? {
            return Err("Recovery evidence changed after quiescence. Current data was preserved; inspect recovery before restoring.".into());
        }
        let (revision, _) = self.index()?;
        let (directory, manifest) = self.verified(&pending.id, revision)?;
        if pending.rollback.is_none() {
            let entry = self.create_locked(revision)?;
            pending.rollback = Some(entry.id);
            let bytes =
                serde_json::to_vec(&pending).map_err(|_| "Cannot encode rollback receipt.")?;
            let database = self.database()?;
            database.commit(&[Change {
                domain: "storage",
                key: "pending-restore",
                expected_revision: database.revision("storage", "pending-restore")?,
                payload: Some(&bytes),
            }])?;
        }
        // Install only immutable artwork. Historical DCS backups remain in the bundle
        // for inspection and never regain cleanup authority in the restored runtime.
        assets::restore(&self.root, &directory, &manifest)?;
        let snapshot = Database::open_existing(directory.join("snapshot.sqlite3"))?;
        let host = trust::fresh_host(&snapshot, osg_companion_service::persistence::seal)?;
        let database = self.database()?.restore_with_fresh_host(
            &directory.join("snapshot.sqlite3"),
            &manifest.database_hash,
            lease,
            host.as_deref(),
        )?;
        Ok(Some(database))
    }
}

static RESTART: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
pub(crate) fn bind_restart(callback: Box<dyn Fn() + Send + Sync>) {
    let _ = RESTART.set(callback);
}
pub(super) fn restart() -> Result<Value, String> {
    let callback=RESTART.get().ok_or("Restart is available in the native desktop Cockpit. Close and reopen this process to complete restore.")?;
    std::thread::Builder::new()
        .name("restore-restart".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            callback();
        })
        .map_err(|_| "Cannot schedule the native restart.")?;
    Ok(json!({"restartScheduled":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopped_boot_requires_exclusive_namespace_and_never_seeds_credentials() {
        let directory = crate::test_directory();
        let path = directory.path().join("cockpit.sqlite3");
        let lease = boot(&path, directory.path()).unwrap();
        assert!(boot(&path, directory.path()).is_err());
        let database = Database::open_existing(&path).unwrap();
        assert!(database
            .read("secrets", "companion-host")
            .unwrap()
            .is_none());
        drop(lease);
        boot(&path, directory.path()).unwrap();
        assert!(restart().is_err());
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;
    #[test]
    fn isolated_boot_pins_namespace_once_and_restart_callback_is_delivered() {
        const KEY: &str = "OSG_SQLITE_BOOT_FIXTURE";
        if std::env::var_os(KEY).is_some() {
            initialize().unwrap();
            initialize().unwrap();
            release_namespace();
            let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let observed = called.clone();
            bind_restart(Box::new(move || {
                observed.store(true, std::sync::atomic::Ordering::Release);
            }));
            restart().unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while !called.load(std::sync::atomic::Ordering::Acquire)
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(called.load(std::sync::atomic::Ordering::Acquire));
            return;
        }
        let directory = crate::test_directory();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact","persistence::backup::startup::process_tests::isolated_boot_pins_namespace_once_and_restart_callback_is_delivered"]).env(KEY,"1").env("OSG_DCS_BIOS_CONFIG",directory.path().join("dcs-bios.json")).env_remove("OSG_SQLITE_DATABASE").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        assert!(command.status().unwrap().success());
        assert!(directory.path().join("cockpit.sqlite3").exists());
        NamespaceLease::acquire(&directory.path().join("cockpit.sqlite3")).unwrap();
    }
}

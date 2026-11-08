//! Asynchronous game-side provisioning, separate from captured binding profiles.
use crate::game_discovery::Platform;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError, Weak,
    },
    thread::{self, JoinHandle},
};

const MAX_SETTINGS: usize = 32768;
struct Inner {
    path: PathBuf,
    work: PathBuf,
    native: bool,
    settings_error: Option<String>,
    store: Result<osg_persistence::Repository<Value>, String>,
    saved: Mutex<Value>,
    settings_writer: Mutex<()>,
    states: Mutex<BTreeMap<String, Arc<Value>>>,
    catalogues: Mutex<provider_settings::CatalogueCache>,
    catalogue_generation: [std::sync::atomic::AtomicU64; 3],
    dcs_catalogue_watch: Mutex<Option<DcsCatalogueWatch>>,
    watched_setup: Mutex<std::collections::BTreeSet<String>>,
    watched_setup_waiting: AtomicBool,
    views: Mutex<setup_cache::SetupViews>,
    fingerprints: crate::acr_setup::FingerprintCache,
    ac_cars: crate::ac_car_catalogue_cache::Cache,
    ac_cars_publication: Mutex<Option<Arc<car_publication::Publication>>>,
    #[cfg(test)]
    ac_cars_publication_pause:
        Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    #[cfg(test)]
    native_probe_pause: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    #[cfg(test)]
    inspection_pause: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    #[cfg(test)]
    settings_commit_pause:
        Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    #[cfg(test)]
    catalogue_stamp_pause:
        Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    cancel: AtomicBool,
    assetto_setup_wake: Mutex<Option<Arc<osg_streams::NativeWake>>>,
    busy: AtomicBool,
    closed: AtomicBool,
    recovery_needed: AtomicBool,
    changes: OnceLock<Arc<crate::game_read_models::signals::Signal>>,
    job: Mutex<Option<(String, String)>>,
}
pub(crate) struct GameIntegrationSetup {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
    admission: RwLock<()>,
}

#[derive(Clone)]
pub(crate) struct DcsCatalogueWatch {
    pub(crate) path: PathBuf,
    pub(crate) changes: Weak<AtomicU64>,
    pub(crate) observed: u64,
}
enum AdmissionGuard<'a> {
    Background { _lock: RwLockReadGuard<'a, ()> },
    Exclusive { _lock: RwLockWriteGuard<'a, ()> },
}
struct SetupAdmission<'a> {
    lock: Option<AdmissionGuard<'a>>,
    inner: &'a Inner,
}
impl Drop for SetupAdmission<'_> {
    fn drop(&mut self) {
        drop(self.lock.take());
        if self.inner.watched_setup_waiting.swap(false, Ordering::AcqRel) {
            if let Some(changes) = self.inner.changes.get() {
                changes.context_update_wake.notify();
                changes.setup_watch_wakes.notify_all();
            }
        }
    }
}
mod car_publication;
mod settings;
mod setup_cache;
pub(crate) use settings::dcs_connection;
use settings::*;
#[path = "../../activity/integration.rs"]
mod activity_capture;
mod inspection;
mod provider_settings;
pub(crate) use provider_settings::sdk_source_path;
mod provision;
mod requests;
/// Resolve a title's explicit binding before entering the adapter's dialect.
fn bound_game(game: &Value) -> Result<Value, String> {
    let key = crate::game_discovery::integration_binding(game)
        .ok_or("Attach a compatible integration to this game first.")?;
    let mut bound = game.clone();
    bound["id"] = json!(key);
    Ok(bound)
}
impl GameIntegrationSetup {
    pub(crate) fn new(path: PathBuf, native: bool) -> Self {
        let store = settings_repository(&path);
        let loaded = store.as_ref().map_err(Clone::clone).and_then(|store| {
            store
                .load()
                .map(|value| value.unwrap_or(json!({"schemaVersion":1,"games":{}})))
        });
        let states = BTreeMap::new();
        Self {
            inner: Arc::new(Inner {
                work: path.with_extension("setup-work"),
                recovery_needed: AtomicBool::new(crate::dcs_setup::recovery_pending(
                    &path.with_extension("setup-work"),
                )),
                ac_cars: crate::ac_car_catalogue_cache::Cache::new(
                    path.with_file_name("assetto-corsa-cars-v1.json"),
                ),
                ac_cars_publication: Mutex::new(None),
                #[cfg(test)]
                ac_cars_publication_pause: Mutex::new(None),
                #[cfg(test)]
                native_probe_pause: Mutex::new(None),
                #[cfg(test)]
                inspection_pause: Mutex::new(None),
                #[cfg(test)]
                settings_commit_pause: Mutex::new(None),
                #[cfg(test)]
                catalogue_stamp_pause: Mutex::new(None),
                path,
                native,
                settings_error: loaded.as_ref().err().cloned(),
                store,
                saved: Mutex::new(loaded.unwrap_or(json!({"schemaVersion":1,"games":{}}))),
                settings_writer: Mutex::new(()),
                states: Mutex::new(states),
                views: Mutex::new(Default::default()),
                catalogues: Mutex::new(BTreeMap::new()),
                catalogue_generation: Default::default(),
                dcs_catalogue_watch: Mutex::default(),
                watched_setup: Default::default(),
                watched_setup_waiting: AtomicBool::new(false),
                fingerprints: Default::default(),
                cancel: AtomicBool::new(false),
                assetto_setup_wake: Mutex::new(None),
                busy: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                changes: OnceLock::new(),
                job: Mutex::new(None),
            }),
            worker: Mutex::new(None),
            admission: RwLock::new(()),
        }
    }
    pub(crate) fn watch_changes(&self, changes: Arc<crate::game_read_models::signals::Signal>) {
        let _ = self.inner.changes.set(changes);
    }
    fn lock_admission(&self) -> SetupAdmission<'_> {
        let lock = self.admission.write().unwrap_or_else(|p| p.into_inner());
        SetupAdmission { lock: Some(AdmissionGuard::Exclusive { _lock: lock }), inner: &self.inner }
    }

    #[cfg(test)]
    pub(super) fn with_test_admission(&self, observe: impl FnOnce()) {
        let _admission = self.lock_admission();
        observe();
    }

    fn try_watched_admission(&self) -> Option<SetupAdmission<'_>> {
        let lock = match self.admission.try_read() {
            Ok(lock) => lock,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        Some(SetupAdmission { lock: Some(AdmissionGuard::Background { _lock: lock }), inner: &self.inner })
    }
    /// New preparation forces a content hash; subsequent native admission checks
    /// metadata immediately, independently of the passive observer's interval.
    pub(crate) fn validate_native_build(
        &self,
        game: &Value,
        force_hash: bool,
    ) -> Result<(), String> {
        crate::game_discovery::integration_key(game)
            .ok_or("Choose a compatible game executable before preparing its integration.")?;
        let game = bound_game(game)?;
        let id = game["id"].as_str().unwrap_or("");
        if !id.starts_with("assetto-corsa") {
            return Ok(());
        }
        let dialect = crate::game_telemetry::Game::from_id(id)?;
        let executable = game["executable"]
            .as_str()
            .ok_or("Choose the Assetto executable.")?;
        let identity = self
            .inner
            .fingerprints
            .inspect(Path::new(executable), dialect, force_hash)?;
        let saved = self.inner.saved.lock().unwrap_or_else(|p| p.into_inner());
        let record = &saved["games"][id];
        if record["configuredFingerprint"] != identity
            || record["verifiedFingerprint"] != identity
            || record["verificationIssue"].is_string()
        {
            return Err("The Assetto executable or native verification changed. Verify this game build before preparing its integration.".into());
        }
        Ok(())
    }
    pub(crate) fn busy(&self) -> bool {
        self.inner.busy.load(Ordering::Acquire)
    }
    pub(crate) fn decorate_installed_cars(&self, game: &mut Value) {
        self.inner.ac_cars.snapshot().decorate(game);
    }
    pub(crate) fn pending(&self) -> bool {
        crate::dcs_setup::recovery_pending(&self.inner.work)
    }
    pub(crate) fn decorate(&self, catalogue: &mut [Value]) {
        for game in catalogue {
            let Some(id) = crate::game_discovery::integration_binding(game) else {
                if let Some(object) = game.as_object_mut() {
                    object.remove("integrationSetup");
                }
                continue;
            };
            let status = self.inner.cached_setup(game, id);
            game["integrationSetup"] = status;
        }
    }
    pub(crate) fn decorate_summary(&self, catalogue: &mut [Value]) {
        for game in catalogue {
            if let Some(id) = crate::game_discovery::integration_binding(game) {
                game["integrationSetup"] = self.inner.cached_setup_summary(game, id);
            } else if let Some(object) = game.as_object_mut() { object.remove("integrationSetup"); }
        }
    }
    /// Source configuration proofs are independent of display eviction/budgets.
    /// Their writers are the existing refresh/command/worker paths.
    pub(crate) fn decorate_source(&self, catalogue: &mut [Value]) {
        for game in catalogue {
            let Some(id) = crate::game_discovery::integration_binding(game) else {
                if let Some(object) = game.as_object_mut() {
                    object.remove("integrationSetup");
                }
                continue;
            };
            let status = self.inner.cached_source_setup(game, id);
            game["integrationSetup"] = status;
        }
    }
    pub(crate) fn refresh(&self, catalogue: &[Value]) {
        let _admission = self.lock_admission();
        self.inner.retain_setup_scopes(catalogue);
        self.refresh_admitted(catalogue);
    }

    pub(crate) fn retain_background_scopes(&self, catalogue: &[Value]) {
        self.inner.retain_setup_scopes(catalogue);
    }

    pub(crate) fn refresh_background_scope(&self, catalogue: &[Value], current: impl Fn() -> bool) -> bool {
        let Some(_admission) = self.try_background_admission() else { return false; };
        self.refresh_scoped(catalogue, current)
    }

    pub(crate) fn invalidate_xplane_catalogue(&self, id: &str) {
        if let Some(generation) = self.inner.catalogue_generation(id) {
            let mut cache = self.inner.catalogues.lock().unwrap_or_else(|error| error.into_inner());
            generation.fetch_add(1, Ordering::AcqRel);
            cache.remove(id);
        }
    }

    pub(crate) fn catalogue_generation(&self, id: &str) -> Option<u64> {
        self.inner.catalogue_generation(id).map(|generation| generation.load(Ordering::Acquire))
    }
    pub(crate) fn dcs_catalogue_watch_path(&self) -> Option<PathBuf> {
        let (record, _) = self.inner.provider_snapshot("dcs");
        record["connection"]["cataloguePath"].as_str().map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }

    pub(crate) fn update_dcs_catalogue_watch(&self, watch: Option<DcsCatalogueWatch>) {
        let mut cache = self.inner.catalogues.lock().unwrap_or_else(|error| error.into_inner());
        *self.inner.dcs_catalogue_watch.lock().unwrap_or_else(|error| error.into_inner()) = watch;
        self.invalidate_dcs_catalogue(&mut cache);
    }

    fn invalidate_dcs_catalogue(&self, cache: &mut provider_settings::CatalogueCache) {
        self.inner.catalogue_generation[2].fetch_add(1, Ordering::AcqRel);
        cache.retain(|key, _| !key.starts_with("dcs:"));
    }
    pub(crate) fn catalogue_watched(&self, id: &str) -> bool {
        self.inner.watched_setup.lock().unwrap_or_else(|p| p.into_inner()).contains(id)
    }

    pub(crate) fn dcs_setup_watch_scope(&self, game: &Value) -> Option<(PathBuf, Option<PathBuf>)> {
        let install = game["installPath"].as_str().map(Path::new).filter(|path| path.is_absolute());
        let (record, _) = self.inner.provider_snapshot("dcs");
        let prepared = self.inner.dcs_prepared_setup(game, &record);
        record["userFolder"].as_str().or_else(|| prepared.as_ref().and_then(|status| status["userFolder"].as_str()))
            .map(Path::new).filter(|path| path.is_absolute())
            .map(|target| (target.to_path_buf(), install.map(Path::to_path_buf)))
    }

    pub(crate) fn set_setup_watch(&self, id: &str, active: bool) {
        let mut watched = self.inner.watched_setup.lock().unwrap_or_else(|error| error.into_inner());
        if active { watched.insert(id.into()); } else { watched.remove(id); }
    }

    fn try_background_admission(&self) -> Option<SetupAdmission<'_>> {
        if self.busy() { return None; }
        let admission = self.try_watched_admission().or_else(|| {
            // Retry after recording demand so a release between the first
            // attempt and registration cannot strand the pending file change.
            self.inner.watched_setup_waiting.store(true, Ordering::Release);
            self.try_watched_admission()
        })?;
        (!self.busy()).then_some(admission)
    }

    pub(crate) fn refresh_watched_setup(&self, id: &str, game: &Value, current: impl Fn() -> bool) -> bool {
        let Some(_admission) = self.try_background_admission() else { return false; };
        if !current() { return false; }
        if id == "dcs" {
            let mut cache = self.inner.catalogues.lock().unwrap_or_else(|error| error.into_inner());
            self.invalidate_dcs_catalogue(&mut cache);
        } else if let Ok(game) = crate::game_telemetry::Game::from_id(id) {
            self.inner.fingerprints.invalidate(game);
        }
        self.refresh_scoped(std::slice::from_ref(game), current)
    }

    fn refresh_admitted(&self, catalogue: &[Value]) {
        self.refresh_scoped(catalogue, || true);
    }

    // Each background worker owns one integration. Admission excludes setup
    // edits and provisioning while inspection and publication are in flight.
    fn refresh_scoped(&self, catalogue: &[Value], current: impl Fn() -> bool) -> bool {
        if self.busy() || !current() { return false; }
        for game in catalogue {
            let Ok(game) = bound_game(game) else { continue; };
            let (record, status) = self.inner.inspect_scoped(&game);
            if !current() { return false; }
            self.inner.publish_setup(&game, &record, status);
        }
        true
    }
    pub(crate) fn shutdown(&self) {
        let _admission = self.lock_admission();
        self.inner.closed.store(true, Ordering::Release);
        // Closing always stops further work; an already reserved durable commit
        // finishes before the existing worker join returns.
        let _ = self.inner.cancel();
        self.inner.wake_cancelled_setup();
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests;
#[cfg(test)]
fn fixture_write_settings(path: &Path, value: &Value) {
    let database = osg_persistence::Database::for_namespace(path).unwrap();
    let payload = serde_json::to_vec(value).unwrap();
    database
        .commit(&[osg_persistence::Change {
            domain: "games",
            key: "setup",
            expected_revision: database.revision("games", "setup").unwrap(),
            payload: Some(&payload),
        }])
        .unwrap();
}

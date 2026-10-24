//! Asynchronous game-side provisioning, separate from captured binding profiles.
use crate::game_discovery::Platform;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
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
    states: Mutex<BTreeMap<String, Value>>,
    catalogues: Mutex<provider_settings::CatalogueCache>,
    fingerprints: Mutex<crate::acr_setup::FingerprintCache>,
    ac_cars: Mutex<crate::ac_car_catalogue_cache::Cache>,
    #[cfg(test)]
    ac_cars_publication_pause:
        Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    #[cfg(test)]
    native_probe_pause: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    cancel: AtomicBool,
    busy: AtomicBool,
    closed: AtomicBool,
}
pub(crate) struct GameIntegrationSetup {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
    admission: Mutex<()>,
}
mod settings;
pub(crate) use settings::dcs_connection;
use settings::*;
mod inspection;
mod provider_settings;
mod provision;
mod requests;
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
                ac_cars: Mutex::new(crate::ac_car_catalogue_cache::Cache::new(
                    path.with_file_name("assetto-corsa-cars-v1.json"),
                )),
                #[cfg(test)]
                ac_cars_publication_pause: Mutex::new(None),
                #[cfg(test)]
                native_probe_pause: Mutex::new(None),
                path,
                native,
                settings_error: loaded.as_ref().err().cloned(),
                store,
                saved: Mutex::new(loaded.unwrap_or(json!({"schemaVersion":1,"games":{}}))),
                states: Mutex::new(states),
                catalogues: Mutex::new(BTreeMap::new()),
                fingerprints: Mutex::new(Default::default()),
                cancel: AtomicBool::new(false),
                busy: AtomicBool::new(false),
                closed: AtomicBool::new(false),
            }),
            worker: Mutex::new(None),
            admission: Mutex::new(()),
        }
    }
    /// New preparation forces a content hash; subsequent native admission checks
    /// metadata immediately, independently of the passive observer's interval.
    pub(crate) fn validate_native_build(
        &self,
        game: &Value,
        force_hash: bool,
    ) -> Result<(), String> {
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
            .lock()
            .unwrap_or_else(|p| p.into_inner())
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
        self.inner
            .ac_cars
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .decorate(game);
    }
    pub(crate) fn pending(&self) -> bool {
        crate::dcs_setup::recovery_pending(&self.inner.work)
    }
    pub(crate) fn decorate(&self, catalogue: &mut [Value]) {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        for game in catalogue {
            let id = game["id"].as_str().unwrap_or("");
            let cached = self
                .inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(id)
                .cloned();
            let mut status = cached.unwrap_or_else(|| {
                let status = self.inner.inspect(game, None);
                self.inner
                    .states
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(id.into(), status.clone());
                status
            });
            status["live"] = json!("waiting");
            status["liveMessage"]=json!("Setup is configuration metadata. Inspect the applied profile for actual source delivery.");
            if let Some(object) = status.as_object_mut() {
                object.remove("channels");
                object.remove("optionalMaxRpm");
            }
            if id == "dcs" {
                status["version"] = json!(crate::dcs_setup::VERSION);
            }
            self.inner.decorate_removal(id, &mut status);
            self.decorate_provider_settings(id, &mut status);
            game["integrationSetup"] = status;
        }
    }
    pub(crate) fn refresh(&self, catalogue: &[Value]) {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        self.refresh_admitted(catalogue);
    }
    // Request admission already owns this mutex. Keep passive inspection and
    // its state publication together so it cannot overwrite a new setup worker.
    fn refresh_admitted(&self, catalogue: &[Value]) {
        if self.busy() {
            return;
        }
        for game in catalogue {
            let status = self.inner.inspect(game, None);
            self.inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(game["id"].as_str().unwrap_or("").into(), status);
        }
    }
    pub(crate) fn shutdown(&self) {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        self.inner.closed.store(true, Ordering::Release);
        self.inner.cancel();
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

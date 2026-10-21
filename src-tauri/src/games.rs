//! Installation/provider metadata and explicit restoration of retained ownership.
//! Unified ProfileSession is the only graph/runtime authoring and launch path.
use crate::game_discovery::Platform;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
type CleanupIo = Arc<dyn Fn(Value) -> Value + Send + Sync>;
const CLIENT: &str = "cockpit-game-session-coordinator";
const GAMES: [&str; 7] = [
    "dcs",
    "assetto-corsa-rally",
    "x-plane-11",
    "x-plane-12",
    "assetto-corsa",
    "assetto-corsa-competizione",
    "assetto-corsa-evo",
];
fn known(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| keys.contains(&key.as_str())))
}
fn game_id(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .filter(|id| GAMES.contains(id))
        .ok_or_else(|| "Choose a supported game.".into())
}

/// Explicit paths prevent installation metadata from loading an old profile
/// library while retaining separately journaled cleanup and provider settings.
pub(crate) struct Paths {
    pub installations: PathBuf,
    pub provider_setup: PathBuf,
    pub recovery: PathBuf,
}
impl Paths {
    pub(crate) fn in_directory(root: &Path) -> Self {
        Self {
            installations: root.join("game-installations-v1.json"),
            provider_setup: root.join("games.integration-setup.json"),
            recovery: root.join("games.recovery.json"),
        }
    }
}
struct Settings {
    saved: crate::game_installations::Settings,
    metadata_error: Option<String>,
    catalogue: Vec<Value>,
    journal: Option<Value>,
    error: Option<String>,
    library_candidates: Vec<crate::game_library::Candidate>,
    library_diagnostics: Vec<String>,
    added_game_id: Option<String>,
}
struct Inner {
    panel: CleanupIo,
    platform: Arc<dyn Platform>,
    installations: crate::game_installations::Store,
    journal_path: PathBuf,
    setup: crate::game_integration_setup::GameIntegrationSetup,
    contexts: Mutex<crate::game_context::Broker>,
    vehicles: Mutex<crate::game_vehicles::Tracker>,
    dcs_vehicle_catalogues: Mutex<std::collections::BTreeSet<String>>,
    context_setup_check: Mutex<Option<Instant>>,
    activation: crate::game_activation::Observer,
    settings: Mutex<Settings>,
    state: Mutex<Value>,
    busy: AtomicBool,
    closed: AtomicBool,
    cancel: AtomicBool,
}
pub(crate) struct GamesRuntime {
    inner: Arc<Inner>,
    admission: Mutex<()>,
    worker: Mutex<Option<JoinHandle<()>>>,
    observer: Mutex<Option<JoinHandle<()>>>,
    activation_observer: Mutex<Option<JoinHandle<()>>>,
}
#[path = "game_activation_runtime.rs"]
mod activation_runtime;
#[path = "game_cleanup.rs"]
mod cleanup;
#[path = "game_context_runtime.rs"]
mod context_runtime;
#[path = "game_dcs_catalogues.rs"]
mod dcs_catalogues;
#[path = "profile_game_launch.rs"]
pub(crate) mod launch;
#[path = "game_metadata_api.rs"]
mod metadata_api;
pub(crate) use metadata_api::PreparedMetadata;
#[path = "game_telemetry_api.rs"]
mod telemetry_api;
pub(crate) use context_runtime::PreparedGame;

impl GamesRuntime {
    pub(crate) fn new(panel: CleanupIo, paths: Paths, platform: Arc<dyn Platform>) -> Self {
        let vehicles = crate::game_vehicles::Tracker::new(
            paths.installations.with_file_name("game-vehicles-v1.json"),
        );
        let installations = crate::game_installations::Store::new(paths.installations);
        let (saved, metadata_error) = match installations.load() {
            Ok(saved) => (saved.unwrap_or_default(), None),
            Err(error) => (Default::default(), Some(error)),
        };
        let mut error = metadata_error.clone();
        let journal = match cleanup::load(&paths.recovery) {
            Ok(journal) => journal,
            Err(issue) => {
                error=Some(format!("Retained game cleanup is invalid: {issue}. Preserve its journal until inspected."));
                Some(
                    json!({"invalid":true,"rawFingerprint":cleanup::fingerprint(&paths.recovery).ok()}),
                )
            }
        };
        let state = match &journal {
            Some(journal) => {
                json!({"phase":"recovery-needed","gameId":journal["gameId"],"retiredProfileId":journal["profileId"],"error":"A retained game owner needs explicit cleanup; no old profile was loaded or started."})
            }
            None => json!({"phase":"idle","error":null}),
        };
        let catalogue = crate::game_discovery::merged_catalogue(platform.as_ref(), &saved);
        let runtime = Self {
            inner: Arc::new(Inner {
                panel,
                setup: crate::game_integration_setup::GameIntegrationSetup::new(
                    paths.provider_setup,
                    platform.integration_setup_supported(),
                ),
                platform,
                installations,
                journal_path: paths.recovery,
                contexts: Mutex::new(Default::default()),
                vehicles: Mutex::new(vehicles),
                dcs_vehicle_catalogues: Mutex::new(Default::default()),
                context_setup_check: Mutex::new(None),
                activation: Default::default(),
                settings: Mutex::new(Settings {
                    saved,
                    metadata_error,
                    catalogue,
                    journal,
                    error,
                    library_candidates: vec![],
                    library_diagnostics: vec![],
                    added_game_id: None,
                }),
                state: Mutex::new(state),
                busy: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
            }),
            admission: Mutex::new(()),
            worker: Mutex::new(None),
            observer: Mutex::new(None),
            activation_observer: Mutex::new(None),
        };
        runtime.start_context_observer();
        runtime.start_activation_observer();
        runtime
    }
    pub(crate) fn resource_catalogue(&self) -> Vec<Value> {
        let mut catalogue = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .clone();
        for game in catalogue.iter_mut().filter(|game| game["custom"] != true) {
            self.inner.setup.decorate(std::slice::from_mut(game));
        }
        self.decorate_game_contexts(&mut catalogue);
        {
            let vehicles = self
                .inner
                .vehicles
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            for game in &mut catalogue {
                if game["custom"] == true {
                    continue;
                }
                vehicles.decorate(game);
                self.inner.setup.decorate_installed_cars(game);
            }
        }
        for game in &mut catalogue {
            game["profileContext"] = self
                .inner
                .activation
                .view(game["id"].as_str().unwrap_or(""));
        }
        crate::game_artwork::decorate(self.inner.installations.root(), &mut catalogue);
        catalogue
    }
    pub(crate) fn current_vehicle_id(&self, game_id: &str) -> Option<String> {
        self.inner
            .vehicles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .current(game_id)
            .and_then(|vehicle| vehicle["id"].as_str().map(str::to_owned))
    }
    #[cfg(test)]
    pub(crate) fn set_test_vehicle(&self, game_id: &str, vehicle_id: Option<&str>) {
        self.inner
            .vehicles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .set_test_vehicle(game_id, vehicle_id);
    }
    pub(crate) fn status(&self) -> Value {
        let catalogue = self.resource_catalogue();
        // UI reads consume the background observer cache; they cannot provide a
        // second process observation or manufacture a live provider identity.
        let observed = self.inner.activation.snapshot();
        let detected=observed.as_ref().map(|items|items.iter().map(|item|{
            let game=catalogue.iter().find(|game|game["id"]==item.identity.game_id);
            json!({"gameId":item.identity.game_id,"name":game.map(|game|game["name"].clone()).unwrap_or_else(||json!(item.identity.game_id))})
        }).collect::<Vec<_>>()).unwrap_or_default();
        let errors = catalogue
            .iter()
            .filter_map(|game| {
                let context = &game["profileContext"];
                (context["status"] == "unavailable").then(|| {
                    context["message"]
                        .as_str()
                        .unwrap_or("Native process observation is unavailable.")
                        .to_owned()
                })
            })
            .collect::<Vec<_>>();
        let settings = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        json!({"catalogue":catalogue,"detectedGames":detected,"detectionErrors":errors,"manualInstallations":settings.saved.manual_installations,"libraryCandidates":settings.library_candidates,"libraryDiagnostics":settings.library_diagnostics,"addedGameId":settings.added_game_id,"cleanup":self.inner.state.lock().unwrap_or_else(|p|p.into_inner()).clone(),"error":settings.error})
    }
    pub(crate) fn active(&self) -> bool {
        self.inner.busy.load(Ordering::Acquire)
            || self.inner.setup.busy()
            || self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .journal
                .is_some()
    }
    pub(crate) fn guard_profile_takeover(&self) -> Result<(), String> {
        if self.inner.busy.load(Ordering::Acquire)
            || self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .journal
                .is_some()
        {
            Err("Explicitly clean up or acknowledge retained game ownership before applying or launching a unified profile.".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn guard_request(&self, request: &Value) -> Result<(), String> {
        if self.guard_profile_takeover().is_ok() {
            return Ok(());
        }
        let command = request["command"].as_str().unwrap_or("");
        if command.starts_with("game_")
            || matches!(
                command,
                "ports"
                    | "connect"
                    | "disconnect"
                    | "module_usb_ports"
                    | "module_usb_list"
                    | "module_usb_status"
                    | "module_usb_connect"
                    | "module_usb_disconnect"
                    | "status"
                    | "config_read"
                    | "action_describe"
                    | "action_state"
                    | "action_ownership"
                    | "events_state"
                    | "events_read"
                    | "controller_list"
                    | "controller_inspect"
                    | "controller_visibility_status"
                    | "controller_visibility_inspect"
                    | "controller_visibility_processes"
                    | "companion_status"
                    | "companion_firmware_check"
                    | "companion_usb_ports"
                    | "companion_usb_status"
            )
        {
            return Ok(());
        }
        Err("Retained game cleanup owns its existing hardware evidence. Finish cleanup before independent writes.".into())
    }
    pub(crate) fn request(&self, request: &Value) -> Result<(), String> {
        if matches!(
            request["command"].as_str(),
            Some(
                "game_custom_save"
                    | "game_custom_remove"
                    | "game_background_save"
                    | "game_library_scan"
                    | "game_library_import"
            )
        ) {
            return self.metadata_request(request);
        }
        // Passive status validates its envelope below but must not wait behind
        // metadata publication or artwork collection.
        let _admission = (request["command"] != "game_get_state")
            .then(|| self.admission.lock().unwrap_or_else(|p| p.into_inner()));
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The game metadata runtime has closed.".into());
        }
        if !request.is_object()
            || serde_json::to_vec(request)
                .map_err(|e| e.to_string())?
                .len()
                > 65536
        {
            return Err("Invalid game metadata request.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("Choose a game metadata operation.")?;
        if command.starts_with("game_profile_")
            || matches!(command, "game_launch" | "game_activate" | "game_deactivate")
        {
            return Err("This independent game profile/activation API is retired. Use a unified profile and profile_game_launch.".into());
        }
        if matches!(
            command,
            "game_custom_save"
                | "game_custom_remove"
                | "game_background_save"
                | "game_library_scan"
                | "game_library_import"
        ) {
            return self.metadata_request(request);
        }
        if command.starts_with("game_integration_") {
            if command != "game_integration_cancel" {
                self.inner.guard_installation_metadata()?;
            }
            let id = game_id(&request["gameId"])?;
            let settings = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let game = settings
                .catalogue
                .iter()
                .find(|game| game["id"] == id)
                .cloned()
                .ok_or("Choose a current game installation.")?;
            if (settings.journal.is_some() || self.inner.busy.load(Ordering::Acquire))
                && !matches!(
                    command,
                    "game_integration_check" | "game_integration_cancel"
                )
            {
                return Err(
                    "Resolve retained game cleanup before changing its provider setup.".into(),
                );
            }
            drop(settings);
            let result = self
                .inner
                .setup
                .request(request, &game, self.inner.platform.clone());
            if result.is_ok() {
                self.inner.refresh_game_contexts(true);
            }
            return result;
        }
        let fields: &[&str] = match command {
            "game_get_state" | "game_scan" | "game_recover" => &[],
            "game_installation" => &["gameId", "executable"],
            "game_abandon" => &["preserveCurrentSetup"],
            _ => return Err("Unknown game metadata operation.".into()),
        };
        if !known(request, &[&["command", "clientId"][..], fields].concat()) {
            return Err("Unknown game metadata request field.".into());
        }
        if command == "game_get_state" {
            return Ok(());
        }
        if command == "game_recover" {
            return self.recover();
        }
        if command == "game_abandon" {
            return self.abandon(request);
        }
        self.guard_profile_takeover()?;
        let mut settings = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if command == "game_scan" {
            settings.catalogue = crate::game_discovery::merged_catalogue(
                self.inner.platform.as_ref(),
                &settings.saved,
            );
            self.inner.setup.refresh(
                &settings
                    .catalogue
                    .iter()
                    .filter(|game| game["custom"] != true)
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            return Ok(());
        }
        if let Some(error) = &settings.metadata_error {
            return Err(format!(
                "Repair installation metadata before changing executable locations: {error}"
            ));
        }
        let id = game_id(&request["gameId"])?;
        let mut next = settings.saved.clone();
        match request.get("executable") {
            Some(Value::Null) => {
                next.manual_installations.remove(id);
            }
            Some(Value::String(executable)) => {
                self.inner.platform.validate(id, executable)?;
                metadata_api::unique_executable(&settings.catalogue, id, Some(executable))?;
                next.manual_installations
                    .insert(id.into(), executable.clone());
            }
            _ => {
                return Err(
                    "Choose an executable string or explicit null to remove a manual installation."
                        .into(),
                )
            }
        }
        next.validate()?;
        self.inner.installations.save(&next)?;
        settings.catalogue =
            crate::game_discovery::merged_catalogue(self.inner.platform.as_ref(), &next);
        self.inner.setup.refresh(
            &settings
                .catalogue
                .iter()
                .filter(|game| game["custom"] != true)
                .cloned()
                .collect::<Vec<_>>(),
        );
        settings.saved = next;
        settings.error = None;
        drop(settings);
        self.inner.refresh_game_contexts(true);
        Ok(())
    }
    fn spawn(&self, work: impl FnOnce() + Send + 'static) {
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = worker.take() {
            let _ = previous.join();
        }
        *worker = Some(thread::spawn(work));
    }
    fn recover(&self) -> Result<(), String> {
        if self.inner.busy.load(Ordering::Acquire) {
            return Err("Wait for retained game cleanup to finish.".into());
        }
        let journal = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal
            .clone();
        let Some(journal) = journal else {
            self.inner.phase("idle", None);
            return Ok(());
        };
        let journal = cleanup::validate_journal(journal)?;
        let lease = cleanup::lease(&self.inner.journal_path)?;
        self.inner.verify_journal()?;
        self.inner.busy.store(true, Ordering::Release);
        self.inner.cancel.store(false, Ordering::Release);
        self.inner.phase("restoring", None);
        let inner = self.inner.clone();
        self.spawn(move || {
            let _lease = lease;
            let result = inner.restore(journal);
            inner.finish(result, None);
        });
        Ok(())
    }
    fn abandon(&self, request: &Value) -> Result<(), String> {
        if request["preserveCurrentSetup"] != true {
            return Err(
                "Explicitly confirm preserving current setup without restoring the retained owner."
                    .into(),
            );
        }
        if self.inner.busy.load(Ordering::Acquire) {
            return Err(
                "Wait for retained cleanup to finish before acknowledging its journal.".into(),
            );
        }
        let lease = cleanup::lease(&self.inner.journal_path)?;
        let journal = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal
            .clone()
            .ok_or("No retained game journal needs acknowledgement.")?;
        // An invalid journal has no trusted owner. Archive only this exact file;
        // separate output/protected ownership records remain explicit recovery.
        if journal["invalid"] != true {
            self.inner.verify_journal()?;
            if journal["companions"]["taken"] == true {
                self.inner.call(
                    "_game_companion_abandon",
                    json!({"companions":journal["companions"]}),
                )?;
            }
            self.inner.verify_journal()?;
        } else {
            let expected=journal["rawFingerprint"].as_str().ok_or("The rejected cleanup file has no bounded native snapshot; preserve it for inspection.")?;
            if cleanup::fingerprint(&self.inner.journal_path)? != expected {
                return Err(
                    "The rejected cleanup file changed after inspection; it was preserved.".into(),
                );
            }
        }
        let archive = self
            .inner
            .journal_path
            .with_file_name(format!("games.abandoned-{}.json", uuid::Uuid::new_v4()));
        fs::rename(&self.inner.journal_path, &archive).map_err(|e| e.to_string())?;
        let mut settings = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        settings.journal = None;
        settings.error = settings.metadata_error.clone();
        drop(settings);
        drop(lease);
        *self.inner.state.lock().unwrap_or_else(|p| p.into_inner()) =
            json!({"phase":"idle","error":null,"recoveryArchive":archive.to_string_lossy()});
        Ok(())
    }
    pub(crate) fn shutdown(&self) {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        self.inner.closed.store(true, Ordering::Release);
        self.inner.cancel.store(true, Ordering::Release);
        for worker in [&self.activation_observer, &self.observer, &self.worker] {
            if let Some(worker) = worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
                let _ = worker.join();
            }
        }
        self.inner.setup.shutdown();
    }
}
impl Drop for GamesRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Inner {
    fn guard_installation_metadata(&self) -> Result<(), String> {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        match &settings.metadata_error {
            Some(error) => Err(format!(
                "Repair game installation metadata before selecting or launching a game: {error}"
            )),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "games_tests.rs"]
mod tests;

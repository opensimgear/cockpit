//! Current installation/provider metadata. Retired game data is never opened.
//! Unified ProfileSession is the only graph/runtime authoring and launch path.
use crate::game_discovery::Platform;
#[cfg(test)]
use std::fs;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
type CleanupIo = Arc<dyn Fn(Value) -> Value + Send + Sync>;
fn known(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| keys.contains(&key.as_str())))
}
fn game_id(value:&Value)->Result<&str,String>{value.as_str().filter(|id|crate::game_installations::valid_game_id(id)).ok_or_else(||"Choose a discovered game.".into())}

/// Explicit paths prevent installation metadata from loading an old profile
/// library; current provider settings have their own SQLite owner.
pub(crate) struct Paths {
    pub installations: PathBuf,
    pub provider_setup: PathBuf,
    #[cfg(test)]
    pub recovery: PathBuf,
}
impl Paths {
    pub(crate) fn in_directory(root: &Path) -> Self {
        Self {
            installations: root.join("game-library.json"),
            provider_setup: root.join("games.integration-setup.json"),
            #[cfg(test)]
            recovery: root.join("games.recovery.json"),
        }
    }
}
struct Settings {
    saved: crate::game_installations::Settings,
    metadata_error: Option<String>,
    catalogue: Vec<Value>,
    error: Option<String>,
    added_game_id: Option<String>,
}
struct Inner {
    platform: Arc<dyn Platform>,
    installations: crate::game_installations::Store,
    setup: crate::game_integration_setup::GameIntegrationSetup,
    contexts: Mutex<crate::game_context::Broker>,
    vehicles: Mutex<crate::game_vehicles::Tracker>,
    dcs_vehicle_catalogues: Mutex<std::collections::BTreeSet<String>>,
    context_setup_check: Mutex<Option<Instant>>,
    activation: crate::game_activation::Observer,
    settings: Mutex<Settings>,
    discovery:Mutex<discovery_review::Review>,
    online_artwork: bool,
    closed: AtomicBool,
}
pub(crate) struct GamesRuntime {
    inner: Arc<Inner>,
    admission: Mutex<()>,
    observer: Mutex<Option<JoinHandle<()>>>,
    discovery_worker:Mutex<Option<JoinHandle<()>>>,
    activation_observer: Mutex<Option<JoinHandle<()>>>,
}
#[path = "game_activation_runtime.rs"]
mod activation_runtime;
#[path = "game_context_runtime.rs"]
mod context_runtime;
#[path = "game_dcs_catalogues.rs"]
mod dcs_catalogues;
#[path = "profile_game_launch.rs"]
pub(crate) mod launch;
#[path = "game_metadata_api.rs"]
mod metadata_api;
#[path="games/metadata_read.rs"]
mod metadata_read;
#[path = "games/artwork_options.rs"]
mod artwork_options;
#[path="activity/games.rs"]
mod activity_capture;
#[path="games/review/mod.rs"]
mod discovery_review;
pub(crate) use metadata_api::PreparedMetadata;
#[path = "game_telemetry_api.rs"]
mod telemetry_api;
pub(crate) use context_runtime::PreparedGame;

impl GamesRuntime {
    pub(crate) fn local_provider_settings(&self, id: &str) -> (Value, String) {
        let key=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner()).catalogue.iter().find(|game|game["id"]==id)
            .and_then(|game|game["integrationKey"].as_str()).unwrap_or(id).to_owned();
        self.inner.setup.provider_preferences_snapshot(&key)
    }
    pub(crate) fn new(panel:CleanupIo,paths:Paths,platform:Arc<dyn Platform>)->Self {Self::construct(panel,paths,platform,true)}
    fn construct(_panel: CleanupIo, paths: Paths, platform: Arc<dyn Platform>,auto_discovery:bool) -> Self {
        let vehicles = crate::game_vehicles::Tracker::new(
            paths.installations.with_file_name("game-vehicles-v1.json"),
        );
        let installations = crate::game_installations::Store::new(paths.installations);
        let (saved, metadata_error) = match installations.load() {
            Ok(saved) => (saved.unwrap_or_default(), None),
            Err(error) => (Default::default(), Some(error)),
        };
        let first_start=auto_discovery && !saved.discovery_initialized && platform.integration_setup_supported();
        let error=metadata_error.clone();
        let catalogue = crate::game_discovery::merged_catalogue(platform.as_ref(), &saved);
        let runtime = Self {
            inner: Arc::new(Inner {
                online_artwork: auto_discovery,
                setup: crate::game_integration_setup::GameIntegrationSetup::new(
                    paths.provider_setup,
                    platform.integration_setup_supported(),
                ),
                platform,
                installations,
                contexts: Mutex::new(Default::default()),
                vehicles: Mutex::new(vehicles),
                dcs_vehicle_catalogues: Mutex::new(Default::default()),
                context_setup_check: Mutex::new(None),
                activation: Default::default(),
                settings: Mutex::new(Settings {
                    saved,
                    metadata_error,
                    catalogue,
                    error,
                    added_game_id: None,
                }),
                discovery:Mutex::new(Default::default()),
                closed: AtomicBool::new(false),
            }),
            admission: Mutex::new(()),
            observer: Mutex::new(None),
            discovery_worker:Mutex::new(None),
            activation_observer: Mutex::new(None),
        };
        runtime.start_context_observer();
        runtime.start_activation_observer();
        if first_start {let _=discovery_review::start(&runtime,&json!({"command":"game_discovery_start"}));}
        runtime
    }
    pub(crate) fn resource_catalogue(&self) -> Vec<Value> {
        let catalogue = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .clone();
        self.decorate_resource_catalogue(catalogue)
    }
    pub(crate) fn authoritative_resource_catalogue(&self) -> Result<Vec<Value>, String> {
        let catalogue = {
            let settings = self.inner.settings.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(error) = &settings.metadata_error {
                return Err(error.clone());
            }
            settings.catalogue.clone()
        };
        Ok(self.decorate_resource_catalogue(catalogue))
    }
    fn decorate_resource_catalogue(&self, mut catalogue: Vec<Value>) -> Vec<Value> {
        for game in catalogue.iter_mut().filter(|game| game["provider"].is_string()) {
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
                if !game["provider"].is_string() {
                    continue;
                }
                vehicles.decorate(game);
            }
        }
        for game in catalogue.iter_mut().filter(|game| game["provider"].is_string()) {
            self.inner.setup.decorate_installed_cars(game);
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
        let mut state=json!({"catalogue":catalogue,"detectedGames":detected,"detectionErrors":errors,"tags":settings.saved.tags,"discovery":self.inner.discovery.lock().unwrap_or_else(|p|p.into_inner()).clone(),"addedGameId":settings.added_game_id,"error":settings.error});
        drop(settings);
        if self.inner.online_artwork && !self.inner.closed.load(Ordering::Acquire) {
            crate::game_artwork::steam_online::decorate(self.inner.installations.root(), &mut state);
        }
        state
    }
    pub(crate) fn recovery_pending(&self)->bool{self.inner.setup.pending()}
    pub(crate) fn active(&self)->bool{self.inner.setup.busy()}
    pub(crate) fn guard_profile_takeover(&self)->Result<(),String>{
        if self.inner.closed.load(Ordering::Acquire){Err("The game metadata runtime has closed.".into())}else{Ok(())}
    }
    pub(crate) fn guard_request(&self,_request:&Value)->Result<(),String>{self.guard_profile_takeover()}
    pub(crate) fn request(&self,request:&Value)->Result<(),String> {
        let command=request["command"].as_str().ok_or("Choose a game operation.")?;
        if self.inner.closed.load(Ordering::Acquire){return Err("The game runtime has closed.".into());}
        if command=="game_discovery_start" {return discovery_review::start(self,request);}
        if matches!(command,"game_discovery_apply"|"game_save"|"game_rescan"|"game_remove"|"game_installation_select"|"game_tag_definitions_save"|"game_tags_save"|"game_background_save") {return self.metadata_request(request);}
        if command=="game_get_state" {
            if !known(request,&["command","clientId"]) {return Err("Unknown game status field.".into());}return Ok(());
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
        Err("Unknown game operation.".into())
    }
    #[cfg(test)]
    pub(crate) fn test_runtime(panel:CleanupIo,paths:Paths,platform:Arc<dyn Platform>)->Self {
        let mut catalogue=platform.catalogue(&Default::default());
        for game in &mut catalogue {
            if game.get("integrationCompatible").is_none() && game["provider"].is_string() {game["integrationCompatible"]=json!(true);}
            if game.get("integrationKey").is_none() && crate::game_discovery::integration_provider(game["id"].as_str().unwrap_or("")).is_some_and(|provider|game["provider"]==provider) {game["integrationKey"]=game["id"].clone();}
        }
        let runtime=Self::construct(panel,paths,platform,false);
        if !catalogue.is_empty() {runtime.seed_test_catalogue(catalogue);}
        let observer_missing=runtime.activation_observer.lock().unwrap().is_none();
        if observer_missing {runtime.start_activation_observer();}
        runtime
    }
    #[cfg(test)]
    pub(crate) fn stage_test_discovery(
        &self,
        candidates: Vec<crate::game_library::Candidate>,
    ) -> Value {
        let saved = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .saved
            .clone();
        let review = discovery_review::Review {
            scan_id: uuid::Uuid::new_v4().to_string(),
            status: "ready".into(),
            entries: discovery_review::test_reconcile(&saved, candidates),
            revision: saved.revision,
            ..Default::default()
        };
        *self.inner.discovery.lock().unwrap_or_else(|p| p.into_inner()) = review.clone();
        serde_json::to_value(review).expect("test discovery review serializes")
    }
    #[cfg(test)]
    pub(crate) fn register_test_game(&self,name:&str,path:&str)->String {
        use crate::game_installations::{GameRecord,Installation,LibrarySource};
        use std::collections::BTreeMap;
        let executable=crate::game_discovery::validate_executable(path).unwrap().to_string_lossy().into_owned();
        let root=Path::new(&executable).parent().unwrap().to_string_lossy().into_owned();
        let id=format!("game-{}",uuid::Uuid::new_v4());let instance=format!("installation-{}",uuid::Uuid::new_v4());
        let mut settings=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner());
        settings.saved.games.insert(id.clone(),GameRecord{name:name.into(),installations:BTreeMap::from([(instance.clone(),Installation{root:root.clone(),executable:Some(executable.clone()),executables:vec![executable],sources:vec![LibrarySource{client:"disk".into(),app_id:instance.clone(),install_path:root,launcher:None}]})]),selected_installation:Some(instance),integration:None,default_background:None});
        settings.saved.revision+=1;self.inner.installations.save(&settings.saved).unwrap();
        let catalogue=crate::game_discovery::merged_catalogue(self.inner.platform.as_ref(),&settings.saved);
        for game in catalogue {settings.catalogue.retain(|old|old["id"]!=game["id"]);settings.catalogue.push(game);}
        settings.added_game_id=Some(id.clone());id
    }
    #[cfg(test)]
    pub(crate) fn seed_test_catalogue(&self,catalogue:Vec<Value>) {
        let mut settings=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner());settings.catalogue=catalogue;
    }
    pub(crate) fn shutdown(&self) {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        self.inner.closed.store(true, Ordering::Release);
        for worker in [&self.activation_observer, &self.observer, &self.discovery_worker] {
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

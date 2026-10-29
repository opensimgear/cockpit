//! Local operator controls. The WSS endpoint never exposes these commands.
use osg_companion_service::server::{ServerConfig, ServerRuntime};
use osg_companion_service::{qrcode_matrix, Hub};
use osg_mapping_host::companions::CompanionProvider;
use serde_json::{json, Value};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

include!(concat!(env!("OUT_DIR"), "/companion_browser.rs"));

const MAX_UPLOAD: usize = 13 * 1024 * 1024;
pub(crate) struct ProfilePanelOperation {
    pub operation: Value,
    pub live_session: bool,
    pub fresh: bool,
}
mod presentation;
use presentation::*;

#[path = "../../profile_companion_authoring.rs"]
pub(crate) mod authoring;
struct Upload {
    id: String,
    owner: String,
    expected: usize,
    bytes: Vec<u8>,
    started: Instant,
}
struct Services {
    hub: Hub,
    server: ServerRuntime,
    usb: osg_companion_serial::Manager,
    usb_setup: Mutex<()>,
    firmware_releases: crate::firmware_releases::Resolver,
    directory: PathBuf,
    saved_listener: crate::companion_settings::Listener,
    panel_cache: Mutex<()>,
}
struct Metadata {
    hub: Hub,
    directory: PathBuf,
}
pub struct Runtime {
    path: Option<PathBuf>,
    stopped: AtomicBool,
    provider: Arc<CompanionProvider>,
    metadata: OnceLock<Metadata>,
    metadata_init: Mutex<()>,
    services: OnceLock<Result<Services, String>>,
    upload: Mutex<Option<Upload>>,
    #[cfg(test)]
    profile_wire_fixture: bool,
}
fn interface_addresses()->Vec<String>{
    osg_companion_service::server::local_interfaces()
            .ok()
            .and_then(|value| {
                value.as_array().map(|rows| {
                    rows.iter()
                        .filter_map(|row| row["ip"].as_str().map(str::to_owned))
                        .collect()
                })
            })
            .unwrap_or_default()
}
impl Runtime {
    pub fn new(path: Option<PathBuf>, provider: Arc<CompanionProvider>) -> Self {
        // Construction stays inert. Native startup initializes protected
        // companion state from the detection worker, where OS store errors are
        // retained and surfaced by companion status.
        Self {
            path,
            stopped: AtomicBool::new(false),
            provider,
            metadata: OnceLock::new(),
            metadata_init: Mutex::new(()),
            services: OnceLock::new(),
            upload: Mutex::new(None),
            #[cfg(test)]
            profile_wire_fixture: false,
        }
    }
    pub fn start_detection(self: &Arc<Self>) {
        let owner = Arc::downgrade(self);
        std::thread::spawn(move || loop {
            let Some(runtime) = owner.upgrade() else {
                break;
            };
            if runtime.stopped.load(Ordering::SeqCst) {
                break;
            }
            // Initialize the installation's protected host identity on first
            // native startup, even before the listener is enabled. `services`
            // caches startup errors so an unavailable store cannot trigger a
            // fresh CA on a later polling pass.
            if let Ok(services) = runtime.services() {
                if crate::companion_panels::ready(&services.hub).unwrap_or(false) {
                    Self::start_saved_listener(services);
                    let _ = services.usb.reconnect_detected(&services.hub);
                }
            }
            drop(runtime);
            std::thread::sleep(Duration::from_secs(2));
        });
    }
    fn existing_metadata(&self)->Result<Option<&Metadata>,String>{
        if self.stopped.load(Ordering::SeqCst){return Err("Companion service is stopped.".into());}
        if let Some(metadata)=self.metadata.get(){return Ok(Some(metadata));}
        let Some(path)=&self.path else{return Ok(None)};
        let Some(database)=osg_persistence::Database::existing_for_namespace(&path.join("companion-host-state.dpapi"))? else{return Ok(None)};
        if database.read("secrets","companion-host")?.is_none()&&database.read("companion","publications")?.is_none(){return Ok(None);}
        self.metadata(false).map(Some)
    }
    fn metadata(&self, create: bool) -> Result<&Metadata, String> {
        if self.stopped.load(Ordering::SeqCst) { return Err("Companion service is stopped.".into()); }
        if let Some(metadata) = self.metadata.get() { return Ok(metadata); }
        let _initialization = self.metadata_init.lock().unwrap_or_else(|p|p.into_inner());
        if self.stopped.load(Ordering::SeqCst) { return Err("Companion service is stopped.".into()); }
        if let Some(metadata) = self.metadata.get() { return Ok(metadata); }
        let (hub, directory) = if let Some(path) = &self.path {
            let hub = if create { Hub::open(path, self.provider.clone())? }
                else { Hub::open_existing(path, self.provider.clone())?
                    .ok_or("Set up and pair a companion before applying this assignment.")? };
            (hub, path.clone())
        } else if create {
            (Hub::new(format!("host-{}", uuid::Uuid::new_v4()), self.provider.clone()),
                std::env::temp_dir().canonicalize().map_err(|error|error.to_string())?.join(format!("osg-companion-test-{}", uuid::Uuid::new_v4())))
        } else { return Err("Set up and pair a companion before applying this assignment.".into()); };
        let _ = self.metadata.set(Metadata {hub, directory});
        Ok(self.metadata.get().expect("metadata initialized under its gate"))
    }
    fn services(&self) -> Result<&Services, String> {
        self.services
            .get_or_init(|| {
                let metadata = self.metadata(true)?;
                let hub = metadata.hub.clone();
                let directory = metadata.directory.clone();
                let server = ServerRuntime::with_browser(hub.clone(), directory.clone(), BROWSER_ASSETS)?;
                let saved_listener=crate::companion_settings::Listener::open(&directory)?;
                let services=Services {
                    hub, server, directory,
                    saved_listener,
                    panel_cache:Mutex::new(()),
                    usb: osg_companion_serial::Manager::default(),
                    usb_setup: Mutex::new(()),
                    firmware_releases: crate::firmware_releases::Resolver::default(),
                };
                Self::start_saved_listener(&services);
                Ok(services)
            })
            .as_ref()
            .map_err(Clone::clone)
    }
    fn start_saved_listener(services:&Services){
        if !crate::companion_panels::ready(&services.hub).unwrap_or(false){return;}
        if services.server.status().enabled{return;}
        services.saved_listener.with_config(|config|if let Some(config)=config{
            if let Err(error)=services.server.start(config){
                services.server.record_error(format!("Saved companion listener could not start: {error}"));
            }
        });
    }
    fn inventory(&self)->Result<Value,String>{
        if self.stopped.load(Ordering::SeqCst){return Err("Companion service is stopped.".into());}
        if let Some(Ok(services))=self.services.get(){return self.status(services);}
        let directory=self.metadata.get().map(|metadata|&metadata.directory).or(self.path.as_ref());
        let database=directory.map(|directory|osg_persistence::Database::existing_for_namespace(&directory.join("companion-host-state.dpapi"))).transpose()?.flatten();
        let metadata=self.existing_metadata()?;
        let host=match metadata{Some(metadata)=>metadata.hub.command(&json!({"command":"companion_status"}))?,None=>json!({"clients":[],"pending":[],"instances":[],"publications":[]})};
        let(revision,config)=if database.is_some(){crate::companion_settings::Listener::open(directory.ok_or("Companion directory is unavailable.")?)?.refresh()?}else{(0,None)};
        let firmware=match metadata{Some(metadata) if database.is_some()=>crate::companion_settings::firmware(&metadata.directory)?.1,_=>json!({})};
        let error=self.services.get().and_then(|result|result.as_ref().err());
        Ok(json!({"host":host,"listener":{"enabled":false,"configuredEnabled":config.is_some(),"settingsRevision":revision,"config":config,"lastError":error},"firmware":firmware,"usb":{"connected":false},"interfaces":interface_addresses(),"browserUrl":null}))
    }
    fn status(&self, services: &Services) -> Result<Value, String> {
        let interfaces=interface_addresses();
        let (firmware_repository,mut firmware)=crate::companion_settings::firmware(&services.directory)?;
        let previous_firmware=firmware.clone();
        let mut host=services.hub.command(&json!({"command":"companion_status"}))?;
        firmware.as_object_mut().unwrap().retain(|id,_|host["clients"].as_array().is_some_and(|clients|clients.iter().any(|client|client["companionId"]==*id)));
        describe_usb_presence(&mut host,&services.usb.statuses());
        for usb in services.usb.statuses(){
        if let Some(id)=usb["device"]["companionId"].as_str().filter(|id|host["clients"].as_array().is_some_and(|rows|rows.iter().any(|c|c["companionId"]==*id))){
            let mut info=json!({"version":usb["device"]["firmwareVersion"].as_str().unwrap_or("unknown"),"profile":usb["device"]["boardProfile"].as_str().or_else(||firmware[id]["profile"].as_str()),"connectionMode":usb["device"]["connectionMode"].as_str().filter(|mode|matches!(*mode,"wifi"|"serial")).or_else(||firmware[id]["connectionMode"].as_str())});
            if let Ok(profile)=osg_companion_service::render_profile::validate(&firmware[id]["renderProfile"]){if profile["firmwareVersion"]==info["version"]&&profile["boardProfile"]==info["profile"]{info["renderProfile"]=profile;}}
            firmware[id]=info;
        }
        }
        if let Some(clients)=host["clients"].as_array(){for client in clients{
            if client["kind"]!="esp32"{continue;}
            let Some(id)=client["companionId"].as_str() else{continue};
            if client["renderProfileError"].is_null(){if let Ok(profile)=osg_companion_service::render_profile::validate(&client["renderProfile"]){
                if !firmware[id].is_object(){firmware[id]=json!({});}
                firmware[id]["version"]=profile["firmwareVersion"].clone();firmware[id]["profile"]=profile["boardProfile"].clone();firmware[id]["renderProfile"]=profile;
                continue;
            }}
            if client["connected"]==true{if let Some(info)=firmware[id].as_object_mut(){info.remove("renderProfile");}}
        }}
        if firmware!=previous_firmware{firmware_repository.save(&firmware)?;}
        if let Some(metadata)=firmware.as_object_mut(){for info in metadata.values_mut(){
            if let Ok(profile)=osg_companion_service::render_profile::validate(&info["renderProfile"]){info["width"]=profile["screen"]["width"].clone();info["height"]=profile["screen"]["height"].clone();}
            else if let Some((width,height))=screen_dimensions(info["profile"].as_str().unwrap_or("")){info["width"]=json!(width);info["height"]=json!(height);}
        }}
        if let Some(clients)=host["clients"].as_array_mut(){for client in clients{if client["kind"]=="esp32"{
            if let Ok(profile)=osg_companion_service::render_profile::validate(&client["renderProfile"]){client["screen"]=profile["screen"].clone();}
            else if let Some(id)=client["companionId"].as_str(){if let Some(size)=screen_dimensions(firmware[id]["profile"].as_str().unwrap_or("")){client["screen"]=json!({"width":size.0,"height":size.1});}}
        }}}
        let mut listener=serde_json::to_value(services.server.status()).map_err(|e|e.to_string())?;
        let (revision,config)=services.saved_listener.refresh()?;
        listener["settingsRevision"]=json!(revision);
        listener["configuredEnabled"]=json!(config.is_some());
        listener["config"]=json!(config);
        let mut status=json!({"firmware":firmware,"host":host, "listener":listener,"browserUrl":services.server.browser_url().ok(), "interfaces":interfaces,"usb":services.usb.status()});
        status.as_object_mut().unwrap().extend(services.firmware_releases.metadata().as_object().unwrap().clone());
        Ok(status)
    }
    pub(crate) fn hub(&self) -> Result<Hub, String> {
        Ok(self.services()?.hub.clone())
    }
    /// A retained panel and paired installation are setup metadata, not a live
    /// transport, input baseline, or authority to change an assignment.
    pub(crate) fn profile_metadata(&self) -> Result<Hub, String> {
        Ok(self.metadata(false)?.hub.clone())
    }
    pub(crate) fn saved_panel_updates(&self) -> Result<Vec<crate::companion_panels::SavedPanel>,String> {
        match self.existing_metadata()?{Some(metadata)=>crate::companion_panels::latest(&metadata.hub),None=>Ok(vec![])}
    }
    pub(crate) fn saved_panel_heads(&self) -> Result<std::collections::BTreeMap<String,String>,String> {
        match self.existing_metadata()?{Some(metadata)=>crate::companion_panels::saved_heads(&metadata.hub),None=>Ok(Default::default())}
    }
    pub(crate) fn saved_panel_references(&self) -> Result<Vec<crate::companion_panels::SavedPanel>,String> {
        Ok(self.existing_metadata()?.map_or_else(Vec::new,|metadata|metadata.hub.saved_panel_references()))
    }
    /// Read-only preparation diagnostics. A recent nonce-verified USB report
    /// can explain failures before session-open; it never supplies a baseline.
    /// Stale snapshots are returned only so a wait can fence its initial retry.
    pub(crate) fn profile_panel_operation(&self, instance: &str, publication: &str) -> Option<ProfilePanelOperation> {
        let hub = &self.metadata.get()?.hub;
        if let Some(operation) = hub.panel_operation(instance,publication) {
            return Some(ProfilePanelOperation { operation, live_session:true, fresh:true });
        }
        let services = self.services.get()?.as_ref().ok()?;
        let snapshot = hub.assignment_snapshot(&[instance.into()]).ok()?;
        let assignment = snapshot["assignments"].as_array()?.iter().find(|row|
            row["instanceId"] == instance && row["publicationHash"] == publication)?;
        usb_panel_operation(assignment,&services.usb.statuses())
    }
    /// Capture before assignment mutation. The previous panel's attempt is
    /// retained only as a retry fence, never as diagnostics for the new target.
    pub(crate) fn profile_panel_operation_history(&self, instance: &str) -> Option<Value> {
        let hub=&self.metadata.get()?.hub;
        let snapshot=hub.assignment_snapshot(&[instance.into()]).ok()?;
        let assignment=snapshot["assignments"].as_array()?.iter().find(|row|row["instanceId"] == instance)?;
        if let Some(Ok(services))=self.services.get() {
            if let Some(report)=usb_panel_snapshot(assignment["companionId"].as_str()?,&services.usb.statuses()) {
                return Some(report.operation);
            }
        }
        hub.panel_operation(instance,assignment["publicationHash"].as_str()?)
    }
    /// Activation is an explicit retry; a stopped/faulted USB companion gets
    /// one reconnect request after its journaled assignment has been acquired.
    pub(crate) fn profile_retry_panel(&self,instance:&str,publication:&str)->Result<(),String>{
        let Some(Ok(services))=self.services.get() else{return Ok(());};
        let snapshot=services.hub.assignment_snapshot(&[instance.into()])?;
        let assignment=snapshot["assignments"].as_array().and_then(|rows|rows.iter().find(|row|row["instanceId"]==instance&&row["publicationHash"]==publication))
            .ok_or("The companion assignment changed before its explicit panel retry.")?;
        let Some(companion)=assignment["companionId"].as_str() else{return Ok(());};
        let faulted=services.usb.statuses().iter().any(|usb|usb["connected"]==true&&usb["ready"]==true&&usb["device"]["companionId"]==companion
            &&matches!(usb["device"]["status"].as_str(),Some("error"|"offline"|"panel-error")));
        if faulted{
            let ms=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"System clock is invalid.")?.as_millis() as u64;
            services.usb.record_for(Some(companion),json!({"command":"connect","unixMs":ms}))?;
        }
        Ok(())
    }
    /// Call only from the journaled acquisition phase.
    pub(crate) fn profile_acquire_transports(&self) -> Result<Hub, String> {
        // Supplied-Hub fixtures already drive the real wire protocol and native
        // provider. They must not additionally enter an OS credential service
        // to create an unrelated TLS listener identity.
        #[cfg(test)]
        if self.profile_wire_fixture {
            return self.profile_metadata();
        }
        Ok(self.services()?.hub.clone())
    }
    pub(crate) fn profile_provider(&self) -> Arc<CompanionProvider> {
        self.provider.clone()
    }
    pub(crate) fn game_command(&self, command: &str, request: &Value) -> Result<Value, String> {
        if !matches!(command,"_game_companion_restore"|"_game_companion_abandon") {
            return Err("Retired independent game companion operation.".into());
        }
        let hub = self.profile_metadata()?;
        match command {
            "_game_companion_restore" => {
                game_owner::restore(&hub, &request["companions"], request["recovery"] == true)
            }
            "_game_companion_abandon" => game_owner::abandon(&hub, &request["companions"]),
            _ => Err("Unknown trusted game companion operation.".into()),
        }
    }
    /// Local mapping uses the same trusted, immutable revision as the authorized
    /// native instance. This getter never starts the listener or game output.
    pub fn assigned_publication(
        &self,
        instance_id: &str,
    ) -> Result<osg_companion_service::Publication, String> {
        self.metadata(false)?.hub.assigned_publication(instance_id)
    }
    pub(crate) fn resource_transport(&self, instance_id: &str) -> Option<osg_companion_service::SourceTransport> {
        self.metadata.get()?.hub.source_transport(instance_id)
    }
    /// Existing pairing metadata only. Inventory must neither create a host nor
    /// start its listener/USB services; read failures are not an empty inventory.
    pub(crate) fn resource_instance_ids(&self) -> Result<std::collections::BTreeSet<String>, String> {
        Ok(self.existing_metadata()?.map_or_else(Default::default,|metadata|metadata.hub.resource_instance_ids()))
    }
    pub(crate) fn resource_companions(&self) -> Result<Vec<osg_companion_service::ResourceCompanion>,String> {
        Ok(self.existing_metadata()?.map_or_else(Vec::new,|metadata|metadata.hub.resource_companions()))
    }
    pub(crate) fn resource_publication(&self, instance_id: &str) -> Result<osg_companion_service::Publication, String> {
        self.metadata.get().ok_or("Companion setup is not initialized; no inventory access starts it.")?
            .hub.assigned_publication(instance_id)
    }

    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(Ok(services)) = self.services.get() {
            services.usb.release();
            services.server.stop();
        }
        self.upload.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

#[cfg(test)]
impl Runtime {
    #[cfg(test)]
    pub(crate) fn with_profile_metadata(hub:Hub, provider:Arc<CompanionProvider>, directory:PathBuf)->Self {
        let mut runtime=Self::new(None,provider);
        assert!(runtime.metadata.set(Metadata {hub,directory}).is_ok());
        runtime.profile_wire_fixture = true;
        runtime
    }
}

#[path = "../../companion_game_owner.rs"]
pub(crate) mod game_owner;

mod commands;
#[cfg(test)]
mod tests;

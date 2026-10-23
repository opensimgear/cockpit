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
fn usb_panel_snapshot(companion: &str, devices: &[Value]) -> Option<ProfilePanelOperation> {
    let mut matching = devices.iter().filter(|usb|usb["connected"] == true && usb["ready"] == true
        && usb["device"]["companionId"] == companion);
    let usb = matching.next()?;
    if matching.next().is_some() { return None; }
    let reported=&usb["device"]["operation"];
    let operation = osg_companion_service::operation::validate_panel_operation(
        reported,reported["instanceId"].as_str()?,reported["publicationHash"].as_str()?).ok()?;
    if operation["attemptId"].as_u64().is_none_or(|attempt|attempt == 0)
        || operation.get("sessionId").is_some_and(|session|session != &usb["device"]["sessionId"])
        || operation.get("generation").is_some_and(|generation|generation != &usb["device"]["generation"]) { return None; }
    Some(ProfilePanelOperation { operation, live_session:false,
        fresh:usb["statusAgeMs"].as_u64().is_some_and(|age|age < 1500) })
}
fn usb_panel_operation(assignment: &Value, devices: &[Value]) -> Option<ProfilePanelOperation> {
    let report=usb_panel_snapshot(assignment["companionId"].as_str()?,devices)?;
    osg_companion_service::operation::validate_panel_operation(&report.operation,
        assignment["instanceId"].as_str()?,assignment["publicationHash"].as_str()?).ok()?;
    Some(report)
}
fn usb_render_profile(usb:&Value)->Option<Value>{
    if usb["connected"]!=true||usb["ready"]!=true{return None;}
    let device=&usb["device"];let profile=osg_companion_service::render_profile::validate(&device["renderProfile"]).ok()?;
    (profile["firmwareVersion"]==device["firmwareVersion"]&&profile["boardProfile"]==device["boardProfile"]).then_some(profile)
}
fn describe_usb_presence(host:&mut Value,devices:&[Value]){
    let assignments=host["instances"].as_array().cloned().unwrap_or_default();
    if let Some(clients)=host["clients"].as_array_mut(){for client in clients{
        let Some(companion)=client["companionId"].as_str().filter(|id|!id.is_empty()) else{continue};
        let matching=|usb:&&Value|usb["device"]["companionId"]==companion;
        // A recovered device can return on a different port while the old link
        // still holds diagnostic history. Prefer its current live observation.
        let Some(usb)=devices.iter().filter(matching).find(|usb|usb["connected"]==true && usb["ready"]==true)
            .or_else(||devices.iter().filter(matching).find(|usb|usb_recovery_message(usb).is_some())) else{continue};
        client["sessionConnected"]=client["connected"].clone();
        if let Some(message)=usb_recovery_message(usb){
            client["connectionStatus"]=json!(message);
            if usb["connected"]!=true || usb["ready"]!=true {continue;}
        }
        client["connected"]=json!(true);
        if let Some(profile)=usb_render_profile(usb){
            if client["renderProfileError"].is_null(){
                if !client["renderProfile"].is_null()&&client["renderProfile"]!=profile{
                    client["renderProfile"]=Value::Null;client["renderProfileError"]=json!("USB and authenticated connections report conflicting renderer profiles.");
                }else{client["renderProfile"]=profile;}
            }
        }else if !usb["device"]["renderProfile"].is_null() || (!client["renderProfile"].is_null()&&client["renderProfile"]["firmwareVersion"]!=usb["device"]["firmwareVersion"]){
            client["renderProfile"]=Value::Null;client["renderProfileError"]=json!("The USB renderer report does not match its firmware identity.");
        }
        if let Some(transports)=client["transports"].as_array_mut(){if !transports.contains(&json!("usb")){transports.push(json!("usb"));}}
        else{client["transports"]=json!(["usb"]);}
        client["connectionStatus"]=json!(usb_recovery_message(usb).unwrap_or_else(||usb["device"]["statusMessage"].as_str().unwrap_or_else(||match usb["device"]["status"].as_str(){
            Some("authenticating")=>"Connecting to Cockpit",
            Some("opening-panel")=>"Loading panel",
            Some("error")=>"Cockpit connection or session failed",
            Some("offline")=>"USB connected; panel session stopped",
            Some("awaiting-assignment")=>"No panel assigned",
            _=>"",
        })));
        if client["operation"].is_null() && usb["device"]["operation"].is_object()
            && assignments.iter().any(|assignment|assignment["companionId"]==client["companionId"]
                && assignment["instanceId"]==usb["device"]["operation"]["instanceId"]
                && assignment["publicationHash"]==usb["device"]["operation"]["publicationHash"]) {
            client["operation"]=usb["device"]["operation"].clone();
        }
        if usb["device"]["status"]=="panel-error"{
            client["panelError"]=json!(usb["device"]["statusMessage"].as_str().unwrap_or("The screen could not load or render this panel."));
        }
    }}
}
fn usb_recovery_message(usb:&Value)->Option<&str>{
    if !matches!(usb["recovery"]["phase"].as_str(),Some("waiting"|"connecting"|"waiting-for-device")){return None;}
    usb["recovery"]["message"].as_str().filter(|message|!message.is_empty()&&message.len()<=191&&!message.chars().any(char::is_control))
}
fn screen_dimensions(profile:&str)->Option<(u64,u64)>{
    match profile {"sunton-3248s035r"=>Some((480,320)),"waveshare-lcd7-s3"=>Some((800,480)),_=>None}
}
fn validate_screen_document(document:&Value,size:(u64,u64))->Result<(),String>{
    let widgets=document["presentation"]["widgets"].as_array().ok_or("Panel has no widgets.")?;
    if document["presentation"]["viewports"].as_array().is_some_and(|v|v.iter().any(|v|v.get("backgroundAssetId").is_some())) || widgets.iter().any(|w|w["config"].get("iconAssetId").is_some()){
        return Err("Use image widgets instead of backgrounds or button icons.".into());
    }
    if document["presentation"]["groups"].as_array().is_some_and(|g|!g.is_empty()) || widgets.iter().any(|w|w.get("groupId").is_some()){
        return Err("ESP32 panels do not support groups. Ungroup widgets before assigning.".into());
    }
    if widgets.len()>32 || serde_json::to_vec(document).map_err(|e|e.to_string())?.len()>16384{return Err("ESP32 panel exceeds the 32 widget or 16 KB document limit.".into());}
    let image_bytes:f64=widgets.iter().filter(|w|w["kind"]=="image").map(|w|{
        let screen=document["presentation"]["viewports"].as_array().and_then(|v|v.iter().find(|v|v["pageId"]==w["pageId"]));
        screen.map(|s|{let m=&s["safeInsets"];(w["bounds"]["width"].as_f64().unwrap_or(0.0)*size.0 as f64*(1.0-m["left"].as_f64().unwrap_or(0.0)-m["right"].as_f64().unwrap_or(0.0))).floor()*(w["bounds"]["height"].as_f64().unwrap_or(0.0)*size.1 as f64*(1.0-m["top"].as_f64().unwrap_or(0.0)-m["bottom"].as_f64().unwrap_or(0.0))).floor()*3.0}).unwrap_or(0.0)
    }).sum();
    if image_bytes>1200.0*1024.0{return Err("Image widgets exceed the 1200 KB device pixel budget.".into());}
    for w in widgets{
        if w["kind"]=="image" && w["config"]["assetId"].as_str().is_none(){return Err("Choose an asset for every image widget.".into());}
        let text_ok=|v:&Value|v.as_str().is_some_and(|s|s.bytes().all(|c|c.is_ascii_alphanumeric()||b" .,:;-_+=|/%!()?".contains(&c)));
        if !text_ok(&w["label"]) || ["prefix","suffix","unit"].iter().any(|key|w["config"].get(*key).is_some_and(|v|!text_ok(v))){return Err("ESP32 labels and text must use the supported device font.".into());}
        if !matches!(w["kind"].as_str(),Some("button"|"indicator"|"text"|"numeric"|"gauge"|"image")){return Err("This widget type is not supported on ESP32 screens.".into());}
        if w["kind"]=="button" && (w["config"]["buttonMode"]!="momentary" || w["config"]["pulseMs"]!=0 || w["config"]["repeatMs"]!=0){return Err("ESP32 buttons must be momentary without pulse or repeat.".into());}
        if w["kind"]=="button"{if let Some(screen)=document["presentation"]["viewports"].as_array().and_then(|screens|screens.iter().find(|v|v["pageId"]==w["pageId"])){
            let width=screen["width"].as_f64().unwrap_or(1.0);let height=screen["height"].as_f64().unwrap_or(1.0);
            let scale=(size.0 as f64/width).min((size.1 as f64)/height);
            let insets=&screen["safeInsets"];
            if (w["bounds"]["width"].as_f64().unwrap_or(0.0)*(1.0-insets["left"].as_f64().unwrap_or(0.0)-insets["right"].as_f64().unwrap_or(0.0))*width*scale).floor()<44.0 || (w["bounds"]["height"].as_f64().unwrap_or(0.0)*(1.0-insets["top"].as_f64().unwrap_or(0.0)-insets["bottom"].as_f64().unwrap_or(0.0))*height*scale).floor()<44.0{return Err("ESP32 buttons must render at least 44 by 44 pixels. Enlarge the button.".into());}
        }}
        if matches!(w["kind"].as_str(),Some("numeric"|"gauge")) && (w["config"]["precision"].as_u64().is_none_or(|p|p>4)||w["config"]["min"].as_f64().is_none_or(|n|n< -1e9)||w["config"]["max"].as_f64().is_none_or(|n|n>1e9)){return Err("ESP32 numeric precision must be 0-4 and ranges must stay within +/-1,000,000,000.".into());}
    }
    let pages=document["ui"]["pages"].as_array().ok_or("Panel has no pages.")?;
    if pages.len()>4{return Err("ESP32 panels support at most 4 pages.".into());}
    let screens=document["presentation"]["viewports"].as_array().ok_or("Panel has no screen size.")?;
    if pages.is_empty()||pages.iter().any(|page|!screens.iter().any(|screen|screen["pageId"]==page["id"]&&screen["width"].as_u64()==Some(size.0)&&screen["height"].as_u64()==Some(size.1))){
        return Err(format!("Every panel page must be {} × {} px to match this ESP32 screen.",size.0,size.1));
    }
    Ok(())
}
fn target_render_profile(client:&Value,firmware:&Value)->Result<Option<Value>,String>{
    if let Some(error)=client["renderProfileError"].as_str().filter(|error|!error.is_empty()){return Err(error.into());}
    if !client["renderProfile"].is_null(){return osg_companion_service::render_profile::validate(&client["renderProfile"]).map(Some);}
    if client["connected"]==true{return Ok(None);}
    if let Ok(profile)=osg_companion_service::render_profile::validate(&firmware["renderProfile"]){
        if profile["firmwareVersion"]==firmware["version"]&&profile["boardProfile"]==firmware["profile"]{return Ok(Some(profile));}
    }
    Ok(None)
}
fn validate_target_panel(hub:&Hub,hash:&str,client:&Value,firmware:&Value)->Result<(),String>{
    if hub.publication_document(hash)?["schemaVersion"]==4 {
        return if client["kind"]=="esp32" {Err("Compiled widgets are currently available for browser companions. ESP32 support is in the backlog.".into())} else {Ok(())};
    }
    if let Some(profile)=target_render_profile(client,firmware)?{
        if let Some(issue)=hub.publication_renderer_issues(hash,&profile)?.first(){return Err(issue.message.clone());}
        return Ok(());
    }
    if client["kind"]!="esp32"{return Ok(());}
    let size=client["screen"]["width"].as_u64().zip(client["screen"]["height"].as_u64())
        .or_else(||screen_dimensions(firmware["profile"].as_str().unwrap_or("")))
        .ok_or("Connect this ESP32 screen by USB to identify its screen size before assigning a panel.")?;
    validate_screen_document(&hub.publication_document(hash)?,size)
}
#[path = "profile_companion_authoring.rs"]
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
    saved_listener: Mutex<Option<ServerConfig>>,
    panel_cache: Mutex<()>,
}
struct Metadata {
    hub: Hub,
    directory: PathBuf,
    cache_warning: Option<String>,
}
pub struct Runtime {
    path: Option<PathBuf>,
    cutover_notice: Option<String>,
    cutover_error: Option<String>,
    stopped: AtomicBool,
    provider: Arc<CompanionProvider>,
    metadata: OnceLock<Metadata>,
    metadata_init: Mutex<()>,
    services: OnceLock<Result<Services, String>>,
    upload: Mutex<Option<Upload>>,
    #[cfg(test)]
    profile_wire_fixture: bool,
}
impl Runtime {
    pub fn new(path: Option<PathBuf>, provider: Arc<CompanionProvider>) -> Self {
        // Protected-store/TLS setup can require an OS credential service. Keep it
        // outside ordinary gateway startup; explicit companion operations and
        // captured-game recovery initialize the same retained state on demand.
        let (cutover_notice,cutover_error)=match path.as_ref().map(|directory|crate::widget_cutover::profiles(directory)){
            Some(Ok(notice))=>(notice,None),Some(Err(error))=>(None,Some(error)),None=>(None,None),
        };
        Self {
            path,
            cutover_notice,cutover_error,
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
            // Do not initialize protected storage for first-time users.
            if runtime.services.get().is_some()
                || runtime
                    .path
                    .as_ref()
                    .is_some_and(|p| p.join("companion-host-state.dpapi").is_file())
            {
                if let Ok(services) = runtime.services() {
                    if crate::companion_panels::ready(&services.hub).unwrap_or(false) {
                        Self::start_saved_listener(services);
                        let _ = services.usb.reconnect_detected(&services.hub);
                    }
                }
            }
            drop(runtime);
            std::thread::sleep(Duration::from_secs(2));
        });
    }
    fn metadata(&self, create: bool) -> Result<&Metadata, String> {
        if let Some(error)=&self.cutover_error{return Err(format!("Widget reset could not finish; companion output remains disabled: {error}"));}
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
                std::env::temp_dir().join(format!("osg-companion-test-{}", uuid::Uuid::new_v4())))
        } else { return Err("Set up and pair a companion before applying this assignment.".into()); };
        let cutover=crate::widget_cutover::panels(&hub,&directory)?;
        let restored=crate::companion_panels::restore_or_quarantine(&hub, &directory)?;
        let warnings=[self.cutover_notice.clone(),cutover,restored].into_iter().flatten().collect::<Vec<_>>();
        let cache_warning=(!warnings.is_empty()).then(||warnings.join(" "));
        let _ = self.metadata.set(Metadata {hub, directory, cache_warning});
        Ok(self.metadata.get().expect("metadata initialized under its gate"))
    }
    fn services(&self) -> Result<&Services, String> {
        self.services
            .get_or_init(|| {
                let metadata = self.metadata(true)?;
                let hub = metadata.hub.clone();
                let directory = metadata.directory.clone();
                let server = ServerRuntime::with_browser(hub.clone(), directory.clone(), BROWSER_ASSETS)?;
                if let Some(warning)=&metadata.cache_warning { server.record_error(warning.clone()); }
                let saved_listener=match std::fs::read(directory.join("companion-listener.json")){
                    Ok(bytes)=>match serde_json::from_slice::<ServerConfig>(&bytes){
                        Ok(config)=>Some(config),
                        Err(error)=>{server.record_error(format!("Saved companion listener settings are invalid: {error}"));None}
                    },
                    Err(_)=>None,
                };
                let services=Services {
                    hub, server, directory,
                    saved_listener:Mutex::new(saved_listener),
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
        if let Some(config)=services.saved_listener.lock().unwrap_or_else(|p|p.into_inner()).clone(){
            if let Err(error)=services.server.start(config){
                services.server.record_error(format!("Saved companion listener could not start: {error}"));
            }
        }
    }
    fn status(&self, services: &Services) -> Result<Value, String> {
        let interfaces: Vec<String> = osg_companion_service::server::local_interfaces()
            .ok()
            .and_then(|value| {
                value.as_array().map(|rows| {
                    rows.iter()
                        .filter_map(|row| row["ip"].as_str().map(str::to_owned))
                        .collect()
                })
            })
            .unwrap_or_default();
        let firmware_path=services.directory.join("companion-firmware.json");
        let mut firmware:Value=std::fs::read(&firmware_path).ok().filter(|b|b.len()<65536).and_then(|b|serde_json::from_slice(&b).ok()).filter(Value::is_object).unwrap_or(json!({}));
        let previous_firmware=firmware.clone();
        let mut host=services.hub.command(&json!({"command":"companion_status"}))?;
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
        if firmware!=previous_firmware{std::fs::write(&firmware_path,serde_json::to_vec(&firmware).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;}
        if let Some(metadata)=firmware.as_object_mut(){for info in metadata.values_mut(){
            if let Ok(profile)=osg_companion_service::render_profile::validate(&info["renderProfile"]){info["width"]=profile["screen"]["width"].clone();info["height"]=profile["screen"]["height"].clone();}
            else if let Some((width,height))=screen_dimensions(info["profile"].as_str().unwrap_or("")){info["width"]=json!(width);info["height"]=json!(height);}
        }}
        if let Some(clients)=host["clients"].as_array_mut(){for client in clients{if client["kind"]=="esp32"{
            if let Ok(profile)=osg_companion_service::render_profile::validate(&client["renderProfile"]){client["screen"]=profile["screen"].clone();}
            else if let Some(id)=client["companionId"].as_str(){if let Some(size)=screen_dimensions(firmware[id]["profile"].as_str().unwrap_or("")){client["screen"]=json!({"width":size.0,"height":size.1});}}
        }}}
        let mut listener=serde_json::to_value(services.server.status()).map_err(|e|e.to_string())?;
        let config=services.saved_listener.lock().unwrap_or_else(|p|p.into_inner()).clone();
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
        let metadata=self.metadata(false)?;
        crate::companion_panels::latest(&metadata.hub,&metadata.directory)
    }
    pub(crate) fn saved_panel_heads(&self) -> Result<std::collections::BTreeMap<String,String>,String> {
        let metadata=self.metadata(false)?;
        crate::companion_panels::saved_heads(&metadata.hub,&metadata.directory)
    }
    pub(crate) fn saved_panel_references(&self) -> Result<Vec<crate::companion_panels::SavedPanel>,String> {
        let heads=self.saved_panel_heads()?;
        Ok(self.saved_panel_updates()?.into_iter().filter(|saved|heads.get(&saved.panel_id)==Some(&saved.publication_hash)).collect())
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
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Missing companion command.")?;
        if command=="companion_game_capture" {
            return Err("Independent game companion capture is retired; edit the unified profile.".into());
        }
        let payload = request
            .get("companion")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !payload.is_object() {
            return Err("Companion operator payload must be an object.".into());
        }
        let owner = request["clientId"]
            .as_str()
            .ok_or("Missing local operator identity.")?;
        // Pause new ESP32 pairing while its replacement renderer is deferred.
        // Saved screens can still use their existing connection settings flow.
        if (command == "companion_pair_offer" && payload["kind"] == "esp32")
            || (command == "companion_usb_provision"
                && payload["companionId"].as_str().is_none_or(|id| id.trim().is_empty()))
        {
            return Err("ESP32 pairing is temporarily unavailable.".into());
        }
        match command {
            "companion_publish_begin" => {
                let expected = payload["size"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= MAX_UPLOAD as u64)
                    .ok_or("Panel upload must contain 1â€“13 MiB.")?
                    as usize;
                let mut slot = self.upload.lock().unwrap_or_else(|p| p.into_inner());
                if slot
                    .as_ref()
                    .is_some_and(|u| u.started.elapsed() < Duration::from_secs(60))
                {
                    return Err("Another panel upload is active; finish or abort it first.".into());
                }
                let id = uuid::Uuid::new_v4().to_string();
                *slot = Some(Upload {
                    id: id.clone(),
                    owner: owner.into(),
                    expected,
                    bytes: Vec::new(),
                    started: Instant::now(),
                });
                return Ok(json!({"uploadId":id}));
            }
            "companion_publish_abort" => {
                let mut slot = self.upload.lock().unwrap_or_else(|p| p.into_inner());
                if slot.as_ref().is_some_and(|u| {
                    u.owner == owner && payload["uploadId"].as_str() == Some(&u.id)
                }) {
                    *slot = None;
                }
                return Ok(json!({}));
            }
            "companion_publish_chunk" | "companion_publish_commit" => {
                let mut slot = self.upload.lock().unwrap_or_else(|p| p.into_inner());
                if slot
                    .as_ref()
                    .is_some_and(|u| u.started.elapsed() >= Duration::from_secs(60))
                {
                    *slot = None;
                    return Err("Panel upload expired; start a fresh upload.".into());
                }
                let upload = slot
                    .as_mut()
                    .filter(|u| u.owner == owner && payload["uploadId"].as_str() == Some(&u.id))
                    .ok_or("Panel upload does not belong to this operator.")?;
                if command.ends_with("_chunk") {
                    let bytes = payload["bytes"]
                        .as_array()
                        .filter(|a| !a.is_empty() && a.len() <= 8192)
                        .ok_or("Panel chunks contain 1â€“8192 bytes.")?;
                    if payload["offset"].as_u64() != Some(upload.bytes.len() as u64)
                        || upload.bytes.len() + bytes.len() > upload.expected
                    {
                        return Err("Panel chunk offset or length mismatch.".into());
                    }
                    let chunk = bytes
                        .iter()
                        .map(|v| {
                            v.as_u64()
                                .filter(|v| *v <= 255)
                                .map(|v| v as u8)
                                .ok_or("Invalid panel byte.")
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    upload.bytes.extend_from_slice(&chunk);
                    return Ok(json!({"received":upload.bytes.len()}));
                }
                if upload.bytes.len() != upload.expected {
                    return Err("Panel upload is incomplete.".into());
                }
                let bytes = slot.take().unwrap().bytes;
                drop(slot);
                let mut value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "Invalid panel upload JSON.")?;
                let object = value
                    .as_object_mut()
                    .ok_or("Panel upload must be an object.")?;
                if !["canonicalDocument", "publicationHash", "assets"]
                        .iter()
                        .all(|k| object.contains_key(*k))
                    || object.keys().any(|key| !["canonicalDocument", "publicationHash", "capabilityFingerprint", "assets"].contains(&key.as_str()))
                {
                    return Err("Unexpected panel upload fields.".into());
                }
                object.insert("command".into(), json!("companion_publish"));
                if payload.get("saveAsLatest").is_some_and(|value|!value.is_boolean()) {
                    return Err("saveAsLatest must be a boolean.".into());
                }
                let services = self.services()?;
                let _cache=services.panel_cache.lock().unwrap_or_else(|p|p.into_inner());
                services.hub.command(&value)?;
                crate::companion_panels::save(&services.hub,&services.directory)?;
                let saved=if payload["saveAsLatest"]!=false {
                    Some(crate::companion_panels::mark_saved(&services.hub,&services.directory,value["publicationHash"].as_str().ok_or("The saved panel has no content identity.")?)?)
                } else {None};
                Self::start_saved_listener(services);
                let mut response=self.status(services)?;
                if let Some(saved)=saved {response["panelSaved"]=serde_json::to_value(saved).map_err(|error|error.to_string())?;}
                return Ok(response);
            }
            _ => {}
        }
        let services = self.services()?;
        match command {
            "companion_firmware_check" => {
                services.firmware_releases.check();
                self.status(services)
            }
            "companion_usb_ports" => {
                let mut status=services.firmware_releases.metadata();
                status["usbPorts"]=services.usb.pairing_ports(&services.hub,payload["companionId"].as_str())?;
                services.firmware_releases.observe_ports(&status["usbPorts"],&services.usb.statuses());
                status["usb"]=services.usb.status();
                Ok(status)
            }
            "companion_usb_update_prepare" => {
                let _setup=services.usb_setup.lock().unwrap_or_else(|p|p.into_inner());
                let id=payload["companionId"].as_str().ok_or("Choose a paired companion.")?;
                let clients=services.hub.command(&json!({"command":"companion_status"}))?;
                if !clients["clients"].as_array().is_some_and(|rows|rows.iter().any(|c|c["companionId"]==id && c["kind"]=="esp32")){return Err("Choose a paired ESP32 companion.".into());}
                let ports=services.usb.pairing_ports(&services.hub,Some(id))?;
                services.firmware_releases.observe_ports(&ports,&services.usb.statuses());
                if ports.as_array().is_none_or(|rows|rows.len()!=1){return Err("Connect only this paired companion by USB, then try again.".into());}
                let metadata=std::fs::read(services.directory.join("companion-firmware.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).unwrap_or(json!({}));
                if !matches!(metadata[id]["profile"].as_str(),Some("sunton-3248s035r"|"waveshare-lcd7-s3")){return Err("This legacy companion has no saved board profile. Identify its board before updating firmware.".into());}
                services.usb.release_port(ports[0]["port"].as_str().ok_or("USB port is unavailable.")?);
                Ok(json!({"usbPorts":ports,"usb":services.usb.status()}))
            }
            "companion_usb_flash_cancel" => {
                services.usb.cancel_firmware_wait()?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_flash" => {
                let _setup = services.usb_setup.lock().unwrap_or_else(|p| p.into_inner());
                let (profile,target)=match payload["profile"].as_str() {
                    Some("waveshare-lcd7-s3")=>("waveshare-lcd7-s3",osg_companion_serial::FirmwareTarget::Esp32s3),
                    Some("sunton-3248s035r")=>("sunton-3248s035r",osg_companion_serial::FirmwareTarget::Esp32),
                    _=>return Err("Choose a supported board profile.".into()),
                };
                let port = payload["port"].as_str().ok_or("Choose a USB device.")?;
                let update=payload["companionId"].as_str();
                if let Some(id)=update{
                    let metadata=std::fs::read(services.directory.join("companion-firmware.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).unwrap_or(json!({}));
                    if metadata[id]["profile"].as_str().is_some_and(|profile|Some(profile)!=payload["profile"].as_str()){return Err("The firmware profile does not match this companion.".into());}
                }
                if !match update {Some(id)=>services.usb.update_candidate_available(port,id)?,None=>services.usb.flash_candidate_available(port)?}
                {
                    return Err("The USB selection changed or firmware detection is still in progress. Refresh devices and select this USB port again.".into());
                }
                let mut installed=Vec::new();
                if let Some(version)=services.firmware_releases.observed_version(port,profile){installed.push(version);}
                if let Some(id)=update {
                    let metadata=std::fs::read(services.directory.join("companion-firmware.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).unwrap_or(json!({}));
                    if let Some(version)=metadata[id]["version"].as_str(){installed.push(version.to_owned());}
                }
                let firmware=services.firmware_releases.load(&services.directory,profile,&installed)?;
                // Downloading must not bypass the serial manager's fresh device identity check.
                if !match update {Some(id)=>services.usb.update_candidate_available(port,id)?,None=>services.usb.flash_candidate_available(port)?} {
                    return Err("The USB selection changed while downloading firmware. Refresh devices and select this USB port again.".into());
                }
                services.usb.flash_owned_options_preserving(
                    payload["port"].as_str().ok_or("Choose a USB device.")?,
                    firmware, target, true, update.is_some(),
                )?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_status" => Ok(json!({"usb":services.usb.status()})),
            "companion_usb_connect" => {
                let port = payload["port"].as_str().ok_or("Select a USB device.")?;
                services.usb.attach(port, services.hub.clone())?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_disconnect" => {
                services.usb.release();
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_scan" => {
                services.usb.scan()?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_arm" => {
                services.usb.arm()?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_disarm" | "companion_usb_forget" | "companion_usb_reconnect" => {
                let op = match command {
                    "companion_usb_disarm" => "disarm",
                    "companion_usb_forget" => "forget",
                    _ => "connect",
                };
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| "System clock is invalid.")?
                    .as_millis() as u64;
                services.usb.record_for(payload["companionId"].as_str(),json!({"command":op,"unixMs":ms}))?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_provision" => {
                let _setup = services.usb_setup.lock().unwrap_or_else(|p| p.into_inner());
                if services.usb.status()["provisioning"] == true {
                    return Err("Device setup is already running.".into());
                }
                if services.usb.status()["ready"] != true {
                    return Err("Wait for the USB device to be ready.".into());
                }
                let usb_status=services.usb.status();
                let mut client=json!({"kind":"esp32","connected":true,"renderProfile":usb_render_profile(&usb_status)});
                if !usb_status["device"]["renderProfile"].is_null()&&client["renderProfile"].is_null(){client["renderProfileError"]=json!("The USB renderer report does not match its firmware identity.");}
                let firmware=json!({"version":usb_status["device"]["firmwareVersion"],"profile":usb_status["device"]["boardProfile"]});
                if let Some(hash)=payload["publicationHash"].as_str().filter(|hash|!hash.is_empty()){validate_target_panel(&services.hub,hash,&client,&firmware)?;}
                let name = payload["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 80)
                    .ok_or("Enter a device name.")?;
                let mode = payload["mode"]
                    .as_str()
                    .filter(|s| *s == "wifi" || *s == "serial")
                    .ok_or("Choose Wi-Fi or direct USB.")?;
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| "System clock is invalid.")?
                    .as_millis() as u64;
                let status = services.usb.status();
                let host = services
                    .hub
                    .command(&json!({"command":"companion_status"}))?;
                let existing = status["device"]["companionId"].as_str().filter(|id| {
                    host["clients"]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|c| c["companionId"] == *id && c["kind"] == "esp32"))
                });
                let id = if let Some(selected) = payload["companionId"].as_str() {
                    if existing != Some(selected) {
                        return Err("Connect the selected paired screen before changing its connection settings.".into());
                    }
                    selected.to_owned()
                } else {
                    if existing.is_some() {
                        return Err(
                            "This screen is already paired. Open its settings instead.".into()
                        );
                    }
                    uuid::Uuid::new_v4().to_string()
                };
                let offer = services
                    .hub
                    .command(&json!({"command":"companion_pair_offer","kind":"esp32"}))?;

                let mut record = if mode == "wifi" {
                    let ssid = payload["ssid"]
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 32)
                        .ok_or("Choose a Wi-Fi network.")?;
                    let password = payload["password"]
                        .as_str()
                        .filter(|s| s.len() <= 64)
                        .ok_or("Enter the Wi-Fi password.")?;
                    let pair = services.server.pairing_payload(
                        offer["secret"].as_str().ok_or("Missing offer.")?,
                        offer["expiresAtMs"]
                            .as_u64()
                            .ok_or("Missing offer expiry.")?,
                    )?;
                    json!({"command":"provision","ssid":ssid,"password":password,"keepWifiPassword":payload["keepWifiPassword"]==true,"pairing":pair,"unixMs":ms,"persist":true})
                } else {
                    json!({"command":"provision-direct","hostId":offer["hostId"],"secret":offer["secret"]})
                };
                record["name"] = json!(name);
                record["companionId"] = json!(id);
                if let Some(hash) = payload["publicationHash"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                {
                    record["_assignment"] = json!({"publicationHash":hash,"instanceId":uuid::Uuid::new_v4().to_string()});
                }
                services.usb.provision(record, id)?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_status" => self.status(services),
            "companion_enable" => {
                let address: IpAddr = payload["address"]
                    .as_str()
                    .ok_or("Choose a listen IP address.")?
                    .parse()
                    .map_err(|_| "Listen address must be an IP address.")?;
                let port = payload["port"]
                    .as_u64()
                    .filter(|p| (1024..=65535).contains(p))
                    .ok_or("Choose a port from 1024 to 65535.")? as u16;
                let advertised_host = payload["advertisedHost"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 253)
                    .ok_or("Choose the address companions can reach.")?
                    .to_owned();
                let config=ServerConfig {
                    bind: SocketAddr::new(address, port),
                    advertised_host,
                    discovery: payload["discovery"] == true,
                };
                if services.server.status().enabled{return Err("Companion listener is already enabled.".into());}
                if let Some(path)=&self.path{
                    std::fs::write(path.join("companion-listener.json"),serde_json::to_vec(&config).map_err(|e|e.to_string())?)
                        .map_err(|error|format!("Cannot save companion listener settings: {error}"))?;
                }
                *services.saved_listener.lock().unwrap_or_else(|p|p.into_inner())=Some(config);
                Self::start_saved_listener(services);
                self.status(services)
            }
            "companion_disable" => {
                *services.saved_listener.lock().unwrap_or_else(|p|p.into_inner())=None;
                if let Some(path)=&self.path {
                    let config=path.join("companion-listener.json");
                    if config.exists(){std::fs::remove_file(config).map_err(|e|e.to_string())?;}
                }
                services.server.stop();
                self.status(services)
            }
            "companion_browser_certificate" => Ok(json!({"certificatePem":services.server.browser_certificate()?})),
            "companion_pair_offer" => {
                if payload["kind"] == "browser" {
                    let browser = services.server.browser_url()?;
                    let offer=services.hub.command(&json!({"command":"companion_pair_offer","kind":"browser","autoApprove":true}))?;
                    let url=format!("{}#pair={}",browser,offer["secret"].as_str().ok_or("Missing pairing secret")?);
                    let mut state=self.status(services)?;
                    state["pairing"]=json!({"browserUrl":url,"ttlMs":60_000,"qrMatrix":[],"manualPayload":""});
                    return Ok(state);
                }
                let offer = services
                    .hub
                    .command(&json!({"command":"companion_pair_offer","kind":payload["kind"].as_str().unwrap_or("desktop"),"autoApprove":true}))?;
                let secret = offer["secret"]
                    .as_str()
                    .ok_or("Pairing offer did not contain a secret.")?;
                let expires = offer["expiresAtMs"]
                    .as_u64()
                    .ok_or("Pairing offer did not contain an expiry.")?;
                let payload = services.server.pairing_payload(secret, expires)?;
                let code = services
                    .hub
                    .command(&json!({"command":"companion_pair_code","payload":payload}))?;
                let manual = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
                // Dense binary rows preserve every QR module without a boolean
                // JSON array exceeding the bounded private-service reply frame.
                let matrix: Vec<String> = qrcode_matrix(&manual)?
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|dark| if dark { '1' } else { '0' })
                            .collect()
                    })
                    .collect();
                let mut state = self.status(services)?;
                state["pairing"] = json!({"manualPayload":manual,"qrMatrix":matrix,"ttlMs":60_000,"code":code["code"]});
                Ok(state)
            }
            "companion_pair_cancel"
            | "companion_feedback"
            | "companion_approve"
            | "companion_update"
            | "companion_revoke"
            | "companion_prune"
            | "companion_unassign"
            | "companion_assign"
            | "companion_disconnect"
            | "companion_takeover" => {
                if command=="companion_assign" {
                    let state=self.status(services)?;
                    if let Some(client)=state["host"]["clients"].as_array().and_then(|clients|clients.iter().find(|client|client["companionId"]==payload["companionId"])){
                        validate_target_panel(&services.hub,payload["publicationHash"].as_str().unwrap_or(""),client,&state["firmware"][payload["companionId"].as_str().unwrap_or("")])?;
                    }
                }
                let mut request = payload;
                request["command"] = json!(command);
                let _cache=services.panel_cache.lock().unwrap_or_else(|p|p.into_inner());
                services.hub.command(&request)?;
                if command=="companion_prune" || command=="companion_assign" {
                    crate::companion_panels::save(&services.hub,&services.directory)?;
                }
                self.status(services)
            }
            _ => Err("Unsupported companion operator command.".into()),
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
        if self.stopped.load(Ordering::SeqCst) { return Err("Companion service is stopped.".into()); }
        if let Some(metadata) = self.metadata.get() { return Ok(metadata.hub.resource_instance_ids()); }
        let Some(path) = &self.path else { return Ok(Default::default()); };
        if !path.join("companion-host-state.dpapi").try_exists().map_err(|error| error.to_string())? {
            return Ok(Default::default());
        }
        Ok(self.metadata(false)?.hub.resource_instance_ids())
    }
    pub(crate) fn resource_companions(&self) -> Result<Vec<osg_companion_service::ResourceCompanion>,String> {
        if self.stopped.load(Ordering::SeqCst) { return Err("Companion service is stopped.".into()); }
        if let Some(metadata)=self.metadata.get(){return Ok(metadata.hub.resource_companions());}
        let Some(path)=&self.path else{return Ok(vec![])};
        if !path.join("companion-host-state.dpapi").try_exists().map_err(|error|error.to_string())?{return Ok(vec![])}
        Ok(self.metadata(false)?.hub.resource_companions())
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
        assert!(runtime.metadata.set(Metadata {hub,directory,cache_warning:None}).is_ok());
        runtime.profile_wire_fixture = true;
        runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assignment_uses_reported_engines_without_reusing_an_offline_profile_for_a_live_legacy_device(){
        use sha2::{Digest,Sha256};
        let fixtures:Value=serde_json::from_str(include_str!("../../apps/companion-esp32/test/widget-compatibility.json")).unwrap();
        let canonical=fixtures["cases"][0]["canonicalDocument"].as_str().unwrap();let hash=format!("sha256:{:x}",Sha256::digest(canonical.as_bytes()));
        let hub=Hub::new("host".into(),Arc::new(osg_companion_service::NullSourceSink));
        crate::retain_companion_test_fixture(&hub,&json!({"canonicalDocument":canonical,"publicationHash":hash,"assets":[]})).unwrap();
        let reports:Value=serde_json::from_str(include_str!("../../apps/companion-esp32/test/render-profiles.json")).unwrap();let profile=reports["valid"][1].clone();
        let firmware=json!({"version":profile["firmwareVersion"],"profile":profile["boardProfile"],"renderProfile":profile});
        let mut client=json!({"kind":"esp32","connected":true,"screen":{"width":800,"height":480},"renderProfile":profile});
        assert!(validate_target_panel(&hub,&hash,&client,&firmware).is_ok());
        client["renderProfile"]=reports["valid"][0].clone();assert!(validate_target_panel(&hub,&hash,&client,&firmware).unwrap_err().contains("documentBytes"));
        client["renderProfile"]=Value::Null;assert!(validate_target_panel(&hub,&hash,&client,&firmware).is_err());
        client["connected"]=json!(false);assert!(validate_target_panel(&hub,&hash,&client,&firmware).is_ok());
        let mut stale=firmware.clone();stale["version"]=json!("older");assert!(validate_target_panel(&hub,&hash,&client,&stale).is_err());
        client["renderProfileError"]=json!("Conflicting renderer profiles.");assert_eq!(validate_target_panel(&hub,&hash,&client,&firmware).unwrap_err(),"Conflicting renderer profiles.");
    }
    #[test]
    fn reported_renderer_profiles_are_matched_to_current_usb_identity_and_conflicts_are_visible(){
        let fixture:Value=serde_json::from_str(include_str!("../../apps/companion-esp32/test/render-profiles.json")).unwrap();
        let profile=fixture["valid"][0].clone();
        let usb=json!({"connected":true,"ready":true,"device":{"companionId":"screen","firmwareVersion":profile["firmwareVersion"],"boardProfile":profile["boardProfile"],"renderProfile":profile}});
        let mut host=json!({"clients":[{"companionId":"screen","kind":"esp32","connected":false}],"instances":[]});
        describe_usb_presence(&mut host,&[usb.clone()]);assert_eq!(host["clients"][0]["renderProfile"],profile);
        host["clients"][0]["renderProfile"]["screen"]["width"]=json!(480);
        describe_usb_presence(&mut host,&[usb.clone()]);assert!(host["clients"][0]["renderProfile"].is_null()&&host["clients"][0]["renderProfileError"].is_string());
        let mut invalid=usb;invalid["device"]["firmwareVersion"]=json!("older");assert!(usb_render_profile(&invalid).is_none());
        invalid["ready"]=json!(false);assert!(usb_render_profile(&invalid).is_none());
    }
    #[test]
    fn pre_open_usb_diagnostics_require_current_assignment_identity_and_nonce_freshness() {
        let hash=format!("sha256:{}","a".repeat(64));
        let assignment=json!({"instanceId":"instance","publicationHash":hash,"companionId":"sunton"});
        let mut usb=json!({"connected":true,"ready":true,"statusAgeMs":100,"device":{"companionId":"sunton",
            "operation":{"instanceId":"instance","publicationHash":hash,"attemptId":2,"phase":"failed",
                "errorCode":"open-panel-rejected","retryable":true,"message":"Assigned panel was removed"}}});
        let report=usb_panel_operation(&assignment,&[usb.clone()]).unwrap();
        assert!(!report.live_session); assert!(report.fresh); assert_eq!(report.operation["errorCode"],"open-panel-rejected");
        usb["statusAgeMs"]=json!(1500);
        assert!(!usb_panel_operation(&assignment,&[usb.clone()]).unwrap().fresh,"Stale nonce status is retry context only");
        for (path,value) in [("connected",json!(false)),("ready",json!(false))] {
            let mut invalid=usb.clone(); invalid[path]=value;
            assert!(usb_panel_operation(&assignment,&[invalid]).is_none());
        }
        let mut other=usb.clone(); other["device"]["companionId"]=json!("waveshare");
        assert!(usb_panel_operation(&assignment,&[other]).is_none());
        let mut other=usb.clone(); other["device"]["operation"]["instanceId"]=json!("other-instance");
        assert!(usb_panel_operation(&assignment,&[other]).is_none());
        let mut other=usb.clone(); other["device"]["operation"]["publicationHash"]=json!(format!("sha256:{}","b".repeat(64)));
        assert!(usb_panel_operation(&assignment,&[other]).is_none());
        assert!(usb_panel_operation(&assignment,&[usb.clone(),usb]).is_none(),"Ambiguous USB identities cannot explain a switch");
    }
    #[test]
    fn usb_presence_is_independent_of_panel_session_errors(){
        let mut host=json!({"clients":[{"companionId":"screen","connected":false}]});
        describe_usb_presence(&mut host,&[json!({"connected":true,"ready":true,"device":{"companionId":"screen","status":"panel-error"},"deviceStatus":"panel-unsupported-corrupt-or-over-budget"})]);
        assert_eq!(host["clients"][0]["connected"],true);
        assert_eq!(host["clients"][0]["sessionConnected"],false);
        assert!(host["clients"][0]["panelError"].is_string());
        let mut offline=json!({"clients":[{"companionId":"screen","connected":false}]});
        describe_usb_presence(&mut offline,&[json!({"connected":false,"ready":true,"device":{"companionId":"screen"}})]);
        assert_eq!(offline["clients"][0]["connected"],false);
    }
    #[test]
    fn usb_status_preserves_the_reported_failure_without_guessing_unsupported_content(){
        for (status,message,panel_error) in [("panel-error","Panel image download failed",true),("error","Cockpit response timed out",false),("opening-panel","Downloading panel images",false)]{
            let mut host=json!({"clients":[{"companionId":"screen","connected":false}]});
            describe_usb_presence(&mut host,&[json!({"connected":true,"ready":true,"device":{"companionId":"screen","status":status,"statusMessage":message},"deviceStatus":"panel-unsupported-corrupt-or-over-budget","lastHostError":"Old ESP32 memory budget failure"})]);
            let client=&host["clients"][0];
            assert_eq!(client["connected"],true);
            assert_eq!(client["connectionStatus"],message);
            assert_eq!(client["panelError"].as_str(),if panel_error{Some(message)}else{None});
        }
    }
    #[test]
    fn usb_recovery_describes_only_the_exact_paired_companion_without_claiming_a_session(){
        for (phase,connected) in [("waiting",true),("connecting",true),("waiting-for-device",false)]{
            let message="USB connection interrupted; reconnecting automatically";
            let mut host=json!({"clients":[{"companionId":"screen","connected":false},{"companionId":"other","connected":false}]});
            describe_usb_presence(&mut host,&[json!({"connected":connected,"ready":connected,"recovery":{"phase":phase,"message":message},"device":{"companionId":"screen","status":"error","statusMessage":"Heartbeat: invalid USB reply"}})]);
            assert_eq!(host["clients"][0]["connectionStatus"],message);
            assert_eq!(host["clients"][0]["connected"],connected);
            assert_eq!(host["clients"][0]["sessionConnected"],false);
            assert!(host["clients"][1]["connectionStatus"].is_null());
            assert_eq!(host["clients"][1]["connected"],false);
        }
    }
    #[test]
    fn recovered_usb_presence_prefers_the_live_port_and_restores_normal_status(){
        let mut host=json!({"clients":[{"companionId":"screen","connected":true}]});
        let old=json!({"connected":false,"ready":false,"recovery":{"phase":"waiting-for-device","message":"Reconnecting automatically"},"device":{"companionId":"screen","status":"error"}});
        let recovered=json!({"connected":true,"ready":true,"recovery":{"phase":"recovered","message":"Recovered old connection"},"device":{"companionId":"screen","status":"live-armed","statusMessage":"Panel connected"}});
        describe_usb_presence(&mut host,&[old,recovered]);
        assert_eq!(host["clients"][0]["connected"],true);
        assert_eq!(host["clients"][0]["connectionStatus"],"Panel connected");
        for message in ["","Bad\nmessage"]{
            assert!(usb_recovery_message(&json!({"recovery":{"phase":"waiting","message":message}})).is_none());
        }
        assert!(usb_recovery_message(&json!({"recovery":{"phase":"waiting","message":"x".repeat(192)}})).is_none());
    }
    #[test]
    fn esp_panels_match_screen_dimensions_on_every_page() {
        assert_eq!(screen_dimensions("sunton-3248s035r"),Some((480,320)));
        assert_eq!(screen_dimensions("waveshare-lcd7-s3"),Some((800,480)));
        assert_eq!(screen_dimensions("unknown"),None);
        let mut document=json!({"ui":{"pages":[{"id":"a"}]},"presentation":{"widgets":[],"viewports":[{"pageId":"a","width":480,"height":320}]}});
        assert!(validate_screen_document(&document,(480,320)).is_ok());
        assert!(validate_screen_document(&document,(320,480)).is_err());
        document["ui"]["pages"].as_array_mut().unwrap().push(json!({"id":"b"}));
        assert!(validate_screen_document(&document,(480,320)).is_err());
    }
    fn runtime() -> Runtime {
        Runtime::new(None, Arc::new(CompanionProvider::new(Arc::new(|| 1))))
    }
    #[test]
    fn inventory_reads_preserve_absent_setup_and_report_stopped_or_corrupt_state() {
        let directory = crate::test_directory();
        for path in [None, Some(directory.path().join("absent"))] {
            let runtime = Runtime::new(path.clone(), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
            assert!(runtime.resource_instance_ids().unwrap().is_empty());
            assert!(runtime.resource_companions().unwrap().is_empty());
            assert!(runtime.resource_transport("missing").is_none());
            assert!(runtime.resource_publication("missing").is_err());
            assert!(runtime.profile_panel_operation("missing", "missing").is_none());
            assert!(runtime.profile_panel_operation_history("missing").is_none());
            assert!(runtime.profile_retry_panel("missing", "missing").is_ok());
            assert!(runtime.metadata.get().is_none());
            assert!(runtime.services.get().is_none());
            assert!(path.is_none_or(|path| !path.exists()));
            runtime.shutdown();
            assert!(runtime.resource_instance_ids().is_err());
            assert!(runtime.resource_companions().is_err());
        }
        std::fs::write(directory.path().join("companion-host-state.dpapi"), b"corrupt").unwrap();
        {
            let runtime = Runtime::new(Some(directory.path().into()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
            assert!(runtime.resource_instance_ids().is_err());
            assert!(runtime.resource_companions().is_err());
            assert!(runtime.services.get().is_none());
            assert_eq!(std::fs::read(directory.path().join("companion-host-state.dpapi")).unwrap(), b"corrupt");
        }
    }
    #[test]
    fn screen_validation_enforces_device_content_and_touch_limits() {
        let base = json!({"ui":{"pages":[{"id":"a"}]},"presentation":{
            "widgets":[{"kind":"button","label":"Test","pageId":"a","bounds":{"width":0.5,"height":0.5},
                "config":{"buttonMode":"momentary","pulseMs":0,"repeatMs":0}}],
            "viewports":[{"pageId":"a","width":480,"height":320}]}});
        assert!(validate_screen_document(&base, (480,320)).is_ok());
        for (pointer, value, message) in [
            ("/presentation/viewports/0/backgroundAssetId", json!("image"), "image widgets"),
            ("/presentation/groups", json!([{"id":"group"}]), "groups"),
            ("/presentation/widgets/0/config/iconAssetId", json!("icon"), "image widgets"),
            ("/presentation/widgets/0/groupId", json!("group"), "groups"),
            ("/presentation/widgets/0/label", json!("Unsupported 🚀"), "font"),
            ("/presentation/widgets/0/kind", json!("slider"), "widget type"),
            ("/presentation/widgets/0/config/buttonMode", json!("toggle"), "momentary"),
            ("/presentation/widgets/0/bounds/width", json!(0.01), "44 by 44"),
            ("/presentation/widgets/0/config/prefix", json!("🚀"), "font"),
            ("/presentation/widgets", json!([]), ""),
        ] {
            let mut document = base.clone();
            // Object keys may be absent in the valid document.
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            document.pointer_mut(parent).unwrap()[key] = value;
            if message.is_empty() { assert!(validate_screen_document(&document,(480,320)).is_ok()); }
            else { assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains(message)); }
        }
        for kind in ["numeric", "gauge", "image", "text", "indicator"] {
            let mut document = base.clone();
            let widget = &mut document["presentation"]["widgets"][0];
            widget["kind"] = json!(kind);
            widget["config"] = json!({"precision":4,"min":-1e9,"max":1e9,"assetId":"image"});
            assert!(validate_screen_document(&document,(480,320)).is_ok());
            if matches!(kind,"numeric"|"gauge") {
                document["presentation"]["widgets"][0]["config"]["precision"] = json!(5);
                assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("precision"));
            }
        }
        let mut document = base.clone();
        document["presentation"]["widgets"] = json!(vec![base["presentation"]["widgets"][0].clone();33]);
        assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("32 widget"));
        document["presentation"]["widgets"] = json!([{"kind":"image","label":"Image","pageId":"a","bounds":{"width":4,"height":4},"config":{"assetId":"image"}}]);
        assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("pixel budget"));
        document["presentation"]["widgets"][0]["bounds"] = json!({"width":0.5,"height":0.5});
        document["presentation"]["widgets"][0]["config"] = json!({});
        assert!(validate_screen_document(&document,(480,320)).unwrap_err().contains("Choose an asset"));
    }
    fn call(runtime: &Runtime, command: &str, payload: Value) -> Result<Value, String> {
        runtime.request(&json!({"command":command,"clientId":"operator-one","companion":payload}))
    }
    #[test]
    fn supplied_wire_fixture_acquisition_preserves_its_hub_without_native_services() {
        let directory = crate::test_directory();
        let transport_directory = directory.path().join("uninitialized-transports");
        let provider = Arc::new(CompanionProvider::new(Arc::new(|| 1)));
        let hub = Hub::new("already-connected-wire-fixture".into(), provider.clone());
        let runtime = Runtime::with_profile_metadata(
            hub.clone(), provider.clone(), transport_directory.clone(),
        );
        for _ in 0..2 {
            let acquired = runtime.profile_acquire_transports().unwrap();
            assert_eq!(acquired.host_id(), hub.host_id());
            assert!(Arc::ptr_eq(&runtime.profile_provider(), &provider));
            assert!(runtime.services.get().is_none());
            assert!(!transport_directory.exists());
        }
        runtime.shutdown();
        assert!(runtime.profile_acquire_transports().is_err());
        assert!(runtime.services.get().is_none());

        // The ordinary constructor still requires the real services admission.
        let ordinary = self::runtime();
        assert!(ordinary.services.set(Err("native transport admission required".into())).is_ok());
        assert_eq!(ordinary.profile_acquire_transports().err().unwrap(), "native transport admission required");
    }
    #[test]
    fn missing_profile_metadata_does_not_create_state_or_cache_a_failed_setup() {
        let directory = std::env::temp_dir().join(format!("osg-companion-metadata-{}", uuid::Uuid::new_v4()));
        let runtime = Runtime::new(Some(directory.clone()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
        assert!(runtime.profile_metadata().is_err());
        assert!(runtime.metadata.get().is_none());
        assert!(runtime.services.get().is_none());
        assert!(!directory.exists());
        assert!(Runtime::new(None, Arc::new(CompanionProvider::new(Arc::new(|| 1)))).profile_metadata().is_err());
        runtime.shutdown();
        assert!(runtime.profile_metadata().is_err());
    }
    #[cfg(windows)]
    #[test]
    fn retained_profile_metadata_restores_descriptors_without_listener_or_tls_setup() {
        let directory = std::env::temp_dir().join(format!("osg-companion-metadata-retained-{}", uuid::Uuid::new_v4()));
        let hub = Hub::open(&directory, Arc::new(osg_companion_service::NullSourceSink)).unwrap();
        let mut upload:Value = serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json")).unwrap();
        upload["command"] = json!("companion_publish");
        hub.command(&upload).unwrap();
        crate::companion_panels::save(&hub, &directory).unwrap();
        // A saved listener is shared setup. Merely inspecting a profile must not
        // construct a server or attempt to bind this saved endpoint.
        std::fs::write(directory.join("companion-listener.json"), b"invalid listener configuration").unwrap();
        let before = std::fs::read(directory.join("companion-host-state.dpapi")).unwrap();
        let runtime = Runtime::new(Some(directory.clone()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
        let metadata = runtime.profile_metadata().unwrap();
        assert_eq!(metadata.host_id(), hub.host_id());
        assert_eq!(metadata.publication_uploads().len(), 1);
        assert!(runtime.resource_instance_ids().unwrap().is_empty());
        assert!(runtime.resource_companions().unwrap().is_empty());
        assert!(runtime.resource_transport("missing").is_none());
        assert!(runtime.resource_publication("missing").is_err());
        assert!(runtime.profile_panel_operation("missing", "missing").is_none());
        assert!(runtime.profile_panel_operation_history("missing").is_none());
        let saved = runtime.saved_panel_updates().unwrap();
        assert_eq!(saved.len(), 1);
        assert!(runtime.saved_panel_heads().unwrap().is_empty());
        assert!(runtime.saved_panel_references().unwrap().is_empty(), "An archive alone is not a saved head");
        crate::companion_panels::mark_saved(&hub, &directory, upload["publicationHash"].as_str().unwrap()).unwrap();
        assert_eq!(runtime.saved_panel_heads().unwrap()[&saved[0].panel_id], saved[0].publication_hash);
        assert_eq!(runtime.saved_panel_references().unwrap()[0].publication_hash, saved[0].publication_hash);
        assert!(runtime.services.get().is_none());
        assert_eq!(std::fs::read(directory.join("companion-host-state.dpapi")).unwrap(), before);
        let mut names = std::fs::read_dir(&directory).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["companion-host-state.dpapi", "companion-listener.json", "companion-panel-heads.json", "companion-panels.json", "widget-panel-cutover-v4.json"]);
        runtime.shutdown();
        for name in names { std::fs::remove_file(directory.join(name)).unwrap(); }
        std::fs::remove_dir(directory).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn explicit_companion_request_reports_unreadable_retained_state_without_replacing_it() {
        let directory =
            std::env::temp_dir().join(format!("osg-companion-lazy-error-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("companion-host-state.dpapi");
        std::fs::write(&path, b"preserve-unreadable-state").unwrap();
        let runtime = Runtime::new(
            Some(directory.clone()),
            Arc::new(CompanionProvider::new(Arc::new(|| 1))),
        );
        assert!(runtime.services.get().is_none());
        assert!(call(&runtime, "companion_status", json!({})).is_err());
        assert!(matches!(runtime.services.get(), Some(Err(_))));
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve-unreadable-state");
        runtime.shutdown();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn ordinary_startup_does_not_initialize_or_modify_protected_companion_state() {
        let directory = std::env::temp_dir().join(format!(
            "osg-companion-lazy-startup-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("companion-host-state.dpapi");
        std::fs::write(&path, b"preserve-unavailable-protected-state").unwrap();
        let provider = Arc::new(CompanionProvider::new(Arc::new(|| 1)));
        let runtime = Runtime::new(Some(directory.clone()), provider);
        assert!(
            runtime.services.get().is_none(),
            "Unrelated gateway startup must not enter an OS credential service"
        );
        runtime.shutdown();
        assert!(runtime.services.get().is_none());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"preserve-unavailable-protected-state"
        );
        assert!(!directory.join("companion-identity.dpapi").exists());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn uploads_are_bounded_ordered_and_owned_without_initializing_the_listener() {
        let runtime = runtime();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":0})).is_err());
        assert!(call(
            &runtime,
            "companion_publish_begin",
            json!({"size":MAX_UPLOAD+1})
        )
        .is_err());
        let id = call(&runtime, "companion_publish_begin", json!({"size":3})).unwrap()["uploadId"]
            .clone();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":2})).is_err());
        assert!(runtime.request(&json!({"command":"companion_publish_chunk","clientId":"operator-two","companion":{"uploadId":id,"offset":0,"bytes":[1]}})).is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":1,"bytes":[1]})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[999]})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1,2,3,4]})
        )
        .is_err());
        assert!(call(&runtime, "companion_publish_commit", json!({"uploadId":id})).is_err());
        call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1,2,3]}),
        )
        .unwrap();
        assert!(call(&runtime, "companion_publish_commit", json!({"uploadId":id})).is_err());
        assert!(runtime.services.get().is_none());
        runtime.shutdown();
    }
    #[test]
    fn upload_abort_and_expiry_preserve_other_operator_ownership() {
        let runtime = runtime();
        let id = call(&runtime, "companion_publish_begin", json!({"size":1})).unwrap()["uploadId"]
            .clone();
        runtime.request(&json!({"command":"companion_publish_abort","clientId":"other","companion":{"uploadId":id}})).unwrap();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":1})).is_err());
        call(&runtime, "companion_publish_abort", json!({"uploadId":id})).unwrap();
        let id = call(&runtime, "companion_publish_begin", json!({"size":1})).unwrap()["uploadId"]
            .clone();
        runtime.upload.lock().unwrap().as_mut().unwrap().started =
            Instant::now() - Duration::from_secs(61);
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1]})
        )
        .unwrap_err()
        .contains("expired"));
        assert!(runtime.upload.lock().unwrap().is_none());
        assert!(runtime.request(&json!({})).is_err());
        assert!(runtime
            .request(&json!({"command":"companion_status","companion":false}))
            .is_err());
        assert!(runtime.services.get().is_none());
    }
    #[test]
    fn new_esp32_pairing_is_rejected_without_starting_services() {
        let runtime = runtime();
        for (command, payload) in [
            ("companion_pair_offer", json!({"kind":"esp32"})),
            ("companion_usb_provision", json!({})),
            ("companion_usb_provision", json!({"companionId":""})),
            ("companion_usb_provision", json!({"companionId":"  "})),
            ("companion_usb_provision", json!({"companionId":null})),
            ("companion_usb_provision", json!({"companionId":123})),
        ] {
            assert_eq!(call(&runtime, command, payload).unwrap_err(), "ESP32 pairing is temporarily unavailable.");
            assert!(runtime.services.get().is_none());
            assert!(runtime.metadata.get().is_none());
        }
    }
    #[cfg(windows)]
    #[test]
    fn usb_operator_requests_require_a_current_selection_and_ready_device() {
        let runtime = runtime();
        let before = call(&runtime, "companion_status", json!({})).unwrap();
        assert!(call(&runtime,"companion_usb_flash_cancel",json!({})).unwrap_err().contains("no update is waiting"));
        // With no paired clients this only enumerates ports; it cannot probe a UART.
        let ports = call(&runtime, "companion_usb_ports", json!({})).unwrap();
        assert!(ports["usbPorts"].is_array());
        assert_eq!(ports["firmwareProfiles"].as_array().unwrap().len(), 2);
        assert_eq!(ports["usb"]["connected"], false);
        for (payload, expected) in [
            (json!({}), "Choose a paired companion."),
            (json!({"companionId":"unpaired"}), "Choose a paired ESP32 companion."),
        ] {
            assert_eq!(call(&runtime, "companion_usb_update_prepare", payload).unwrap_err(), expected);
        }
        assert_eq!(
            call(&runtime, "companion_usb_flash", json!({"profile":"unknown"})).unwrap_err(),
            "Choose a supported board profile."
        );
        for profile in ["waveshare-lcd7-s3", "sunton-3248s035r"] {
            assert_eq!(
                call(&runtime, "companion_usb_flash", json!({"profile":profile})).unwrap_err(),
                "Choose a USB device."
            );
            // A path which was never enumerated cannot reach firmware installation,
            // including the update path with an unverified companion identity.
            let absent = format!("absent-test-port-{}", uuid::Uuid::new_v4());
            for companion in [Value::Null, json!("unpaired")] {
                assert!(call(&runtime, "companion_usb_flash", json!({
                    "profile":profile,"port":absent,"companionId":companion
                })).unwrap_err().contains("USB selection changed"));
            }
        }
        assert_eq!(
            call(&runtime, "companion_usb_connect", json!({})).unwrap_err(),
            "Select a USB device."
        );
        for command in [
            "companion_usb_scan", "companion_usb_arm", "companion_usb_disarm",
            "companion_usb_forget", "companion_usb_reconnect",
        ] {
            assert_eq!(call(&runtime, command, json!({})).unwrap_err(), "Connect a USB device first.");
        }
        assert_eq!(
            call(&runtime, "companion_usb_provision", json!({
                "companionId":"saved-screen","name":"Never provisioned","mode":"serial"
            })).unwrap_err(),
            "Wait for the USB device to be ready."
        );
        let disconnected = call(&runtime, "companion_usb_disconnect", json!({})).unwrap();
        assert_eq!(disconnected["usb"]["connected"], false);
        assert_eq!(call(&runtime, "companion_usb_status", json!({})).unwrap(), disconnected);
        let after = call(&runtime, "companion_status", json!({})).unwrap();
        for key in ["clients", "instances", "publications"] {
            assert_eq!(after["host"][key], before["host"][key]);
        }
        assert_eq!(after["listener"]["enabled"], false);
        assert_eq!(after["usb"]["devices"], json!([]));
        runtime.shutdown();
    }
    #[cfg(windows)]
    #[test]
    fn listener_configuration_restores_after_restart_and_disable_removes_it() {
        let directory=std::env::temp_dir().join(format!("osg-browser-listener-test-{}",uuid::Uuid::new_v4()));
        let make=|| {
            let mut runtime=runtime();runtime.path=Some(directory.clone());runtime
        };
        let first=make();
        let reservation=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port=reservation.local_addr().unwrap().port();drop(reservation);
        call(&first,"companion_enable",json!({"address":"127.0.0.1","advertisedHost":"127.0.0.1","port":port,"discovery":false})).unwrap();
        assert!(directory.join("companion-listener.json").is_file());
        first.shutdown();drop(first);
        let blocker=std::net::TcpListener::bind(("127.0.0.1",port)).unwrap();
        let second=make();
        let waiting=call(&second,"companion_status",json!({})).unwrap();
        assert_eq!(waiting["listener"]["enabled"],false);
        assert_eq!(waiting["listener"]["configuredEnabled"],true);
        assert_eq!(waiting["listener"]["config"]["discovery"],false);
        drop(blocker);
        Runtime::start_saved_listener(second.services().unwrap());
        assert_eq!(call(&second,"companion_status",json!({})).unwrap()["listener"]["enabled"],true);
        call(&second,"companion_disable",json!({})).unwrap();
        assert!(!directory.join("companion-listener.json").exists());
        second.shutdown();drop(second);
        let third=make();
        let disabled=call(&third,"companion_status",json!({})).unwrap();
        assert_eq!(disabled["listener"]["enabled"],false);
        assert_eq!(disabled["listener"]["configuredEnabled"],false);
        third.shutdown();drop(third);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn native_operator_status_pairing_publication_and_assignment_remain_separate_from_output() {
        let runtime = runtime();
        let initial = call(&runtime, "companion_status", json!({})).unwrap();
        assert_eq!(initial["listener"]["enabled"], false);
        assert!(initial["interfaces"].is_array());
        assert!(call(
            &runtime,
            "companion_enable",
            json!({"address":"invalid","port":54437,"advertisedHost":"127.0.0.1"})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_enable",
            json!({"address":"127.0.0.1","port":1,"advertisedHost":"127.0.0.1"})
        )
        .is_err());
        assert!(call(&runtime, "companion_pair_offer", json!({})).is_err());
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = occupied.local_addr().unwrap().port();
        let config = json!({"address":"127.0.0.1","port":port,"advertisedHost":"127.0.0.1","discovery":false});
        let waiting=call(&runtime,"companion_enable",config.clone()).unwrap();
        assert_eq!(waiting["listener"]["configuredEnabled"],true);
        assert_eq!(waiting["listener"]["enabled"],false);
        assert!(waiting["listener"]["lastError"].as_str().unwrap().contains("Cannot bind"));
        drop(occupied);
        assert_eq!(
            call(&runtime, "companion_enable", config.clone()).unwrap()["listener"]["enabled"],
            true
        );
        assert!(call(&runtime, "companion_enable", config).is_err());
        if !BROWSER_ASSETS.is_empty() {
            let browser = call(&runtime, "companion_pair_offer", json!({"kind":"browser"})).unwrap();
            assert!(browser["pairing"]["browserUrl"].as_str().unwrap().starts_with(&format!("https://127.0.0.1:{port}/#pair=")));
            assert_eq!(browser["pairing"]["ttlMs"],60_000);
        } else {
            assert!(call(&runtime,"companion_pair_offer",json!({"kind":"browser"})).unwrap_err().contains("Build the browser"));
        }
        assert!(call(&runtime,"companion_browser_certificate",json!({})).unwrap()["certificatePem"].as_str().unwrap().starts_with("-----BEGIN CERTIFICATE-----"));
        let offer = call(&runtime, "companion_pair_offer", json!({})).unwrap();
        let rows = offer["pairing"]["qrMatrix"].as_array().unwrap();
        assert!((21..=177).contains(&rows.len()));
        let original = qrcode_matrix(offer["pairing"]["manualPayload"].as_str().unwrap()).unwrap();
        for (row, expected) in rows.iter().zip(original) {
            let bits = row.as_str().unwrap();
            assert_eq!(bits.len(), rows.len());
            assert!(bits.bytes().all(|bit| bit == b'0' || bit == b'1'));
            assert_eq!(
                bits.bytes().map(|bit| bit == b'1').collect::<Vec<_>>(),
                expected
            );
        }
        // Pairing must fit both the native acceptance frame and browser bridge,
        // without increasing either transport's bound to accommodate QR data.
        assert!(
            serde_json::to_vec(&json!({"ok":true,"state":{"companions":offer}}))
                .unwrap()
                .len()
                < 66_561
        );
        let pairing: Value =
            serde_json::from_str(offer["pairing"]["manualPayload"].as_str().unwrap()).unwrap();
        assert!(pairing["endpoint"]
            .as_str()
            .unwrap()
            .starts_with("wss://127.0.0.1:"));
        let services = runtime.services().unwrap();
        assert_eq!(call(&runtime, "companion_pair_offer", json!({"kind":"esp32"})).unwrap_err(), "ESP32 pairing is temporarily unavailable.");
        assert_eq!(call(&runtime, "companion_usb_provision", json!({"mode":"serial"})).unwrap_err(), "ESP32 pairing is temporarily unavailable.");
        // Rejected ESP32 setup must leave the active non-ESP32 offer usable.
        let pending=services.hub.handle_wire(json!({"type":"pair","version":1,"secret":pairing["secret"],"companionId":"test-installation","name":"Desktop panel"}),"root-test-peer",services.hub.now_ms());
        assert_eq!(pending["type"], "pair-pending");
        call(
            &runtime,
            "companion_approve",
            json!({"requestId":pending["requestId"],"companionId":"test-installation"}),
        )
        .unwrap();

        let fixture: Value = serde_json::from_str(include_str!(
            "../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json"
        ))
        .unwrap();
        let upload = json!({"canonicalDocument":fixture["canonicalDocument"],"publicationHash":fixture["publicationHash"],"assets":fixture["assets"]});
        let bytes = serde_json::to_vec(&upload).unwrap();
        let upload_id = call(
            &runtime,
            "companion_publish_begin",
            json!({"size":bytes.len()}),
        )
        .unwrap()["uploadId"]
            .clone();
        for (index, chunk) in bytes.chunks(8192).enumerate() {
            call(
                &runtime,
                "companion_publish_chunk",
                json!({"uploadId":upload_id,"offset":index*8192,"bytes":chunk}),
            )
            .unwrap();
        }
        let published = call(
            &runtime,
            "companion_publish_commit",
            json!({"uploadId":upload_id}),
        )
        .unwrap();
        assert_eq!(
            published["host"]["publications"].as_array().unwrap().len(),
            1
        );
        let assigned=call(&runtime,"companion_assign",json!({"companionId":"test-installation","instanceId":"saved-panel","publicationHash":fixture["publicationHash"]})).unwrap();
        assert_eq!(assigned["host"]["instances"][0]["armed"], false);
        assert_eq!(runtime.hub().unwrap().host_id(), services.hub.host_id());
        assert!(runtime.resource_instance_ids().unwrap().contains("saved-panel"));
        assert_eq!(runtime.resource_companions().unwrap().len(), 1);
        assert_eq!(runtime.resource_publication("saved-panel").unwrap().publication_hash, fixture["publicationHash"]);
        assert!(runtime.profile_panel_operation("saved-panel", fixture["publicationHash"].as_str().unwrap()).is_none(),
            "An assignment without an opened session has no live operation");
        assert!(runtime.profile_panel_operation_history("saved-panel").is_none());
        assert!(runtime.profile_retry_panel("saved-panel", fixture["publicationHash"].as_str().unwrap()).is_ok());
        assert!(runtime.profile_retry_panel("saved-panel", "changed-publication").is_err());
        let legacy_screen = json!({"kind":"esp32","connected":true,"screen":{"width":480,"height":320}});
        assert!(validate_target_panel(&services.hub, "missing", &legacy_screen, &Value::Null).is_err());
        assert!(validate_target_panel(&services.hub, fixture["publicationHash"].as_str().unwrap(), &legacy_screen, &Value::Null).is_err(),
            "A published panel still needs to pass the target screen preflight");
        assert_eq!(
            runtime
                .assigned_publication("saved-panel")
                .unwrap()
                .publication_hash,
            fixture["publicationHash"].as_str().unwrap()
        );
        assert!(runtime.assigned_publication("unknown-instance").is_err());
        assert!(call(&runtime,"companion_game_capture",json!({"instanceIds":["saved-panel"]})).unwrap_err().contains("retired"));
        assert!(runtime.game_command("_game_companion_snapshot",&json!({})).is_err());
        assert!(runtime.game_command("_game_companion_take",&json!({})).is_err());
        assert!(runtime.game_command("_game_companion_unknown", &json!({})).is_err());
        assert!(call(&runtime, "companion_status", json!({})).unwrap()["host"]["instances"]
            .as_array().unwrap().iter().all(|instance| instance["armed"] == false));
        assert!(call(
            &runtime,
            "companion_approve",
            json!({"requestId":"missing"})
        )
        .is_err());
        call(
            &runtime,
            "companion_disconnect",
            json!({"instanceId":"saved-panel"}),
        )
        .unwrap();
        call(
            &runtime,
            "companion_takeover",
            json!({"instanceId":"saved-panel","companionId":"test-installation"}),
        )
        .unwrap();
        call(
            &runtime,
            "companion_revoke",
            json!({"companionId":"test-installation"}),
        )
        .unwrap();
        assert!(runtime.assigned_publication("saved-panel").is_err());
        assert!(call(&runtime, "companion_unsupported", json!({})).is_err());
        assert_eq!(
            call(&runtime, "companion_disable", json!({})).unwrap()["listener"]["enabled"],
            false
        );
        runtime.shutdown();
    }
}

#[path = "companion_game_owner.rs"]
pub(crate) mod game_owner;

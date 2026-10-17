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

include!(concat!(env!("OUT_DIR"), "/companion_firmware.rs"));

include!(concat!(env!("OUT_DIR"), "/companion_browser.rs"));

const MAX_UPLOAD: usize = 13 * 1024 * 1024;
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
    directory: PathBuf,
    saved_listener: Mutex<Option<ServerConfig>>,
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
}
impl Runtime {
    pub fn new(path: Option<PathBuf>, provider: Arc<CompanionProvider>) -> Self {
        // Protected-store/TLS setup can require an OS credential service. Keep it
        // outside ordinary gateway startup; explicit companion operations and
        // captured-game recovery initialize the same retained state on demand.
        Self {
            path,
            stopped: AtomicBool::new(false),
            provider,
            metadata: OnceLock::new(),
            metadata_init: Mutex::new(()),
            services: OnceLock::new(),
            upload: Mutex::new(None),
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
        crate::companion_panels::restore(&hub, &directory)?;
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
        let host=services.hub.command(&json!({"command":"companion_status"}))?;
        for usb in services.usb.statuses(){
        if let Some(id)=usb["device"]["companionId"].as_str().filter(|id|host["clients"].as_array().is_some_and(|rows|rows.iter().any(|c|c["companionId"]==*id))){
            let info=json!({"version":usb["device"]["firmwareVersion"].as_str().unwrap_or("unknown"),"profile":usb["device"]["boardProfile"].as_str().or_else(||firmware[id]["profile"].as_str())});
            if firmware[id]!=info{firmware[id]=info;std::fs::write(&firmware_path,serde_json::to_vec(&firmware).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;}
        }
        }
        let mut listener=serde_json::to_value(services.server.status()).map_err(|e|e.to_string())?;
        let config=services.saved_listener.lock().unwrap_or_else(|p|p.into_inner()).clone();
        listener["configuredEnabled"]=json!(config.is_some());
        listener["config"]=json!(config);
        Ok(
            json!({"firmware":firmware,"firmwareVersion":ESP32_FIRMWARE_VERSION,"host":services.hub.command(&json!({"command":"companion_status"}))?, "listener":listener,"browserUrl":services.server.browser_url().ok(), "interfaces":interfaces,"usb":services.usb.status()}),
        )
    }
    pub(crate) fn hub(&self) -> Result<Hub, String> {
        Ok(self.services()?.hub.clone())
    }
    /// A retained panel and paired installation are setup metadata, not a live
    /// transport, input baseline, or authority to change an assignment.
    pub(crate) fn profile_metadata(&self) -> Result<Hub, String> {
        Ok(self.metadata(false)?.hub.clone())
    }
    /// Call only from the journaled acquisition phase.
    pub(crate) fn profile_acquire_transports(&self) -> Result<Hub, String> {
        Ok(self.services()?.hub.clone())
    }
    pub(crate) fn profile_provider(&self) -> Arc<CompanionProvider> {
        self.provider.clone()
    }
    pub(crate) fn game_command(&self, command: &str, request: &Value) -> Result<Value, String> {
        let hub = self.hub()?;
        match command {
            "_game_companion_snapshot" => game_owner::snapshot(&hub, &request["capture"]),
            "_game_companion_take" => game_owner::take(&hub, &request["companions"]),
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
                if object.len() != 3
                    || !["canonicalDocument", "publicationHash", "assets"]
                        .iter()
                        .all(|k| object.contains_key(*k))
                {
                    return Err("Unexpected panel upload fields.".into());
                }
                object.insert("command".into(), json!("companion_publish"));
                let services = self.services()?;
                let _cache=services.panel_cache.lock().unwrap_or_else(|p|p.into_inner());
                services.hub.command(&value)?;
                crate::companion_panels::save(&services.hub,&services.directory)?;
                Self::start_saved_listener(services);
                return self.status(services);
            }
            _ => {}
        }
        let services = self.services()?;
        match command {
            "companion_usb_ports" => Ok(
                json!({"usbPorts":services.usb.pairing_ports(&services.hub,payload["companionId"].as_str())?,"usb":services.usb.status(),"firmwareProfiles":[{"id":"waveshare-lcd7-s3","name":"Waveshare ESP32-S3 Touch LCD 7 · Rev 1.2 · 800×480","available":!WAVESHARE_FIRMWARE.is_empty()},{"id":"sunton-3248s035r","name":"Sunton/GUITION ESP32-3248S035R · resistive · 480×320","available":!SUNTON_FIRMWARE.is_empty()}]}),
            ),
            "companion_usb_update_prepare" => {
                let _setup=services.usb_setup.lock().unwrap_or_else(|p|p.into_inner());
                let id=payload["companionId"].as_str().ok_or("Choose a paired companion.")?;
                let clients=services.hub.command(&json!({"command":"companion_status"}))?;
                if !clients["clients"].as_array().is_some_and(|rows|rows.iter().any(|c|c["companionId"]==id && c["kind"]=="esp32")){return Err("Choose a paired ESP32 companion.".into());}
                let ports=services.usb.pairing_ports(&services.hub,Some(id))?;
                if ports.as_array().is_none_or(|rows|rows.len()!=1){return Err("Connect only this paired companion by USB, then try again.".into());}
                let metadata=std::fs::read(services.directory.join("companion-firmware.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).unwrap_or(json!({}));
                if !matches!(metadata[id]["profile"].as_str(),Some("sunton-3248s035r"|"waveshare-lcd7-s3")){return Err("This legacy companion has no saved board profile. Identify its board before updating firmware.".into());}
                services.usb.release_port(ports[0]["port"].as_str().ok_or("USB port is unavailable.")?);
                Ok(json!({"usbPorts":ports,"usb":services.usb.status()}))
            }
            "companion_usb_flash" => {
                let _setup = services.usb_setup.lock().unwrap_or_else(|p| p.into_inner());
                let (firmware,target)=match payload["profile"].as_str() {
                    Some("waveshare-lcd7-s3")=>(WAVESHARE_FIRMWARE,osg_companion_serial::FirmwareTarget::Esp32s3),
                    Some("sunton-3248s035r")=>(SUNTON_FIRMWARE,osg_companion_serial::FirmwareTarget::Esp32),
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
                services.usb.flash_options_preserving(
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
                services.usb.record(json!({"command":op,"unixMs":ms}))?;
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
                let offer = services
                    .hub
                    .command(&json!({"command":"companion_pair_offer","kind":"esp32"}))?;
                let status = services.usb.status();
                let host = services
                    .hub
                    .command(&json!({"command":"companion_status"}))?;
                let existing = status["device"]["companionId"].as_str().filter(|id| {
                    host["clients"]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|c| c["companionId"] == *id))
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
            "companion_game_capture" => {
                Ok(json!({"capture":game_owner::capture(&services.hub,&payload)?}))
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
        let runtime=Self::new(None,provider);
        assert!(runtime.metadata.set(Metadata {hub,directory}).is_ok());
        runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime() -> Runtime {
        Runtime::new(None, Arc::new(CompanionProvider::new(Arc::new(|| 1))))
    }
    fn call(runtime: &Runtime, command: &str, payload: Value) -> Result<Value, String> {
        runtime.request(&json!({"command":command,"clientId":"operator-one","companion":payload}))
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
        let mut upload:Value = serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
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
        assert!(runtime.services.get().is_none());
        assert_eq!(std::fs::read(directory.join("companion-host-state.dpapi")).unwrap(), before);
        let mut names = std::fs::read_dir(&directory).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["companion-host-state.dpapi", "companion-listener.json", "companion-panels.json"]);
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
    #[cfg(windows)]
    #[test]
    fn usb_operator_requests_require_a_current_selection_and_ready_device() {
        let runtime = runtime();
        let before = call(&runtime, "companion_status", json!({})).unwrap();
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
                "name":"Never provisioned","mode":"serial"
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
        let pending=services.hub.handle_wire(json!({"type":"pair","version":1,"secret":pairing["secret"],"companionId":"test-installation","name":"Desktop panel"}),"root-test-peer",services.hub.now_ms());
        assert_eq!(pending["type"], "pair-pending");
        call(
            &runtime,
            "companion_approve",
            json!({"requestId":pending["requestId"],"companionId":"test-installation"}),
        )
        .unwrap();

        let fixture: Value = serde_json::from_str(include_str!(
            "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
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
        assert_eq!(
            runtime
                .assigned_publication("saved-panel")
                .unwrap()
                .publication_hash,
            fixture["publicationHash"].as_str().unwrap()
        );
        assert!(runtime.assigned_publication("unknown-instance").is_err());
        let mut capture = call(&runtime, "companion_game_capture", json!({"instanceIds":["saved-panel"]})).unwrap()["capture"].clone();
        capture["assignments"][0]["instanceId"] = json!("game-panel");
        let mut journal = runtime.game_command("_game_companion_snapshot", &json!({"capture":capture})).unwrap();
        assert_eq!(journal["taken"], false);
        assert!(runtime.assigned_publication("game-panel").is_err());
        assert!(runtime.game_command("_game_companion_take", &json!({"companions":journal})).is_err());
        journal["taken"] = json!(true);
        runtime.game_command("_game_companion_take", &json!({"companions":journal})).unwrap();
        assert_eq!(runtime.assigned_publication("game-panel").unwrap().publication_hash, fixture["publicationHash"]);
        runtime.game_command("_game_companion_restore", &json!({"companions":journal,"recovery":true})).unwrap();
        assert!(runtime.assigned_publication("game-panel").is_err());
        assert!(runtime.assigned_publication("saved-panel").is_ok());
        assert_eq!(runtime.game_command("_game_companion_abandon", &json!({"companions":journal})).unwrap(), json!({"abandoned":true}));
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

#[path = "companion_feedback.rs"]
pub(crate) mod feedback;
#[path = "companion_game_owner.rs"]
pub(crate) mod game_owner;

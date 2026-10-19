//! USB companion transport. Operator messages stay on the local side of Hub.
mod flash;
mod reconnect;
use base64::{engine::general_purpose::STANDARD, Engine};
pub use flash::*;
use osg_companion_service::Hub;
use serde_json::{json, Value};
use serialport::{SerialPort, SerialPortType};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroize;

pub fn ports() -> Result<Value, String> {
    let rows = serialport::available_ports().map_err(|_| "Cannot enumerate USB serial devices.")?;
    Ok(json!(rows.into_iter().filter_map(|p|match p.port_type{
        SerialPortType::UsbPort(info)=>Some(json!({"port":p.port_name,"name":info.product.unwrap_or_else(||"USB serial device".into()),"vid":info.vid,"pid":info.pid,"serial":info.serial_number})),
        _=>None
    }).collect::<Vec<_>>()))
}
fn assignment_hash(host:&Value,companion:&str)->Option<String>{
    host["instances"].as_array()?.iter().find(|a|a["companionId"]==companion)?
        ["publicationHash"].as_str().map(str::to_owned)
}
fn panel_retry_allowed(blocked:Option<&str>,current:Option<&str>)->bool{
    blocked.is_none() || blocked!=current
}
fn clear_recovered_error(state:&mut Value,status:Option<&str>){
    if matches!(status,Some("live-armed"|"live-disarmed"|"awaiting-assignment")){
        if let Some(state)=state.as_object_mut(){state.remove("lastHostError");state.remove("deviceStatus");}
    }
}
fn refresh_panel_recovery(state:&mut Value,device:&Value,blocked:&mut Option<String>){
    // A rejection applies only until this panel succeeds or its decoder changes.
    // Keeping it after recovery would suppress reconnects on a later USB fault.
    let recovered=matches!(device["status"].as_str(),Some("live-armed"|"live-disarmed"|"awaiting-assignment"));
    let updated=state["device"]["firmwareVersion"].as_str().zip(device["firmwareVersion"].as_str()).is_some_and(|(previous,current)|previous!=current);
    if recovered||updated{
        *blocked=None;
        if let Some(state)=state.as_object_mut(){state.remove("deviceStatus");}
    }
    clear_recovered_error(state,device["status"].as_str());
}
fn same_usb_candidate(candidate: &Value, current: &Value) -> bool {
    ["port", "vid", "pid", "serial"].iter().all(|key| candidate[key] == current[key])
}
enum Action {
    Record(Value),
    Provision(Value, String),
    Arm,
    Stop,
}
struct Link {
    port: String,
    tx: SyncSender<Action>,
    state: Arc<Mutex<Value>>,
    join: Option<thread::JoinHandle<()>>,
}
#[derive(Default)]
pub struct Manager {
    flash_state: Arc<Mutex<Value>>,
    flash_candidates: Mutex<Vec<Value>>,
    flash_owner: Mutex<Option<String>>,
    operation: Mutex<()>,
    auto_released: AtomicBool,
    detection: Mutex<reconnect::Detection>,
    links: Mutex<std::collections::BTreeMap<String,Link>>,
    selected: Mutex<Option<String>>,
}
impl Manager {
    pub fn status(&self) -> Value {
        let flashing = self
            .flash_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if flashing["flashing"] == true
            || flashing["flashFinished"] == true
            || flashing["flashError"].is_string()
        {
            return flashing;
        }
        let selected=self.selected.lock().unwrap_or_else(|p|p.into_inner()).clone();
        let devices=self.statuses();
        let mut status=devices.iter().find(|v|v["port"].as_str()==selected.as_deref()).or_else(||devices.first()).cloned()
            .unwrap_or(json!({"connected":false,"stage":"Choose a USB device"}));
        status["devices"]=json!(devices);status
    }
    pub fn statuses(&self)->Vec<Value>{
        self.links.lock().unwrap_or_else(|p|p.into_inner()).values().map(|link|link.state.lock().unwrap_or_else(|p|p.into_inner()).clone()).collect()
    }
    fn selected_link(&self)->Option<(String,Arc<Mutex<Value>>)>{
        let selected=self.selected.lock().unwrap_or_else(|p|p.into_inner()).clone()?;
        self.links.lock().unwrap_or_else(|p|p.into_inner()).get(&selected).map(|l|(l.port.clone(),l.state.clone()))
    }
    pub fn attach(&self, port: &str, hub: Hub) -> Result<(), String> {
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        self.auto_released.store(false, Ordering::SeqCst);
        self.attach_inner(port, hub, None)?;
        *self.selected.lock().unwrap_or_else(|p|p.into_inner())=Some(port.into());Ok(())
    }
    fn attach_inner(&self, port: &str, hub: Hub, expected: Option<String>) -> Result<(), String> {
        if self.flash_state.lock().unwrap_or_else(|p| p.into_inner())["flashing"] == true {
            return Err("Wait for firmware installation to finish.".into());
        }
        *self.flash_state.lock().unwrap_or_else(|p| p.into_inner()) = Value::Null;
        if self.links.lock().unwrap_or_else(|p|p.into_inner()).get(port)
            .is_some_and(|l|l.join.as_ref().is_some_and(|j|!j.is_finished())){return Ok(());}
        let valid = ports()?
            .as_array()
            .is_some_and(|rows| rows.iter().any(|p| p["port"].as_str() == Some(port)));
        if !valid {
            return Err("Choose a connected USB serial device.".into());
        }
        self.shutdown_port(port);
        // Once setup owns this port, normal unplug/replug discovery may use it.
        if expected.is_some(){
            self.flash_candidates.lock().unwrap_or_else(|p|p.into_inner()).retain(|candidate|candidate["port"]!=port);
        }
        let port_name = port.to_owned();
        let state = Arc::new(Mutex::new(
            json!({"connected":true,"stage":"Connecting to device","port":port}),
        ));
        let (tx, rx) = mpsc::sync_channel(8);
        let public = state.clone();
        let path = port.to_owned();
        let join = thread::spawn(move || {
            let result = worker(&path, &hub, &rx, &public, expected.as_deref());
            if result.is_err() {
                set(
                    &public,
                    "stage",
                    json!("USB connection failed. Reconnect the device and try again."),
                );
            }
            set(&public, "connected", json!(false));
            set(&public, "provisioning", json!(false));
        });
        self.links.lock().unwrap_or_else(|p|p.into_inner()).insert(port.into(),Link {
            port: port_name,
            tx,
            state,
            join: Some(join),
        });
        let mut selected=self.selected.lock().unwrap_or_else(|p|p.into_inner());
        if selected.is_none(){*selected=Some(port.into());}
        Ok(())
    }
    pub fn flash(&self, port: &str, firmware: &'static [FirmwareSegment]) -> Result<(), String> {
        self.flash_profile(port, firmware, FirmwareTarget::Esp32s3)
    }
    pub fn flash_profile(&self, port: &str, firmware: &'static [FirmwareSegment], target: FirmwareTarget) -> Result<(), String> {
        self.flash_options(port,firmware,target,false)
    }
    pub fn flash_options(&self, port: &str, firmware: &'static [FirmwareSegment], target: FirmwareTarget, manual_boot: bool) -> Result<(), String> {
        self.flash_options_preserving(port,firmware,target,manual_boot,false)
    }
    pub fn flash_options_preserving(&self,port:&str,firmware:&'static [FirmwareSegment],target:FirmwareTarget,manual_boot:bool,preserve:bool)->Result<(),String>{
        validate_firmware_for(target, firmware)?;
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        let mut state = self.flash_state.lock().unwrap_or_else(|p| p.into_inner());
        if state["flashing"] == true {
            return Err("Firmware installation is already running.".into());
        }
        if firmware.is_empty() {
            return Err("This Cockpit build does not include the selected board firmware.".into());
        }
        self.shutdown_port(port);
        *state =
            json!({"connected":false,"flashing":true,"stage":"Installing firmware","progress":0,"preserveSetup":preserve});
        let progress = self.flash_state.clone();
        let port = port.to_owned();
        thread::spawn(move || {
            let result = flash_firmware(&port, firmware, target, manual_boot, &progress);
            let mut state = progress.lock().unwrap_or_else(|p| p.into_inner());
            state["flashing"] = json!(false);
            match result {
                Ok(()) => {
                    state["flashFinished"] = json!(true);
                    state["stage"] = json!("Firmware installed");
                }
                Err(error) => {
                    if error.starts_with("bootloader-required:"){
                        state["flashWarning"]=json!("Could not connect to the bootloader. Hold BOOT, tap RESET while holding BOOT, release BOOT, then retry Install update. Keep the USB cable connected.");
                    }
                    state["flashError"] = json!(error.trim_start_matches("bootloader-required:"));
                    state["stage"] = json!("Firmware installation failed");
                }
            }
        });
        Ok(())
    }
    pub fn pairing_ports(&self, hub: &Hub, selected: Option<&str>) -> Result<Value, String> {
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        let paired = hub.command(&json!({"command":"companion_status"}))?["clients"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let statuses=self.statuses();
        let rows = ports()?.as_array().cloned().unwrap_or_default();
        let mut candidates = Vec::new();
        for row in rows {
            let port = row["port"].as_str().unwrap_or("");
            let status=statuses.iter().find(|v|v["port"]==port).cloned().unwrap_or(Value::Null);
            if status["connected"] == true
                && status["port"] == port
                && !status["device"]["companionId"].is_string()
            {
                // A read-only firmware check is not evidence of a paired device.
                // Retain only the exact previously enumerated unpaired candidate.
                if selected.is_none() && self.flash_owner.lock().unwrap_or_else(|p|p.into_inner()).is_none()
                    && self.flash_candidates.lock().unwrap_or_else(|p|p.into_inner()).iter().any(|candidate|same_usb_candidate(candidate,&row)){
                    candidates.push(row);
                }
                continue;
            }
            let identity = if status["connected"] == true && status["port"] == port {
                status["device"]["companionId"]
                    .as_str()
                    .filter(|id| paired.iter().any(|c| c["companionId"] == *id))
                    .map(str::to_owned)
            } else if !paired.is_empty()
                && [0x303a, 0x1a86, 0x10c4, 0x0403].contains(&row["vid"].as_u64().unwrap_or(0))
            {
                reconnect::inspect(port, &paired).ok().flatten()
            } else {
                None
            };
            if candidate_visible(identity.as_deref(), selected) {
                candidates.push(row);
            }
        }
        *self.flash_candidates.lock().unwrap_or_else(|p| p.into_inner()) = candidates.clone();
        *self.flash_owner.lock().unwrap_or_else(|p|p.into_inner())=selected.map(str::to_owned);
        Ok(json!(candidates))
    }
    /// Validate the previously inspected selection without opening its UART.
    /// Opening it again can disturb a manually entered ROM download session.
    pub fn flash_candidate_available(&self, port: &str) -> Result<bool, String> {
        let current = ports()?;
        let cached = self.flash_candidates.lock().unwrap_or_else(|p| p.into_inner());
        let status = self.status();
        if status["connected"] == true && status["port"] == port {
            return Ok(false);
        }
        Ok(current.as_array().is_some_and(|rows| rows.iter().any(|row|
            row["port"] == port && cached.iter().any(|candidate| same_usb_candidate(candidate, row)))))
    }
    pub fn update_candidate_available(&self, port:&str, companion:&str)->Result<bool,String>{
        if self.flash_owner.lock().unwrap_or_else(|p|p.into_inner()).as_deref()!=Some(companion){return Ok(false);}
        let current=ports()?;
        let cached=self.flash_candidates.lock().unwrap_or_else(|p|p.into_inner());
        Ok(current.as_array().is_some_and(|rows|rows.iter().any(|row|row["port"]==port && cached.iter().any(|candidate|same_usb_candidate(candidate,row)))))
    }
    /// Explicit release lasts until a manual connection or application restart.
    pub fn release(&self) {
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        self.auto_released.store(true, Ordering::SeqCst);
        let selected=self.selected.lock().unwrap_or_else(|p|p.into_inner()).clone();
        if let Some(port)=selected{self.shutdown_port(&port);}
    }
    pub fn release_port(&self,port:&str){
        let _operation=self.operation.lock().unwrap_or_else(|p|p.into_inner());
        self.auto_released.store(true,Ordering::SeqCst);self.shutdown_port(port);
    }
    /// Only a nonce-matched saved identity already paired with this Hub can reconnect.
    /// Detection never flashes, provisions, resets credentials, or assigns a panel.
    pub fn reconnect_detected(&self, hub: &Hub) -> Result<(), String> {
        let Ok(_operation) = self.operation.try_lock() else {
            return Ok(());
        };
        if self.auto_released.load(Ordering::SeqCst) {
            return Ok(());
        }
        let status = self.status();
        if status["flashing"] == true
            || status["flashFinished"] == true
            || status["flashError"].is_string()
        {
            return Ok(());
        }
        let paired = hub.command(&json!({"command":"companion_status"}))?["clients"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if paired.is_empty() {
            return Ok(());
        }
        let rows = ports()?.as_array().cloned().unwrap_or_default();
        let mut detection = self.detection.lock().unwrap_or_else(|p| p.into_inner());
        let candidates = self.flash_candidates.lock().unwrap_or_else(|p| p.into_inner());
        let active=self.statuses();
        let rows: Vec<_> = rows.into_iter().filter(|row|
            !active.iter().any(|link|link["connected"]==true && link["port"]==row["port"]) &&
            !candidates.iter().any(|candidate| same_usb_candidate(candidate, row))).collect();
        drop(candidates);
        for port in detection.candidates(&rows) {
            if let Ok(Some(id)) = reconnect::probe(&port, &paired) {
                self.attach_inner(&port, hub.clone(), Some(id))?;
            }
        }
        Ok(())
    }
    fn enqueue(&self, action: Action) -> Result<(), String> {
        let selected=self.selected.lock().unwrap_or_else(|p|p.into_inner()).clone().ok_or("Connect a USB device first.")?;
        let links=self.links.lock().unwrap_or_else(|p|p.into_inner());
        links.get(&selected).ok_or("Connect a USB device first.")?.tx.try_send(action).map_err(|_|"USB device is busy or disconnected.".into())
    }
    pub fn scan(&self)->Result<(),String>{
        if self.status()["scanning"]==true{return Err("A Wi-Fi scan is already running.".into());}
        self.record(json!({"command":"scan-wifi"}))?;
        if let Some((_,public))=self.selected_link(){
            let mut state=public.lock().unwrap_or_else(|p|p.into_inner());state["scanning"]=json!(true);state["scanError"]=Value::Null;
        }
        Ok(())
    }
    pub fn record(&self, v: Value) -> Result<(), String> {
        self.enqueue(Action::Record(v))
    }
    pub fn provision(&self, v: Value, companion: String) -> Result<(), String> {
        let result = self.enqueue(Action::Provision(v, companion));
        if result.is_ok() {
            let link=self.selected_link();
            if let Some((port,state))=link{
                self.flash_candidates.lock().unwrap_or_else(|p|p.into_inner()).retain(|candidate|candidate["port"]!=port);
                set(&state,"provisioning",json!(true));
            }
        }
        result
    }
    pub fn arm(&self) -> Result<(), String> {
        self.enqueue(Action::Arm)
    }
    fn shutdown_port(&self,port:&str){
        let link=self.links.lock().unwrap_or_else(|p|p.into_inner()).remove(port);
        if let Some(mut link)=link{
            let _=link.tx.try_send(Action::Stop);drop(link.tx);
            if let Some(join)=link.join.take(){let _=join.join();}
        }
    }
    pub fn shutdown(&self){
        let ports:Vec<_>=self.links.lock().unwrap_or_else(|p|p.into_inner()).keys().cloned().collect();
        for port in ports{self.shutdown_port(&port);}
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn candidate_visible(paired: Option<&str>, selected: Option<&str>) -> bool {
    match selected {
        Some(selected) => paired == Some(selected),
        None => paired.is_none(),
    }
}
#[cfg(test)]
mod candidate_tests {
    use super::*;
    #[test]
    fn pairing_excludes_known_devices_but_settings_only_lists_the_selected_device() {
        assert!(candidate_visible(None, None));
        assert!(!candidate_visible(Some("paired"), None));
        assert!(candidate_visible(Some("paired"), Some("paired")));
        assert!(!candidate_visible(Some("other"), Some("paired")));
        assert!(!candidate_visible(None, Some("paired")));
        // A removed identity is no longer classified as paired, so it is eligible again.
        assert!(candidate_visible(None, None));
    }
}
fn set(state: &Arc<Mutex<Value>>, key: &str, value: Value) {
    state.lock().unwrap_or_else(|p| p.into_inner())[key] = value;
}
fn send(port: &mut dyn SerialPort, v: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(v).map_err(|_| "Invalid serial record.")?;
    if bytes.len() > 8192 {
        bytes.zeroize();
        return Err("Serial record exceeds device budget.".into());
    }
    bytes.push(b'\n');
    let result = port
        .write_all(&bytes)
        .map_err(|_| "USB write failed.".to_string());
    bytes.zeroize();
    result
}
fn reply_frames(value: &Value) -> Result<Vec<Value>, String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Invalid reply.")?;
    if bytes.len() > 32768 {
        return Ok(vec![
            json!({"ok":false,"type":"error","error":"Panel exceeds ESP32 memory budget."}),
        ]);
    }
    if bytes.len() <= 4096 {
        return Ok(vec![value.clone()]);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
    let mut frames =
        vec![json!({"type":"transfer-start","id":id,"bytes":bytes.len(),"sha256":hash})];
    for (index, chunk) in bytes.chunks(2048).enumerate() {
        frames.push(
            json!({"type":"transfer-chunk","id":id,"index":index,"base64":STANDARD.encode(chunk)}),
        );
    }
    frames.push(json!({"type":"transfer-end","id":id}));
    Ok(frames)
}
struct Wire {
    link: String,
    connection: String,
    sequence: u64,
}
impl Wire {
    fn new() -> Self {
        Self {
            link: String::new(),
            connection: uuid::Uuid::new_v4().to_string(),
            sequence: 0,
        }
    }
    fn reset(&mut self, hub: &Hub, link: &str) {
        hub.disconnect_connection(&self.connection, hub.now_ms());
        self.connection = uuid::Uuid::new_v4().to_string();
        self.link = link.into();
        self.sequence = 0;
    }
    fn handle(&mut self, hub: &Hub, v: &Value) -> Result<Vec<Value>, String> {
        let link = v["linkId"].as_str().ok_or("Missing link identity.")?;
        if link != self.link || link.is_empty() {
            return Err("Stale USB link.".into());
        }
        let id = v["requestId"].as_str().ok_or("Missing request identity.")?;
        let seq = id.parse::<u64>().map_err(|_| "Invalid request sequence.")?;
        if seq != self.sequence + 1 || seq == u64::MAX || seq.to_string() != id {
            return Err("Out of order USB request.".into());
        }
        self.sequence = seq;
        let frame = v["frame"].clone();
        if !frame.is_object() {
            return Err("Invalid USB frame.".into());
        }
        let response = hub.handle_serial_wire(frame, &self.connection, hub.now_ms());
        Ok(reply_frames(&response)?.into_iter().map(|frame|json!({"command":"serial-reply","linkId":link,"requestId":id,"frame":frame})).collect())
    }
}
fn approve(hub: &Hub, id: &str) -> Result<bool, String> {
    let state = hub.command(&json!({"command":"companion_status"}))?;
    if let Some(p) = state["pending"].as_array().and_then(|a| {
        a.iter()
            .find(|p| p["companionId"].as_str() == Some(id) && p["approved"] != true)
    }) {
        hub.command(
            &json!({"command":"companion_approve","requestId":p["requestId"],"companionId":id}),
        )?;
        return Ok(true);
    }
    Ok(false)
}
fn worker(
    path: &str,
    hub: &Hub,
    rx: &Receiver<Action>,
    public: &Arc<Mutex<Value>>,
    expected: Option<&str>,
) -> Result<(), String> {
    let mut port = serialport::new(path, 115200)
        .timeout(Duration::from_millis(10))
        .open()
        .map_err(|_| "USB port unavailable.")?;
    let _ = port.write_data_terminal_ready(false);
    let _ = port.write_request_to_send(false);
    let mut wire = Wire::new();
    let result = (|| {
        let started = Instant::now();
        let mut baud_switched = Instant::now();
        let mut last_probe = Instant::now() - Duration::from_secs(1);
        let mut last_status = None;
        let mut scan_started:Option<Instant>=None;
        let mut last_connect: Option<Instant> = None;
        let mut blocked_panel:Option<String>=None;
        let mut ready = false;
        let mut nonce = String::new();
        let mut provision: Option<(String, Instant, Option<Value>)> = None;
        let mut line = Vec::new();
        let mut overflow = false;
        let mut bytes = [0u8; 512];
        loop {
            let _ = hub.tick(hub.now_ms());
            if scan_started.is_some_and(|at|at.elapsed()>Duration::from_secs(15)){
                scan_started=None;set(public,"scanning",json!(false));set(public,"scanError",json!("Wi-Fi scan timed out. Try again."));
                last_status=Some(Instant::now());
            }
            if ready && scan_started.is_none() && last_status.is_some_and(|at: Instant| at.elapsed() > Duration::from_secs(5))
            {
                return Err("USB companion stopped responding.".into());
            }
            if !ready && started.elapsed() > Duration::from_secs(45) {
                return Err("Device firmware did not respond.".into());
            }
            if !ready && baud_switched.elapsed() > Duration::from_secs(2) {
                let baud = if port.baud_rate().unwrap_or(115200) == 115200 { 921600 } else { 115200 };
                port.set_baud_rate(baud).map_err(|e| e.to_string())?;
                let _ = port.clear(serialport::ClearBuffer::Input);
                line.clear();
                overflow = false;
                baud_switched = Instant::now();
            }
            if last_probe.elapsed() > Duration::from_millis(if ready { 1000 } else { 500 }) {
                nonce = uuid::Uuid::new_v4().to_string();
                use std::io::Write;
                port.write_all(b"\n").map_err(|e|e.to_string())?;
                send(&mut *port, &json!({"command":"status","requestId":nonce}))?;
                last_probe = Instant::now();
            }
            // One local command per loop; no provisioning retries.
            match rx.try_recv() {
                Ok(Action::Stop) => {
                    if public.lock().unwrap_or_else(|p| p.into_inner())["device"]["connectionMode"]
                        == "serial"
                    {
                        let _ = send(&mut *port, &json!({"command":"stop"}));
                    }
                    return Ok(());
                }
                Ok(action) => {
                    if !ready {
                        set(
                            public,
                            "stage",
                            json!("Wait for the device to become ready"),
                        );
                        continue;
                    }
                    match action {
                        Action::Record(v) => {
                            if v["command"]=="scan-wifi"{
                                scan_started=Some(Instant::now());set(public,"scanning",json!(true));set(public,"scanError",Value::Null);set(public,"networks",json!([]));
                            }
                            send(&mut *port,&v)?;
                        },
                        Action::Provision(mut v, id) => {
                            if provision.is_some() {
                                set(public, "stage", json!("Provisioning is already running"));
                                continue;
                            }
                            let assignment =
                                v.as_object_mut().and_then(|v| v.remove("_assignment"));
                            send(&mut *port, &v)?;
                            provision = Some((id, Instant::now(), assignment));
                            set(public, "stage", json!("Provisioning device"));
                        }
                        Action::Arm => {
                            let status =
                                public.lock().unwrap_or_else(|p| p.into_inner())["device"].clone();
                            if status["readyForArm"] != true
                                || !last_status.is_some_and(|t: Instant| {
                                    t.elapsed() < Duration::from_millis(1500)
                                })
                            {
                                set(
                                    public,
                                    "stage",
                                    json!(
                                        "Release touch and wait for a healthy panel before arming"
                                    ),
                                );
                                continue;
                            }
                            send(
                                &mut *port,
                                &json!({"command":"arm","sessionId":status["sessionId"],"generation":status["generation"],"publicationHash":status["publicationHash"],"armEpoch":status["armEpoch"]}),
                            )?;
                        }
                        Action::Stop => unreachable!(),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if let Some((id, at, assignment)) = &provision {
                if approve(hub, id)? {
                    if let Some(a) = assignment {
                        hub.command(&json!({"command":"companion_assign","companionId":id,"instanceId":a["instanceId"],"publicationHash":a["publicationHash"]}))?;
                    }
                }
                if at.elapsed() > Duration::from_secs(60) {
                    set(
                        public,
                        "stage",
                        json!("Setup timed out. Check Wi-Fi and listener settings, then retry."),
                    );
                    provision = None;
                    set(public, "provisioning", json!(false));
                }
            }
            match port.read(&mut bytes) {
                Ok(n) => {
                    for &b in &bytes[..n] {
                        if b != b'\n' && b != b'\r' {
                            if line.len() < 8192 && !overflow {
                                line.push(b);
                            } else {
                                overflow = true;
                            }
                            continue;
                        }
                        if !overflow && !line.is_empty() {
                            if let Ok(v) = serde_json::from_slice::<Value>(&line) {
                                match v["type"].as_str() {
                                    Some("serial-open") => {
                                        let link = v["linkId"]
                                            .as_str()
                                            .filter(|s| s.len() <= 32 && s.starts_with("usb-"))
                                            .ok_or("Invalid USB link.")?;
                                        wire.reset(hub, link);
                                    }
                                    Some("serial-close") => {
                                        if v["linkId"].as_str() == Some(&wire.link) {
                                            hub.disconnect_connection(
                                                &wire.connection,
                                                hub.now_ms(),
                                            );
                                            wire.link.clear();
                                        }
                                    }
                                    Some("serial-request") => {
                                        set(public, "lastWireRequest", v["frame"]["type"].clone());
                                        for response in wire.handle(hub, &v)? {
                                            if response["frame"]["type"] == "error" {
                                                set(
                                                    public,
                                                    "lastHostError",
                                                    response["frame"]["error"].clone(),
                                                );
                                            }
                                            send(&mut *port, &response)?;
                                        }
                                    }
                                    _ => {
                                        if v["protocolVersion"] == 2
                                            && v["deviceType"] == "esp32"
                                            && v["requestId"].as_str() == Some(&nonce)
                                        {
                                            if !ready
                                                && expected.is_some_and(|id| {
                                                    v["companionId"] != id
                                                        || v["setupSaved"] != true
                                                        || v["connectionMode"] != "serial"
                                                })
                                            {
                                                return Err("Detected USB companion identity changed before reconnect.".into());
                                            }
                                            let host=hub.command(&json!({"command":"companion_status"}))?;
                                            let paired = host["clients"].as_array().is_some_and(|rows| rows.iter().any(|c|c["companionId"]==v["companionId"]));
                                            let assigned=assignment_hash(&host,v["companionId"].as_str().unwrap_or(""));
                                            refresh_panel_recovery(&mut public.lock().unwrap_or_else(|p|p.into_inner()),&v,&mut blocked_panel);
                                            let retry_allowed=panel_retry_allowed(blocked_panel.as_deref(),assigned.as_deref());
                                            if v["setupSaved"] == true && (!ready || (paired && retry_allowed && v["connectionMode"]=="serial" && matches!(v["status"].as_str(),Some("offline"|"error")) && last_connect.is_none_or(|at|at.elapsed()>Duration::from_secs(3)))) {
                                                last_connect=Some(Instant::now());
                                                let unix = std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .map_err(|_| "Invalid PC clock.")?
                                                    .as_millis()
                                                    as u64;
                                                send(
                                                    &mut *port,
                                                    &json!({"command":"connect","unixMs":unix}),
                                                )?;
                                            }
                                            ready = true;
                                            last_status = Some(Instant::now());
                                            set(public, "ready", json!(true));
                                            let mut safe = json!({});
                                            for key in [
                                                "firmwareVersion",
                                                "boardProfile",
                                                "status",
                                                "statusMessage",
                                                "name",
                                                "ssid",
                                                "wifiPasswordSaved",
                                                "connectionMode",
                                                "setupSaved",
                                                "companionId",
                                                "touchAvailable",
                                                "readyForArm",
                                                "sessionId",
                                                "generation",
                                                "publicationHash",
                                                "armEpoch",
                                            ] {
                                                if let Some(value) = v.get(key) {
                                                    safe[key] = value.clone();
                                                }
                                            }
                                            set(public, "device", safe);
                                            set(
                                                public,
                                                "stage",
                                                json!(v["statusMessage"].as_str().filter(|s| !s.is_empty() && s.len()<=191 && !s.chars().any(char::is_control)).unwrap_or(match v["status"].as_str() {
                                                    Some("live-armed") =>
                                                        "Panel connected · touch ready",
                                                    Some("live-disarmed") =>
                                                        "Waiting for the display and released touch",
                                                    Some("awaiting-assignment") =>
                                                        "Paired · assign a panel in Cockpit",
                                                    Some("error") =>
                                                        "Connection failed · check device status",
                                                    Some("pairing" | "awaiting-approval") =>
                                                        "Pairing device",
                                                    Some("authenticating" | "opening-panel") =>
                                                        "Connecting panel",
                                                    Some("panel-error") => "Panel could not load or render",
                                                    _ => "Device ready",
                                                })),
                                            );
                                            if (v["setupSaved"] == true || v["status"] == "error")
                                                && provision.as_ref().is_some_and(|p| {
                                                    v["companionId"].as_str() == Some(&p.0)
                                                })
                                            {
                                                provision = None;
                                                set(public, "provisioning", json!(false));
                                            }
                                        } else if v["status"] == "wifi-networks" {
                                            scan_started=None;last_status=Some(Instant::now());
                                            set(public,"scanning",json!(false));set(public,"scanError",Value::Null);
                                            set(public, "networks", v["networks"].clone());
                                            set(public, "stage", json!("Choose a Wi-Fi network"));
                                        } else if v["status"]=="wifi-scan-failed"{
                                            scan_started=None;last_status=Some(Instant::now());set(public,"scanning",json!(false));
                                            set(public,"scanError",json!("The screen could not scan Wi-Fi networks. Try again or enter the network name."));
                                        } else if let Some(status) = v["status"].as_str() {
                                            if matches!(status,"panel-unsupported-corrupt-or-over-budget"|"panel-load-failed"|"panel-render-failed") {
                                                let device=public.lock().unwrap_or_else(|p|p.into_inner())["device"].clone();
                                                let host=hub.command(&json!({"command":"companion_status"}))?;
                                                blocked_panel=assignment_hash(&host,device["companionId"].as_str().unwrap_or(""));
                                            }
                                            if status.len() < 96
                                                && status
                                                    .bytes()
                                                    .all(|b| b.is_ascii_lowercase() || b == b'-')
                                            {
                                                set(public, "deviceStatus", json!(status));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        line.zeroize();
                        line.clear();
                        overflow = false;
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(_) => return Err("USB disconnected.".into()),
            }
        }
    })();
    hub.disconnect_connection(&wire.connection, hub.now_ms());
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovered_panel_and_updated_firmware_release_stale_rejection(){
        for status in ["live-armed","live-disarmed","awaiting-assignment"]{
            let mut state=json!({"device":{"firmwareVersion":"old"},"deviceStatus":"panel-unsupported-corrupt-or-over-budget","lastHostError":"Old error"});
            let mut blocked=Some("panel-a".to_owned());
            refresh_panel_recovery(&mut state,&json!({"status":status,"firmwareVersion":"old"}),&mut blocked);
            assert!(panel_retry_allowed(blocked.as_deref(),Some("panel-a")));
            assert!(state.get("deviceStatus").is_none());
            assert!(state.get("lastHostError").is_none());
        }
        let mut state=json!({"device":{"firmwareVersion":"old"},"deviceStatus":"panel-unsupported-corrupt-or-over-budget"});
        let mut blocked=Some("panel-a".to_owned());
        refresh_panel_recovery(&mut state,&json!({"status":"error","firmwareVersion":"old"}),&mut blocked);
        assert!(!panel_retry_allowed(blocked.as_deref(),Some("panel-a")));
        refresh_panel_recovery(&mut state,&json!({"status":"error","firmwareVersion":"new"}),&mut blocked);
        assert!(panel_retry_allowed(blocked.as_deref(),Some("panel-a")));
        assert!(state.get("deviceStatus").is_none());
    }
    #[test]
    fn unsupported_panel_waits_for_an_assignment_change_before_retry(){
        assert!(panel_retry_allowed(None,Some("panel-a")));
        assert!(!panel_retry_allowed(Some("panel-a"),Some("panel-a")));
        assert!(panel_retry_allowed(Some("panel-a"),Some("panel-b")));
        assert!(panel_retry_allowed(Some("panel-a"),None));
    }
    #[test]
    fn host_errors_clear_only_after_panel_recovery(){
        for status in ["error","offline","authenticating","opening-panel"]{
            let mut state=json!({"lastHostError":"Panel missing"});
            clear_recovered_error(&mut state,Some(status));
            assert!(state["lastHostError"].is_string());
        }
        for status in ["live-armed","live-disarmed","awaiting-assignment"]{
            let mut state=json!({"lastHostError":"Panel missing","connected":true});
            clear_recovered_error(&mut state,Some(status));
            assert!(state.get("lastHostError").is_none());
            assert_eq!(state["connected"],true);
        }
    }
    #[test]
    fn serial_panels_keep_independent_workers_and_targeted_release(){
        let manager=Manager::default();
        let (tx5,rx5)=mpsc::sync_channel(8);let (tx6,rx6)=mpsc::sync_channel(8);
        for (port,tx) in [("COM5",tx5),("COM6",tx6)]{
            manager.links.lock().unwrap().insert(port.into(),Link{port:port.into(),tx,state:Arc::new(Mutex::new(json!({"port":port,"connected":true}))),join:None});
        }
        *manager.selected.lock().unwrap()=Some("COM5".into());
        manager.record(json!({"command":"status"})).unwrap();
        assert!(matches!(rx5.try_recv(),Ok(Action::Record(_))));assert!(rx6.try_recv().is_err());
        *manager.selected.lock().unwrap()=Some("COM6".into());
        manager.record(json!({"command":"scan-wifi"})).unwrap();
        assert!(matches!(rx6.try_recv(),Ok(Action::Record(_))));assert!(rx5.try_recv().is_err());
        assert_eq!(manager.status()["devices"].as_array().unwrap().len(),2);
        manager.release_port("COM6");
        assert!(matches!(rx6.try_recv(),Ok(Action::Stop)));assert!(rx5.try_recv().is_err());
        assert_eq!(manager.statuses().len(),1);assert_eq!(manager.statuses()[0]["port"],"COM5");
        manager.shutdown();assert!(matches!(rx5.try_recv(),Ok(Action::Stop)));
    }
    #[test]
    fn flash_selection_requires_the_same_enumerated_usb_device() {
        let original=json!({"port":"COM6","vid":0x1a86,"pid":0x7523,"serial":"screen"});
        assert!(same_usb_candidate(&original,&original));
        for (key,value) in [("port",json!("COM7")),("vid",json!(0x303a)),("pid",json!(1)),("serial",json!("other"))] {
            let mut changed=original.clone();
            changed[key]=value;
            assert!(!same_usb_candidate(&original,&changed));
        }
    }
    #[test]
    fn flash_errors_remain_available_for_the_installer_until_an_explicit_retry() {
        let manager=Manager::default();
        let hub=Hub::new("test-host".into(),Arc::new(osg_companion_service::NullSourceSink));
        *manager.flash_state.lock().unwrap()=json!({"connected":false,"flashing":false,"flashError":"Download mode required","stage":"Firmware installation failed"});
        manager.reconnect_detected(&hub).unwrap();
        assert_eq!(manager.status()["flashError"],"Download mode required");
    }

    #[test]
    fn chunks_are_bounded_and_digest_matches() {
        let value = json!({"ok":true,"data":"x".repeat(16000)});
        let frames = reply_frames(&value).unwrap();
        assert_eq!(frames.first().unwrap()["type"], "transfer-start");
        let bytes: Vec<u8> = frames
            .iter()
            .filter(|v| v["type"] == "transfer-chunk")
            .flat_map(|v| STANDARD.decode(v["base64"].as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), value);
        assert_eq!(
            frames[0]["sha256"],
            format!("sha256:{:x}", Sha256::digest(&bytes))
        );
        for frame in frames {
            assert!(serde_json::to_vec(&frame).unwrap().len() < 4096);
        }
    }
    #[test]
    fn oversize_panel_returns_an_error() {
        assert_eq!(
            reply_frames(&json!({"data":"x".repeat(40000)})).unwrap()[0]["ok"],
            false
        );
    }
    #[test]
    fn pairing_is_bound_to_usb_identity_and_sequences() {
        let hub = Hub::new(
            "test-host".into(),
            Arc::new(osg_companion_service::NullSourceSink),
        );
        let offer = hub
            .command(&json!({"command":"companion_pair_offer"}))
            .unwrap();
        let mut wire = Wire::new();
        wire.reset(&hub, "usb-test");
        let pair = json!({"type":"serial-request","linkId":"usb-test","requestId":"1","frame":{"type":"pair","secret":offer["secret"],"companionId":"test-screen","name":"Test screen"}});
        let pending = wire.handle(&hub, &pair).unwrap()[0]["frame"].clone();
        assert_eq!(pending["type"], "pair-pending");
        assert!(!approve(&hub, "different-screen").unwrap());
        assert!(approve(&hub, "test-screen").unwrap());
        let poll = json!({"type":"serial-request","linkId":"usb-test","requestId":"2","frame":{"type":"pair-status","requestId":pending["requestId"],"secret":offer["secret"]}});
        let paired = wire.handle(&hub, &poll).unwrap()[0]["frame"].clone();
        assert_eq!(paired["type"], "paired");
        let auth = json!({"type":"serial-request","linkId":"usb-test","requestId":"3","frame":{"type":"auth","token":paired["token"]}});
        assert_eq!(
            wire.handle(&hub, &auth).unwrap()[0]["frame"]["type"],
            "authenticated"
        );
        assert!(wire.handle(&hub, &auth).is_err());
        let old = wire.connection.clone();
        wire.reset(&hub, "usb-new");
        assert_ne!(wire.connection, old);
        assert!(wire.handle(&hub, &auth).is_err());
        let mut fresh = auth;
        fresh["linkId"] = json!("usb-new");
        fresh["requestId"] = json!("1");
        assert_eq!(
            wire.handle(&hub, &fresh).unwrap()[0]["frame"]["type"],
            "authenticated"
        );
    }
}

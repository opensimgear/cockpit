//! One retained standalone-USB worker per observed endpoint. Enumeration and
//! metadata reads cannot claim a writer; operator changes are explicit requests.
use crate::module_usb::{
    self, ActionRequest, ConfigChange, ModuleUsbService, UsbRequest, UsbRouteToken, UsbSnapshot,
};
use crate::transport::PortInfo;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

pub(crate) type SerialPorts = Arc<dyn Fn() -> Result<Vec<PortInfo>, String> + Send + Sync>;
const MAX_PRESENT: usize = 32;
const MAX_RETAINED: usize = 128;
const MAX_REPLY_BYTES: usize = 110_000;
trait Endpoint: Send + Sync {
    fn snapshot(&self) -> UsbSnapshot;
    fn request(&self, request: UsbRequest) -> Result<UsbSnapshot, String>;
    fn shutdown(&self);
}
impl Endpoint for ModuleUsbService {
    fn snapshot(&self) -> UsbSnapshot {
        self.snapshot()
    }
    fn request(&self, request: UsbRequest) -> Result<UsbSnapshot, String> {
        self.request(request)
    }
    fn shutdown(&self) {
        self.shutdown()
    }
}
type Create = Arc<dyn Fn() -> Arc<dyn Endpoint> + Send + Sync>;
struct Slot {
    port: PortInfo,
    present: bool,
    automatic: bool,
    ambiguous: bool,
    suppressed: bool,
    service: Option<Arc<dyn Endpoint>>,
    cached: UsbSnapshot,
    retiring_service: Option<Arc<dyn Endpoint>>,
    busy: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
}
struct State {
    slots: BTreeMap<String, Slot>,
    diagnostic: Option<String>,
    /// A successful role/UID handshake permits reconnecting this selected MCU;
    /// generic port metadata alone never enters this set.
    selected: BTreeMap<String, String>,
}
pub(crate) struct Fleet {
    serial: SerialPorts,
    native: bool,
    create: Create,
    state: Mutex<State>,
    discovery: Mutex<()>,
    closed: AtomicBool,
}
#[derive(Clone, Debug)]
pub(crate) struct SlotSnapshot {
    pub key: String,
    pub port: PortInfo,
    pub present: bool,
    pub automatic: bool,
    pub admissible: bool,
    pub snapshot: UsbSnapshot,
    pub diagnostics: Vec<String>,
}
impl SlotSnapshot {
    pub(crate) fn value(&self) -> Value {
        json!({"key":self.key,"port":self.port.value(),"present":self.present,"automatic":self.automatic,"admissible":self.admissible,"snapshot":self.snapshot,"diagnostics":self.diagnostics})
    }
    fn summary_value(&self) -> Value {
        let snapshot = &self.snapshot;
        json!({"key":self.key,"port":self.port.value(),"present":self.present,"automatic":self.automatic,"admissible":self.admissible,"snapshot":{"connected":snapshot.connected,"port":snapshot.port,"identity":snapshot.identity,"route":snapshot.route,"claimedBy":snapshot.claimed_by,"observedAtMs":snapshot.observed_at_ms,"error":snapshot.error},"diagnostics":self.diagnostics})
    }
}
struct Busy(Arc<AtomicBool>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Fleet {
    pub(crate) fn new(serial: SerialPorts, native: bool) -> Self {
        Self::with_create(
            serial,
            native,
            Arc::new(|| Arc::new(ModuleUsbService::new())),
        )
    }
    fn with_create(serial: SerialPorts, native: bool, create: Create) -> Self {
        Self {
            serial,
            native,
            create,
            state: Mutex::new(State {
                slots: BTreeMap::new(),
                diagnostic: None,
                selected: BTreeMap::new(),
            }),
            closed: AtomicBool::new(false),
            discovery: Mutex::new(()),
        }
    }
    pub(crate) fn scan(&self) -> Result<Vec<SlotSnapshot>, String> {
        self.discover(self.native)?;
        Ok(self.snapshots())
    }
    fn discover(&self, automatic: bool) -> Result<(), String> {
        // Enumerate and adopt in one order; a slow older scan must not replace
        // a newer complete scan from a concurrent explicit Ports request.
        let _discovery = self.discovery.lock().unwrap_or_else(|p| p.into_inner());
        if self.closed.load(Ordering::Acquire) {
            return Err("USB module fleet has stopped.".into());
        }
        let observed = match (self.serial)().and_then(deduplicate) {
            Ok(observed) => observed,
            Err(error) => {
                let error = bounded_error(&error);
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                state.diagnostic = Some(error.clone());
                // Enumeration failure is unknown, not physical absence. Keep
                // workers but withdraw current presence/proof until a good scan.
                for slot in state.slots.values_mut() {
                    slot.present = false;
                }
                return Err(error);
            }
        };
        let mut retired = Vec::new();
        let mut jobs = Vec::new();
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if self.closed.load(Ordering::Acquire) {
                return Err("USB module fleet has stopped.".into());
            }
            let new_count = observed
                .keys()
                .filter(|key| !state.slots.contains_key(*key))
                .count();
            if state.slots.len() + new_count > MAX_RETAINED {
                let error =
                    "USB endpoint inventory is full; restart after removing stale endpoints.";
                state.diagnostic = Some(error.into());
                for slot in state.slots.values_mut() {
                    slot.present = false;
                }
                return Err(error.into());
            }
            let mut serial_counts = BTreeMap::<String, usize>::new();
            let mut path_counts = BTreeMap::<String, usize>::new();
            for port in observed.values() {
                *serial_counts.entry(serial(port)).or_default() += 1;
                *path_counts.entry(alias_path(&port.path)).or_default() += 1;
            }
            state.diagnostic = None;
            for (key, slot) in &mut state.slots {
                if !observed.contains_key(key) {
                    slot.present = false;
                    slot.suppressed = false;
                    if let Some(service) = slot.service.take() {
                        let snapshot = service.snapshot();
                        slot.retiring_service = Some(service.clone());
                        slot.cached = offline(snapshot);
                        slot.closing.store(true, Ordering::Release);
                        retired.push((service, slot.closing.clone()));
                    }
                }
            }
            let occupied = state
                .slots
                .iter()
                .filter(|(_, slot)| {
                    slot.closing.load(Ordering::Acquire)
                        || slot.busy.load(Ordering::Acquire)
                        || slot
                            .service
                            .as_ref()
                            .is_some_and(|service| service.snapshot().connected)
                })
                .map(|(key, slot)| (serial(&slot.port), alias_path(&slot.port.path), key.clone()))
                .collect::<Vec<_>>();
            for (key, port) in observed {
                let ambiguous =
                    serial_counts[&serial(&port)] > 1 || path_counts[&alias_path(&port.path)] > 1;
                let strict = module_usb::candidate(&port);
                let remembered = state.selected.contains_key(&serial(&port));
                let other_worker = occupied
                    .iter()
                    .any(|(other_serial, other_path, other_key)| {
                        (*other_serial == serial(&port) || *other_path == alias_path(&port.path))
                            && *other_key != key
                    });
                let slot = state.slots.entry(key).or_insert_with(|| Slot {
                    port: port.clone(),
                    present: true,
                    automatic: strict || remembered,
                    ambiguous,
                    suppressed: false,
                    service: None,
                    cached: UsbSnapshot::default(),
                    retiring_service: None,
                    busy: Arc::new(AtomicBool::new(false)),
                    closing: Arc::new(AtomicBool::new(false)),
                });
                slot.port = port;
                slot.present = true;
                slot.automatic = strict || remembered;
                slot.ambiguous = ambiguous;
                if !automatic
                    || slot.suppressed
                    || !(strict || remembered)
                    || ambiguous
                    || other_worker
                    || slot.closing.load(Ordering::Acquire)
                    || slot.busy.swap(true, Ordering::AcqRel)
                {
                    continue;
                }
                let service = slot.service.get_or_insert_with(|| (self.create)()).clone();
                let snapshot = service.snapshot();
                let request = if !snapshot.connected {
                    Some(if strict {
                        UsbRequest::Connect {
                            port: slot.port.clone(),
                        }
                    } else {
                        UsbRequest::ConnectExplicit {
                            port: slot.port.clone(),
                        }
                    })
                } else {
                    None
                };
                let route = json!(snapshot.route);
                let needs_caps = snapshot.connected
                    && (snapshot
                        .events
                        .as_ref()
                        .is_none_or(|events| events["route"] != route)
                        || snapshot
                            .actions
                            .as_ref()
                            .is_none_or(|actions| actions["route"] != route));
                if request.is_some() || needs_caps {
                    jobs.push((service, slot.busy.clone(), request));
                } else {
                    slot.busy.store(false, Ordering::Release);
                }
            }
        }
        for (service, closing) in retired {
            std::thread::spawn(move || {
                service.shutdown();
                closing.store(false, Ordering::Release);
            });
        }
        for (service, busy, request) in jobs {
            let reset = busy.clone();
            if std::thread::Builder::new()
                .name("osg-usb-discovery".into())
                .spawn(move || {
                    let _busy = Busy(busy);
                    if let Some(request) = request {
                        if service.request(request).is_err() {
                            return;
                        }
                    }
                    if let Some(route) = service.snapshot().route {
                        let _ = service.request(UsbRequest::EventsState {
                            route: route.clone(),
                        });
                        let _ = service.request(UsbRequest::Action {
                            route,
                            action: ActionRequest::Describe,
                        });
                    }
                })
                .is_err()
            {
                reset.store(false, Ordering::Release);
            }
        }
        Ok(())
    }
    pub(crate) fn snapshots(&self) -> Vec<SlotSnapshot> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state
            .slots
            .iter()
            .map(|(key, slot)| snapshot(key, slot, state.diagnostic.as_deref()))
            .collect()
    }
    pub(crate) fn configuration_claims(
        &self,
    ) -> Vec<crate::module_configuration_routes::WriterPin> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .slots
            .values()
            .filter_map(|slot| {
                slot.service
                    .as_ref()
                    .and_then(|service| writer_pin(&service.snapshot()))
                    .or_else(|| {
                        slot.closing
                            .load(Ordering::Acquire)
                            .then(|| {
                                slot.retiring_service
                                    .as_ref()
                                    .and_then(|service| writer_pin(&service.snapshot()))
                            })
                            .flatten()
                    })
            })
            .collect()
    }
    /// Native profile-only typed access to the existing worker. It cannot open,
    /// disconnect, save flash, reset settings or replay a failed command. The
    /// Manager's ProfilePanelIo owns cross-transport policy and serialization.
    pub(crate) fn request_native(
        &self,
        key: &str,
        operation: UsbRequest,
    ) -> Result<UsbSnapshot, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("USB module fleet has stopped.".into());
        }
        let route = match &operation {
            UsbRequest::ConfigRead { route } | UsbRequest::Claim { route, .. }
            | UsbRequest::Release { route, .. } | UsbRequest::Action { route, .. }
            | UsbRequest::EventsState { route } | UsbRequest::EventsRead { route, .. }
            | UsbRequest::ConfigChange { route, change: ConfigChange::Apply { save: false, .. }, .. } => route,
            _ => return Err("Native profile USB operations must use an exact live route and cannot persist device settings.".into()),
        };
        let (service, _busy) = self.endpoint(key, "native-profile")?;
        let snapshot = service.snapshot();
        if self.closed.load(Ordering::Acquire)
            || !snapshot.connected
            || snapshot.route.as_ref() != Some(route)
        {
            return Err(
                "The native USB route changed before dispatch; acquire a fresh baseline.".into(),
            );
        }
        let result = service.request(operation);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(slot) = state.slots.get_mut(key).filter(|slot| {
            slot.service
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &service))
        }) {
            // An uncertain write must be visible to the next native capture too.
            slot.cached = service.snapshot();
        }
        result
    }
    pub(crate) fn request(&self, request: &Value) -> Result<Value, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("USB module fleet has stopped.".into());
        }
        if !request.is_object()
            || serde_json::to_vec(request)
                .map_err(|e| e.to_string())?
                .len()
                > 65_536
        {
            return Err("Invalid bounded USB module request.".into());
        }
        let command = required(request, "command")?;
        let fields: &[&str] = match command {
            "module_usb_ports" => &[],
            "module_usb_list" => &["offset", "limit"],
            "module_usb_connect" => &["port", "key"],
            "module_usb_disconnect" | "module_usb_status" => &["key"],
            "module_usb_config_read"
            | "module_usb_claim"
            | "module_usb_release"
            | "module_usb_config_save"
            | "module_usb_config_discard"
            | "module_usb_config_reset"
            | "module_usb_actions_describe"
            | "module_usb_actions_state"
            | "module_usb_events_state" => &["key", "route"],
            "module_usb_config_apply" => &["key", "route", "config", "save"],
            "module_usb_actions_invoke" => {
                &["key", "route", "catalogueRevision", "actionId", "values"]
            }
            "module_usb_events_read" => &["key", "route", "stream", "after"],
            _ => return Err("Unknown standalone USB module command.".into()),
        };
        if request
            .as_object()
            .unwrap()
            .keys()
            .any(|key| key != "command" && key != "clientId" && !fields.contains(&key.as_str()))
        {
            return Err("Unknown standalone USB request field.".into());
        }
        if matches!(command, "module_usb_ports" | "module_usb_connect") {
            self.discover(false)?;
        }
        if command == "module_usb_ports" {
            let modules = self.snapshots();
            let ports=modules.iter().filter(|slot|slot.present).map(|slot|json!({"key":slot.key,"port":slot.port.value(),"present":true,"automatic":slot.automatic,"admissible":slot.admissible,"connected":slot.snapshot.connected,"diagnostics":slot.diagnostics})).collect::<Vec<_>>();
            return bounded_reply(json!({"ports":ports}));
        }
        if command == "module_usb_list" {
            let offset = optional_page(request, "offset", 0, MAX_RETAINED)?;
            let limit = optional_page(request, "limit", MAX_PRESENT, MAX_PRESENT)?;
            if limit == 0 {
                return Err("USB module page limit must be positive.".into());
            }
            let slots = self.snapshots();
            let mut modules = slots
                .iter()
                .skip(offset)
                .take(limit)
                .map(SlotSnapshot::summary_value)
                .collect::<Vec<_>>();
            loop {
                let response = json!({"offset":offset,"total":slots.len(),"modules":modules});
                if serde_json::to_vec(&response)
                    .map_err(|e| e.to_string())?
                    .len()
                    <= MAX_REPLY_BYTES
                {
                    return Ok(response);
                }
                if modules.len() <= 1 {
                    return Err("USB module metadata exceeds the reply budget.".into());
                }
                modules.pop();
            }
        }
        let key = if command == "module_usb_connect" {
            let path = required(request, "port")?;
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let selected = state
                .slots
                .iter()
                .filter(|(_, slot)| slot.present && alias_path(&slot.port.path) == alias_path(path))
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            if selected.len() != 1 {
                return Err("Select one currently observed plausible USB endpoint.".into());
            }
            if request.get("key").is_some() && required(request, "key")? != selected[0] {
                return Err("The selected USB endpoint key no longer matches this port.".into());
            }
            selected[0].clone()
        } else {
            required(request, "key")?.to_owned()
        };
        if command == "module_usb_status"
            && self
                .snapshots()
                .iter()
                .any(|s| s.key == key && (!s.admissible || !s.snapshot.connected))
        {
            return self.response(&key);
        }
        let (service, _busy) = self.endpoint(&key, command)?;
        let operation = match command {
            "module_usb_connect" => {
                let port = self.state.lock().unwrap_or_else(|p| p.into_inner()).slots[&key]
                    .port
                    .clone();
                UsbRequest::ConnectExplicit { port }
            }
            "module_usb_disconnect" => UsbRequest::Disconnect,
            "module_usb_status" => {
                if !service.snapshot().connected {
                    return self.response(&key);
                }
                UsbRequest::Refresh
            }
            _ => {
                let route: UsbRouteToken = serde_json::from_value(request["route"].clone())
                    .map_err(|_| "Provide the exact current USB route from module status.")?;
                match command {
                    "module_usb_config_read" => UsbRequest::ConfigRead { route },
                    "module_usb_claim" => UsbRequest::Claim {
                        route,
                        client_id: client(request)?,
                    },
                    "module_usb_release" => UsbRequest::Release {
                        route,
                        client_id: client(request)?,
                    },
                    "module_usb_config_apply" => {
                        let save = request
                            .get("save")
                            .map_or(Ok(false), |v| v.as_bool().ok_or("save must be a boolean."))?;
                        UsbRequest::ConfigChange {
                            route,
                            client_id: client(request)?,
                            change: ConfigChange::Apply {
                                config: request["config"].clone(),
                                save,
                            },
                        }
                    }
                    "module_usb_config_save"
                    | "module_usb_config_discard"
                    | "module_usb_config_reset" => UsbRequest::ConfigChange {
                        route,
                        client_id: client(request)?,
                        change: match command {
                            "module_usb_config_save" => ConfigChange::Save,
                            "module_usb_config_discard" => ConfigChange::Discard,
                            _ => ConfigChange::Reset,
                        },
                    },
                    "module_usb_actions_describe" => UsbRequest::Action {
                        route,
                        action: ActionRequest::Describe,
                    },
                    "module_usb_actions_state" => UsbRequest::Action {
                        route,
                        action: ActionRequest::State,
                    },
                    "module_usb_actions_invoke" => {
                        let values = request["values"]
                            .as_array()
                            .filter(|v| v.len() <= 4)
                            .ok_or("Provide the exact action argument array, up to four values.")?
                            .clone();
                        UsbRequest::Action {
                            route,
                            action: ActionRequest::Invoke {
                                catalogue_revision: number(request, "catalogueRevision", false)?,
                                action_id: required(request, "actionId")?.to_owned(),
                                values,
                            },
                        }
                    }
                    "module_usb_events_state" => UsbRequest::EventsState { route },
                    _ => UsbRequest::EventsRead {
                        route,
                        stream: number(request, "stream", false)?,
                        after: number(request, "after", true)?,
                    },
                }
            }
        };
        let result = service.request(operation);
        if result.is_ok() {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if command == "module_usb_connect" {
                if let Some(identity) = &service.snapshot().identity {
                    state
                        .selected
                        .insert(identity.serial.clone(), identity.module_id.clone());
                }
            } else if command == "module_usb_disconnect" {
                if let Some(slot) = state.slots.get(&key) {
                    let serial = serial(&slot.port);
                    state.selected.remove(&serial);
                }
            }
            if let Some(slot) = state.slots.get_mut(&key) {
                slot.cached = service.snapshot();
                slot.suppressed = command == "module_usb_disconnect";
            }
        }
        result?;
        self.response(&key)
    }
    fn endpoint(&self, key: &str, command: &str) -> Result<(Arc<dyn Endpoint>, Busy), String> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let slot = state
            .slots
            .get(key)
            .ok_or("Unknown standalone USB endpoint.")?;
        if command != "module_usb_disconnect"
            && (!slot.present || state.diagnostic.is_some() || slot.ambiguous)
        {
            return Err("The USB endpoint is absent, uncertain, or has an ambiguous physical serial. Refresh discovery before using a route.".into());
        }
        if slot.closing.load(Ordering::Acquire) {
            return Err(
                "Wait for the previous USB worker to close before opening a fresh route.".into(),
            );
        }
        if command == "module_usb_connect" {
            if !slot.present {
                return Err("USB endpoint is no longer observed.".into());
            }
            let selected_serial = serial(&slot.port);
            if state.slots.iter().any(|(other, slot)| {
                other != key
                    && (serial(&slot.port) == selected_serial
                        || alias_path(&slot.port.path) == alias_path(&state.slots[key].port.path))
                    && (slot.closing.load(Ordering::Acquire)
                        || slot.service.as_ref().is_some_and(|s| {
                            s.snapshot().connected || slot.busy.load(Ordering::Acquire)
                        }))
            }) {
                return Err("Another endpoint with this physical USB serial is connected or opening; disconnect it before selecting another route.".into());
            }
        }
        let slot = state.slots.get_mut(key).unwrap();
        if slot.busy.swap(true, Ordering::AcqRel) {
            return Err(
                "The USB endpoint is busy; wait for the pending request. No command was replayed."
                    .into(),
            );
        }
        let busy = Busy(slot.busy.clone());
        let service = if command == "module_usb_connect" {
            slot.service.get_or_insert_with(|| (self.create)()).clone()
        } else {
            slot.service
                .clone()
                .ok_or("Connect the observed USB endpoint first.")?
        };
        Ok((service, busy))
    }
    fn response(&self, key: &str) -> Result<Value, String> {
        self.snapshots()
            .into_iter()
            .find(|slot| slot.key == key)
            .map(|slot| bounded_reply(json!({"module":slot.value()})))
            .ok_or_else(|| "USB endpoint disappeared during the request.".to_owned())?
    }
    pub(crate) fn shutdown(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let services = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state
                .slots
                .values_mut()
                .filter_map(|slot| {
                    slot.present = false;
                    slot.service.take()
                })
                .collect::<Vec<_>>()
        };
        for service in services {
            service.shutdown();
        }
    }
}
impl Drop for Fleet {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn required<'a>(request: &'a Value, key: &str) -> Result<&'a str, String> {
    request[key]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 2048 && !v.chars().any(char::is_control))
        .ok_or_else(|| format!("{key} must be a bounded string."))
}
fn optional_page(request: &Value, key: &str, default: usize, max: usize) -> Result<usize, String> {
    request.get(key).map_or(Ok(default), |value| {
        value
            .as_u64()
            .filter(|value| *value <= max as u64)
            .map(|value| value as usize)
            .ok_or_else(|| format!("Invalid USB module page {key}."))
    })
}
fn bounded_reply(value: Value) -> Result<Value, String> {
    if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > MAX_REPLY_BYTES {
        Err("USB module reply exceeds the IPC budget; inspect one module at a time.".into())
    } else {
        Ok(value)
    }
}
fn bounded_error(error: &str) -> String {
    let mut result = String::new();
    for ch in error.chars().filter(|ch| !ch.is_control()) {
        if result.len() + ch.len_utf8() > 512 {
            break;
        }
        result.push(ch);
    }
    result
}
fn client(request: &Value) -> Result<String, String> {
    let client = required(request, "clientId")?;
    if !crate::protocol::identifier(client, 1, 80) {
        return Err("Invalid USB writer client identity.".into());
    }
    Ok(client.into())
}
fn number(request: &Value, key: &str, zero: bool) -> Result<u32, String> {
    request[key]
        .as_u64()
        .filter(|v| *v <= u32::MAX as u64 && (zero || *v > 0))
        .map(|v| v as u32)
        .ok_or_else(|| format!("Invalid USB {key}."))
}
fn serial(port: &PortInfo) -> String {
    port.serial_number
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
}
fn alias_path(path: &str) -> String {
    path.replace("/dev/tty.usbmodem", "/dev/usbmodem")
        .replace("/dev/cu.usbmodem", "/dev/usbmodem")
}
fn deduplicate(ports: Vec<PortInfo>) -> Result<BTreeMap<String, PortInfo>, String> {
    let mut result = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for port in ports
        .into_iter()
        .filter(|port| port.path.len() <= 256 && module_usb::candidate_for_explicit(port))
    {
        let path = alias_path(&port.path);
        let key = format!("{}:{path}", serial(&port));
        if !paths.insert((key.clone(), port.path.clone())) {
            continue;
        }
        if !result.contains_key(&key) || port.path.starts_with("/dev/cu.") {
            result.insert(key, port);
        }
    }
    if result.len() > MAX_PRESENT {
        return Err("Standalone USB discovery exceeds 32 plausible endpoints; narrow the connected device set.".into());
    }
    Ok(result)
}
fn offline(mut snapshot: UsbSnapshot) -> UsbSnapshot {
    snapshot.connected = false;
    snapshot.route = None;
    snapshot.claimed_by = None;
    snapshot.draft = None;
    snapshot.events = None;
    if let Some(actions) = &mut snapshot.actions {
        actions["status"] = json!("uncertain");
        actions["states"] = json!([]);
    }
    snapshot
}
fn writer_pin(snapshot: &UsbSnapshot) -> Option<crate::module_configuration_routes::WriterPin> {
    let route = snapshot.route.as_ref()?;
    Some(crate::module_configuration_routes::WriterPin {
        module_id: route.module_id.clone(),
        client_id: snapshot.claimed_by.as_ref()?.clone(),
        boot: route.boot,
    })
}
fn snapshot(key: &str, slot: &Slot, diagnostic: Option<&str>) -> SlotSnapshot {
    let mut diagnostics = diagnostic
        .map(str::to_owned)
        .into_iter()
        .collect::<Vec<_>>();
    if slot.ambiguous {
        diagnostics.push("USB endpoint identities conflict or several endpoints advertise the same physical serial; connection and routed access are blocked.".into());
    }
    if slot.suppressed {
        diagnostics.push("Disconnected explicitly; automatic reconnect is suspended until a new arrival or explicit Connect.".into());
    }
    let admissible = slot.present
        && diagnostic.is_none()
        && !slot.ambiguous
        && !slot.closing.load(Ordering::Acquire);
    let current = slot
        .service
        .as_ref()
        .map_or_else(|| slot.cached.clone(), |service| service.snapshot());
    SlotSnapshot {
        key: key.into(),
        port: slot.port.clone(),
        present: slot.present && diagnostic.is_none(),
        automatic: slot.automatic,
        admissible,
        snapshot: if admissible {
            current
        } else {
            offline(current)
        },
        diagnostics,
    }
}

#[cfg(test)]
#[path = "module_usb_fleet_tests.rs"]
pub(crate) mod tests;

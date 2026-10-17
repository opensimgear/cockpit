//! A standalone USB module owns one long-lived CDC worker. Observations never
//! claim configuration or outputs. USB has no mutation replay/correlation cache.
use crate::{
    module_console::{self, Console, Reply, ReplyKind, UsbIdentity},
    transport::{Factory, NativeFactory, PortInfo},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) use crate::module_console::{candidate, candidate_for_explicit};
const CLOSED: &str = "USB module service has stopped.";
const STALE: &str =
    "The USB module route changed; refresh the device and establish a fresh session.";
const UNKNOWN_CONFIG: &str = "The USB configuration reply was lost. Read active and saved settings before another explicit change; the write was not repeated.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UsbRouteToken {
    pub module_id: String,
    pub serial: String,
    pub connection_generation: u64,
    pub boot: u32,
    pub configuration_generation: u32,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsbSnapshot {
    pub connected: bool,
    pub port: Option<String>,
    pub identity: Option<UsbIdentity>,
    pub route: Option<UsbRouteToken>,
    /// Always active settings. Writer draft data has a distinct field.
    pub config: Option<Value>,
    pub draft: Option<Value>,
    pub actions: Option<Value>,
    pub events: Option<Value>,
    pub claimed_by: Option<String>,
    pub observed_at_ms: Option<u64>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub(crate) enum ConfigChange {
    Apply { config: Value, save: bool },
    Save,
    Discard,
    Reset,
}
/// Ownership tokens are input-only capabilities: no Debug/Serialize and no
/// request or raw response is retained in public snapshots or error messages.
#[derive(Clone)]
pub(crate) enum ActionRequest {
    Describe,
    State,
    Invoke {
        catalogue_revision: u32,
        action_id: String,
        values: Vec<Value>,
    },
    Ownership {
        offset: u32,
    },
    Claim {
        catalogue_revision: u32,
        action_id: String,
        values: Vec<Value>,
        token: String,
        ttl_ms: u32,
    },
    Renew {
        token: String,
        lease_id: u32,
        ttl_ms: u32,
    },
    Release {
        token: String,
        lease_id: u32,
    },
    InvokeOwned {
        catalogue_revision: u32,
        action_id: String,
        values: Vec<Value>,
        token: String,
        lease_id: u32,
    },
}
#[derive(Clone)]
pub(crate) enum UsbRequest {
    Connect {
        port: PortInfo,
    },
    /// Explicit operator selection permits generic OS labels; role and UID are verified.
    ConnectExplicit {
        port: PortInfo,
    },
    Refresh,
    Disconnect,
    ConfigRead {
        route: UsbRouteToken,
    },
    Claim {
        route: UsbRouteToken,
        client_id: String,
    },
    Release {
        route: UsbRouteToken,
        client_id: String,
    },
    ConfigChange {
        route: UsbRouteToken,
        client_id: String,
        change: ConfigChange,
    },
    Action {
        route: UsbRouteToken,
        action: ActionRequest,
    },
    EventsState {
        route: UsbRouteToken,
    },
    EventsRead {
        route: UsbRouteToken,
        stream: u32,
        after: u32,
    },
}
#[derive(Clone)]
struct Timing {
    read: Duration,
    command: Duration,
    refresh: Duration,
    reconnect: Duration,
    settle: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            read: Duration::from_millis(25),
            command: Duration::from_secs(3),
            refresh: Duration::from_secs(2),
            reconnect: Duration::from_secs(10),
            settle: Duration::from_millis(350),
        }
    }
}
struct Writer {
    client: String,
    owner: u32,
    route: UsbRouteToken,
}
struct Worker {
    factory: Box<dyn Factory>,
    console: Option<Console>,
    snapshot: UsbSnapshot,
    generation: u64,
    explicit: bool,
    writer: Option<Writer>,
    baseline: Option<(UsbRouteToken, u32, u32)>,
    timing: Timing,
    cancel: Arc<AtomicBool>,
}
impl Worker {
    fn new(factory: Box<dyn Factory>, timing: Timing, cancel: Arc<AtomicBool>) -> Self {
        Self {
            factory,
            timing,
            cancel,
            console: None,
            snapshot: UsbSnapshot::default(),
            generation: ((uuid::Uuid::new_v4().as_u128() as u64) & ((1u64 << 52) - 1)).max(1),
            explicit: false,
            writer: None,
            baseline: None,
        }
    }
    fn close(&mut self) {
        self.console = None; // Dropping DTR invalidates USB-scoped firmware access.
        self.writer = None;
        self.baseline = None;
        self.snapshot.connected = false;
        self.snapshot.route = None;
        self.snapshot.draft = None;
        self.snapshot.claimed_by = None;
        self.snapshot.events = None;
        if let Some(actions) = &mut self.snapshot.actions {
            actions["status"] = json!("uncertain");
            actions["states"] = json!([]);
            actions.as_object_mut().map(|v| {
                v.remove("lastLease");
                v.remove("ownershipPage");
            });
        }
    }
    fn exchange(&mut self, command: &str, kind: ReplyKind) -> Result<Reply, String> {
        let result = self
            .console
            .as_mut()
            .ok_or("Connect the standalone USB module first.")?
            .exchange(command, kind, Instant::now() + self.timing.command);
        if result.is_err() {
            self.close();
        }
        result
    }
    fn text(&mut self, command: &str) -> Result<String, String> {
        match self.exchange(command, ReplyKind::Text)? {
            Reply::Text(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    fn config_command(&mut self, child: &str) -> Result<Value, String> {
        let Reply::Config { error, snapshot } =
            self.exchange(&format!("config {child}"), ReplyKind::Config)?
        else {
            unreachable!()
        };
        self.accept_config(&snapshot)?;
        if let Some(error) = error {
            return Err(format!("The USB module rejected configuration: {error}"));
        }
        Ok(snapshot)
    }
    fn accept_config(&mut self, config: &Value) -> Result<(), String> {
        module_console::validate_config(config)?;
        let generation = config["generation"].as_u64().unwrap() as u32;
        if let Some(route) = &mut self.snapshot.route {
            if route.configuration_generation != generation {
                route.configuration_generation = generation;
                self.snapshot.actions = None;
                self.snapshot.events = None;
                self.baseline = None;
                self.writer = None;
                self.snapshot.claimed_by = None;
            }
        }
        if config["view"] == "active" || config["dirty"] == false && config["pendingApply"] == false
        {
            let mut active = config.clone();
            active["view"] = json!("active");
            active["dirty"] = json!(false);
            self.snapshot.config = Some(active);
            if let Some(identity) = &mut self.snapshot.identity {
                identity.profile = config["config"]["name"]
                    .as_str()
                    .unwrap_or(&identity.profile)
                    .to_owned();
                identity.button_count = config["layout"]["buttons"].as_u64().unwrap_or(0) as u32;
                identity.axis_count = config["layout"]["axes"].as_u64().unwrap_or(0) as u32;
                identity.hat_count = config["layout"]["hats"].as_u64().unwrap_or(0) as u32;
            }
        }
        if config["view"] == "draft" {
            self.snapshot.draft = Some(config.clone());
        }
        if self
            .writer
            .as_ref()
            .is_some_and(|writer| config["owner"].as_u64() != Some(writer.owner as u64))
        {
            self.writer = None;
            self.snapshot.claimed_by = None;
            self.snapshot.draft = None;
        }
        self.snapshot.observed_at_ms = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        );
        Ok(())
    }
    fn status(&mut self) -> Result<module_console::ModuleStatus, String> {
        let line = self.text("can status")?;
        let status = module_console::status(&line).inspect_err(|_| self.close())?;
        if self
            .snapshot
            .route
            .as_ref()
            .is_some_and(|route| route.boot != status.boot)
        {
            self.close();
            return Err(STALE.into());
        }
        Ok(status)
    }
    fn connect(&mut self, requested: &PortInfo, explicit: bool) -> Result<(), String> {
        let eligible = if explicit {
            candidate_for_explicit
        } else {
            candidate
        };
        if !eligible(requested) {
            return Err("Select a compatible standalone OpenSimGear USB module.".into());
        }
        if self.snapshot.identity.as_ref().is_some_and(|identity| {
            requested
                .serial_number
                .as_deref()
                .and_then(module_console::serial)
                .as_deref()
                != Some(&identity.serial)
        }) {
            return Err(
                "This USB service is bound to another physical module; create a separate service."
                    .into(),
            );
        }
        let ports: Vec<_> = self
            .factory
            .ports()?
            .into_iter()
            .filter(|port| {
                eligible(port)
                    && port.path == requested.path
                    && port
                        .serial_number
                        .as_deref()
                        .and_then(module_console::serial)
                        == requested
                            .serial_number
                            .as_deref()
                            .and_then(module_console::serial)
            })
            .collect();
        if ports.len() != 1 {
            return Err("The selected USB module port is missing or ambiguous.".into());
        }
        let port = &ports[0];
        if self.snapshot.connected
            && self.snapshot.port.as_deref() == Some(&port.path)
            && self.snapshot.identity.as_ref().is_some_and(|id| {
                Some(id.serial.clone())
                    == port
                        .serial_number
                        .as_deref()
                        .and_then(module_console::serial)
            })
        {
            self.refresh()?;
            self.explicit = explicit;
            return Ok(());
        }
        self.close();
        self.console = Some(Console::new(
            self.factory.open(port, self.timing.read)?,
            self.cancel.clone(),
        )?);
        let result = (|| {
            let identity = module_console::identity(&self.text("info")?, port)?;
            let status = module_console::status(&self.text("can status")?)?;
            let config = self.config_command("get_active")?;
            if config["view"] != "active"
                || config["valid"] != true
                || config["layout"]["buttons"] != identity.button_count
                || config["layout"]["axes"] != identity.axis_count
                || config["layout"]["hats"] != identity.hat_count
            {
                return Err("USB module identity and active layout disagree.".into());
            }
            self.generation = self
                .generation
                .checked_add(1)
                .filter(|g| *g <= 9_007_199_254_740_991)
                .ok_or("USB connection generation is exhausted.")?;
            self.snapshot.identity = Some(identity.clone());
            self.snapshot.port = Some(port.path.clone());
            self.snapshot.route = Some(UsbRouteToken {
                module_id: identity.module_id,
                serial: identity.serial,
                boot: status.boot,
                connection_generation: self.generation,
                configuration_generation: config["generation"].as_u64().unwrap() as u32,
            });
            self.snapshot.connected = true;
            self.explicit = explicit;
            self.console.as_mut().unwrap().finish_handshake();
            self.snapshot.error = None;
            Ok(())
        })();
        if result.is_err() {
            self.close();
        }
        result
    }
    fn refresh(&mut self) -> Result<(), String> {
        self.status()?;
        let active = self.config_command("get_active")?;
        if active["view"] != "active" || active["valid"] != true {
            self.close();
            return Err("USB module did not return active settings.".into());
        }
        self.snapshot.error = None;
        Ok(())
    }
    fn guard(&mut self, expected: &UsbRouteToken) -> Result<(), String> {
        if !self.snapshot.connected || self.snapshot.route.as_ref() != Some(expected) {
            return Err(STALE.into());
        }
        self.refresh()?;
        if self.snapshot.route.as_ref() != Some(expected) {
            return Err(STALE.into());
        }
        Ok(())
    }
    fn writer(&self, route: &UsbRouteToken, client: &str) -> Result<u32, String> {
        self.writer
            .as_ref()
            .filter(|writer| writer.client == client && writer.route == *route)
            .map(|writer| writer.owner)
            .ok_or_else(|| "Start a fresh USB setup claim before changing configuration.".into())
    }
    fn claim(&mut self, route: &UsbRouteToken, client: &str) -> Result<(), String> {
        if !crate::protocol::identifier(client, 1, 80) {
            return Err("Invalid USB setup client identity.".into());
        }
        self.guard(route)?;
        if self
            .writer
            .as_ref()
            .is_some_and(|writer| writer.client != client)
        {
            return Err("Another Cockpit session owns this USB module.".into());
        }
        let owner = crate::runtime::owner(client);
        let snapshot = self.config_command(&format!("claim {owner}"))?;
        if snapshot["owner"].as_u64() != Some(owner as u64) {
            return Err("The USB module did not grant the writer claim.".into());
        }
        self.writer = Some(Writer {
            client: client.into(),
            owner,
            route: route.clone(),
        });
        self.snapshot.claimed_by = Some(client.into());
        Ok(())
    }
    fn wait(&self, duration: Duration) -> Result<(), String> {
        let end = Instant::now() + duration;
        while Instant::now() < end {
            if self.cancel.load(Ordering::Acquire) {
                return Err(CLOSED.into());
            }
            thread::sleep(
                Duration::from_millis(10).min(end.saturating_duration_since(Instant::now())),
            );
        }
        Ok(())
    }
    fn reconcile(
        &mut self,
        prior: &UsbRouteToken,
        expected: &Value,
        saved: bool,
        applied: bool,
    ) -> Result<(), String> {
        let expected_generation = prior
            .configuration_generation
            .checked_add(u32::from(applied))
            .ok_or("USB configuration generation is exhausted.")?;
        let explicit = self.explicit;
        let eligible = if explicit {
            candidate_for_explicit
        } else {
            candidate
        };
        self.close();
        let deadline = Instant::now() + self.timing.reconnect;
        self.wait(self.timing.settle)?;
        while Instant::now() < deadline {
            let ports: Vec<_> = self
                .factory
                .ports()?
                .into_iter()
                .filter(|port| {
                    eligible(port)
                        && port
                            .serial_number
                            .as_deref()
                            .and_then(module_console::serial)
                            .as_deref()
                            == Some(&prior.serial)
                })
                .collect();
            if ports.len() > 1 {
                return Err(
                    "USB module reconnection is ambiguous; the commit was not repeated.".into(),
                );
            }
            if let Some(port) = ports.first() {
                if self.connect(port, explicit).is_ok() {
                    let actual_route = self.snapshot.route.as_ref().unwrap();
                    if actual_route.module_id != prior.module_id || actual_route.boot != prior.boot
                    {
                        self.close();
                        return Err("The USB module restarted or was replaced during configuration; the commit was not repeated.".into());
                    }
                    let actual = self.snapshot.config.as_ref().unwrap();
                    if actual["config"] == *expected
                        && actual["generation"].as_u64() == Some(expected_generation as u64)
                        && (!saved || actual["generation"] == actual["savedGeneration"])
                    {
                        return Ok(());
                    }
                }
            }
            self.wait(Duration::from_millis(50))?;
        }
        Err(UNKNOWN_CONFIG.into())
    }
    fn change(
        &mut self,
        route: &UsbRouteToken,
        client: &str,
        change: ConfigChange,
    ) -> Result<(), String> {
        self.guard(route)?;
        let owner = self.writer(route, client)?;
        match change {
            ConfigChange::Discard | ConfigChange::Reset => {
                let verb = if matches!(change, ConfigChange::Discard) {
                    "discard"
                } else {
                    "reset"
                };
                self.config_command(&format!("{verb} {owner}"))?;
                Ok(())
            }
            ConfigChange::Save => {
                let expected = self
                    .snapshot
                    .config
                    .as_ref()
                    .ok_or("Read active settings first.")?["config"]
                    .clone();
                match self.config_command(&format!("save {owner}")) {
                    Ok(reply) => {
                        if reply["generation"] != reply["savedGeneration"] {
                            return Err("USB module did not confirm saved settings.".into());
                        }
                        match self.refresh() {
                            Ok(()) => {
                                let active = self.snapshot.config.as_ref().unwrap();
                                if active["config"] != expected
                                    || active["generation"] != active["savedGeneration"]
                                {
                                    return Err(
                                        "USB active settings readback did not confirm the save."
                                            .into(),
                                    );
                                }
                                Ok(())
                            }
                            Err(_) if self.console.is_none() => {
                                self.reconcile(route, &expected, true, false)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Err(_) if self.console.is_none() => {
                        self.reconcile(route, &expected, true, false)
                    }
                    Err(error) => Err(error),
                }
            }
            ConfigChange::Apply { config, save } => {
                if route.configuration_generation == u32::MAX {
                    return Err("USB configuration generation is exhausted.".into());
                }
                let commands = crate::runtime::commands(&config, owner)?;
                for child in commands {
                    self.config_command(&child)?;
                }
                if self
                    .snapshot
                    .draft
                    .as_ref()
                    .is_none_or(|draft| draft["valid"] != true || draft["config"] != config)
                {
                    return Err(
                        "The USB module draft did not match the requested configuration.".into(),
                    );
                }
                let verb = if save { "apply_save" } else { "apply" };
                match self.config_command(&format!("{verb} {owner}")) {
                    Ok(_) => self.reconcile(route, &config, save, true),
                    Err(_) if self.console.is_none() => self.reconcile(route, &config, save, true),
                    Err(error) => Err(error),
                }
            }
        }
    }
    fn capability_json(&mut self, command: &str) -> Result<Option<Value>, String> {
        let value = match self.exchange(command, ReplyKind::Json)? {
            Reply::Json(value) => value,
            Reply::Unsupported => return Ok(None),
            _ => unreachable!(),
        };
        if value["ok"] == false {
            let error = value["error"]
                .as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 63
                        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
                .ok_or("USB module returned an invalid capability error.")?;
            return Err(format!("The USB module rejected the request: {error}"));
        }
        Ok(Some(value))
    }
    fn json(&mut self, command: &str) -> Result<Value, String> {
        self.capability_json(command)?
            .ok_or_else(|| "This module does not advertise the requested capability.".into())
    }
    fn unsupported(&mut self, route: &UsbRouteToken, actions: bool) -> Result<(), String> {
        self.status()?;
        if self.snapshot.route.as_ref() != Some(route) {
            return Err(STALE.into());
        }
        let marker = json!({"boot":route.boot,"route":route,"status":"unsupported"});
        if actions {
            self.snapshot.actions = Some(marker);
        } else {
            self.snapshot.events = Some(marker);
            self.baseline = None;
        }
        Ok(())
    }
    fn describe(&mut self, route: &UsbRouteToken) -> Result<(), String> {
        self.snapshot.actions = None;
        let mut offset = 0;
        let mut revision = None;
        let mut total = None;
        let mut ownership = Value::Null;
        let mut actions = vec![];
        let mut ids = HashSet::new();
        loop {
            let Some(reply) = self.capability_json(&format!("action describe {offset}"))? else {
                if offset == 0 {
                    return self.unsupported(route, true);
                }
                return Err("The USB action catalogue became unavailable during discovery.".into());
            };
            let (current_revision, current_total, next, page) =
                crate::actions::page(&reply, offset)?;
            let capability = reply
                .get("ownership")
                .map(crate::output_leases::capability)
                .transpose()?
                .unwrap_or(Value::Null);
            if revision.is_some_and(|v| v != current_revision)
                || total.is_some_and(|v| v != current_total)
                || revision.is_some() && ownership != capability
            {
                return Err("The USB action catalogue changed during discovery.".into());
            }
            revision = Some(current_revision);
            total = Some(current_total);
            ownership = capability;
            for action in page {
                if !ids.insert(action["id"].as_str().unwrap().to_owned()) {
                    return Err(crate::actions::INVALID.into());
                }
                actions.push(action);
            }
            if let Some(next) = next {
                offset = next;
            } else {
                break;
            }
        }
        self.status()?;
        if self.snapshot.route.as_ref() != Some(route) {
            return Err(STALE.into());
        }
        self.snapshot.actions = Some(
            json!({"boot":route.boot,"route":route,"status":"ready","catalogueRevision":revision,"actions":actions,"states":[],"ownership":ownership,"error":null}),
        );
        self.action(route, ActionRequest::State)
    }
    fn action(&mut self, route: &UsbRouteToken, action: ActionRequest) -> Result<(), String> {
        if matches!(action, ActionRequest::Describe) {
            return self.describe(route);
        }
        let mut cache = self
            .snapshot
            .actions
            .clone()
            .filter(|cache| cache["route"] == json!(route))
            .ok_or("Discover USB module actions first.")?;
        let revision = cache["catalogueRevision"]
            .as_u64()
            .ok_or(crate::actions::INVALID)?;
        let descriptors = cache["actions"]
            .as_array()
            .ok_or(crate::actions::INVALID)?
            .clone();
        let (command, request, invocation) = match action {
            ActionRequest::Describe => unreachable!(),
            ActionRequest::State => ("action_state", json!({}), false),
            ActionRequest::Invoke {
                catalogue_revision,
                action_id,
                values,
            } => (
                "action_invoke",
                json!({"catalogueRevision":catalogue_revision,"actionId":action_id,"values":values}),
                true,
            ),
            ActionRequest::Ownership { offset } => {
                ("action_ownership", json!({"offset":offset}), false)
            }
            ActionRequest::Claim {
                catalogue_revision,
                action_id,
                values,
                token,
                ttl_ms,
            } => (
                "action_claim",
                json!({"catalogueRevision":catalogue_revision,"actionId":action_id,"values":values,"ownershipToken":token,"ttlMs":ttl_ms}),
                false,
            ),
            ActionRequest::Renew {
                token,
                lease_id,
                ttl_ms,
            } => (
                "action_renew",
                json!({"ownershipToken":token,"leaseId":lease_id,"ttlMs":ttl_ms}),
                false,
            ),
            ActionRequest::Release { token, lease_id } => (
                "action_release",
                json!({"ownershipToken":token,"leaseId":lease_id}),
                false,
            ),
            ActionRequest::InvokeOwned {
                catalogue_revision,
                action_id,
                values,
                token,
                lease_id,
            } => (
                "action_invoke_owned",
                json!({"catalogueRevision":catalogue_revision,"actionId":action_id,"values":values,"ownershipToken":token,"leaseId":lease_id}),
                true,
            ),
        };
        let mutating = !matches!(command, "action_state" | "action_ownership");
        if mutating && cache["status"] != "ready" {
            return Err("Refresh USB output state before another mutation.".into());
        }
        let lease_operation = matches!(
            command,
            "action_claim"
                | "action_renew"
                | "action_release"
                | "action_ownership"
                | "action_invoke_owned"
        );
        if lease_operation && command != "action_ownership" && cache["ownership"].is_null() {
            return Err("The USB module does not advertise endpoint ownership.".into());
        }
        let child = if lease_operation {
            crate::output_leases::child(command, &request, revision, &descriptors)?
        } else if invocation {
            if request["catalogueRevision"].as_u64() != Some(revision) {
                return Err("The USB action catalogue changed; discover it again.".into());
            }
            let descriptor = descriptors
                .iter()
                .find(|a| a["id"] == request["actionId"])
                .ok_or("The USB module does not advertise that action.")?;
            let args = crate::actions::arguments(descriptor, &request["values"])?;
            format!(
                "invoke {revision} {}{}",
                descriptor["id"].as_str().unwrap(),
                if args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", args.join(" "))
                }
            )
        } else {
            "state".into()
        };
        let result: Result<(), String> = (|| {
            let reply = self.json(&format!("action {child}"))?;
            self.status()?;
            if self.snapshot.route.as_ref() != Some(route) {
                return Err(STALE.into());
            }
            if let Some(capability) = reply.get("ownership") {
                if capability.is_object() {
                    cache["ownership"] = crate::output_leases::capability(capability)?;
                }
            }
            if command == "action_ownership" {
                cache["ownershipPage"] = crate::output_leases::ownership_page(
                    &reply,
                    revision,
                    request["offset"].as_u64().unwrap() as usize,
                )?;
            } else if matches!(command, "action_claim" | "action_renew" | "action_release") {
                crate::output_leases::acknowledgement(&reply, revision, &request, command)?;
                if command == "action_claim"
                    && descriptors
                        .iter()
                        .find(|a| a["id"] == request["actionId"])
                        .is_none_or(|a| a["endpoint"] != reply["endpoint"])
                {
                    return Err(crate::actions::INVALID.into());
                }
                cache["lastLease"] = crate::output_leases::lease_receipt(&reply);
            } else {
                if invocation && reply["id"] != request["actionId"] {
                    return Err(crate::actions::INVALID.into());
                }
                let states = crate::actions::states(&reply, revision, &descriptors, invocation)?;
                if invocation {
                    let existing = cache["states"]
                        .as_array_mut()
                        .ok_or(crate::actions::INVALID)?;
                    for state in states {
                        existing.retain(|prior| prior["id"] != state["id"]);
                        existing.push(state);
                    }
                } else {
                    cache["states"] = json!(states);
                }
                cache["status"] = json!("ready");
                cache["error"] = Value::Null;
            }
            Ok(())
        })();
        if let Err(error) = result {
            cache["status"] = json!(if mutating { "uncertain" } else { "error" });
            cache["states"] = json!([]);
            cache["error"] = json!(if mutating {
                crate::actions::UNKNOWN
            } else {
                &error
            });
            self.snapshot.actions = Some(cache);
            return Err(if mutating {
                crate::actions::UNKNOWN.into()
            } else {
                error
            });
        }
        self.snapshot.actions = Some(cache);
        Ok(())
    }
    fn events(&mut self, route: &UsbRouteToken, cursor: Option<(u32, u32)>) -> Result<(), String> {
        if let Some((stream, after)) = cursor {
            if self
                .baseline
                .as_ref()
                .is_none_or(|(baseline, expected_stream, floor)| {
                    baseline != route || *expected_stream != stream || after < *floor
                })
            {
                return Err("Establish a fresh USB input baseline before reading events; earlier retained presses are not replayed.".into());
            }
        }
        let command = cursor.map_or_else(
            || "events state".into(),
            |(stream, after)| format!("events read {stream} {after}"),
        );
        let result: Result<(), String> = (|| {
            let Some(mut reply) = self.capability_json(&command)? else {
                if cursor.is_none() {
                    return self.unsupported(route, false);
                }
                return Err("The USB input event capability became unavailable.".into());
            };
            crate::panel_events::validate(&reply, cursor)?;
            let config = self
                .snapshot
                .config
                .as_ref()
                .ok_or("Read the USB module active configuration first.")?;
            if reply["buttonCount"] != config["layout"]["buttons"]
                || reply
                    .get("axisCount")
                    .is_some_and(|count| *count != config["layout"]["axes"])
            {
                return Err("USB event capabilities do not match the active module layout.".into());
            }
            self.status()?;
            if self.snapshot.route.as_ref() != Some(route) {
                return Err(STALE.into());
            }
            if cursor.is_none()
                && self.baseline.as_ref().is_none_or(|(prior, stream, _)| {
                    prior != route || Some(*stream as u64) != reply["stream"].as_u64()
                })
            {
                self.baseline = Some((
                    route.clone(),
                    reply["stream"].as_u64().unwrap() as u32,
                    reply["head"].as_u64().unwrap() as u32,
                ));
            }
            reply["boot"] = json!(route.boot);
            reply["route"] = json!(route);
            self.snapshot.events = Some(reply);
            Ok(())
        })();
        if result.is_err() {
            self.snapshot.events = None;
            self.baseline = None;
        }
        result
    }
    fn handle(&mut self, request: UsbRequest) -> Result<(), String> {
        match request {
            UsbRequest::Connect { port } => self.connect(&port, false),
            UsbRequest::ConnectExplicit { port } => self.connect(&port, true),
            UsbRequest::Disconnect => {
                self.close();
                Ok(())
            }
            UsbRequest::Refresh => self.refresh(),
            UsbRequest::ConfigRead { route } => self.guard(&route),
            UsbRequest::Claim { route, client_id } => self.claim(&route, &client_id),
            UsbRequest::Release { route, client_id } => {
                self.guard(&route)?;
                let owner = self.writer(&route, &client_id)?;
                self.config_command(&format!("release {owner}"))?;
                self.writer = None;
                self.snapshot.claimed_by = None;
                self.snapshot.draft = None;
                Ok(())
            }
            UsbRequest::ConfigChange {
                route,
                client_id,
                change,
            } => self.change(&route, &client_id, change),
            UsbRequest::Action { route, action } => {
                self.guard(&route)?;
                self.action(&route, action)
            }
            UsbRequest::EventsState { route } => {
                self.guard(&route)?;
                self.events(&route, None)
            }
            UsbRequest::EventsRead {
                route,
                stream,
                after,
            } => {
                self.guard(&route)?;
                self.events(&route, Some((stream, after)))
            }
        }
    }
}

enum Message {
    Request(UsbRequest, mpsc::Sender<Result<UsbSnapshot, String>>),
    Shutdown,
}
pub(crate) struct ModuleUsbService {
    sender: mpsc::SyncSender<Message>,
    closed: Arc<AtomicBool>,
    latest: Arc<Mutex<UsbSnapshot>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl ModuleUsbService {
    pub fn new() -> Self {
        Self::with_factory(Box::new(NativeFactory), Timing::default())
    }
    fn with_factory(factory: Box<dyn Factory>, timing: Timing) -> Self {
        let (sender, receiver) = mpsc::sync_channel(8);
        let closed = Arc::new(AtomicBool::new(false));
        let cancel = closed.clone();
        let latest = Arc::new(Mutex::new(UsbSnapshot::default()));
        let cache = latest.clone();
        let worker = thread::Builder::new()
            .name("osg-module-usb".into())
            .spawn(move || {
                let mut module = Worker::new(factory, timing, cancel.clone());
                while !cancel.load(Ordering::Acquire) {
                    match receiver.recv_timeout(module.timing.refresh) {
                        Ok(Message::Request(request, reply)) => {
                            let result = module.handle(request);
                            module.snapshot.error = result.as_ref().err().cloned();
                            *cache.lock().unwrap_or_else(|p| p.into_inner()) =
                                module.snapshot.clone();
                            let _ = reply.send(result.map(|_| module.snapshot.clone()));
                        }
                        Ok(Message::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if module.snapshot.connected {
                                let result = module.refresh();
                                module.snapshot.error = result.err();
                                *cache.lock().unwrap_or_else(|p| p.into_inner()) =
                                    module.snapshot.clone();
                            }
                        }
                    }
                }
                module.close();
                *cache.lock().unwrap_or_else(|p| p.into_inner()) = module.snapshot.clone();
                while let Ok(Message::Request(_, reply)) = receiver.try_recv() {
                    let _ = reply.send(Err(CLOSED.into()));
                }
            });
        let worker = match worker {
            Ok(worker) => Some(worker),
            Err(error) => {
                closed.store(true, Ordering::Release);
                latest.lock().unwrap_or_else(|p| p.into_inner()).error =
                    Some(format!("USB module worker could not start: {error}"));
                None
            }
        };
        Self {
            sender,
            closed,
            latest,
            worker: Mutex::new(worker),
        }
    }
    /// Cached observations only: this never opens a port or claims anything.
    pub fn snapshot(&self) -> UsbSnapshot {
        self.latest
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn request(&self, request: UsbRequest) -> Result<UsbSnapshot, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CLOSED.into());
        }
        let (sender, receiver) = mpsc::channel();
        self.sender
            .try_send(Message::Request(request, sender))
            .map_err(|error| {
                match error {
                    mpsc::TrySendError::Full(_) => {
                        "The USB module is busy; wait for the current request."
                    }
                    mpsc::TrySendError::Disconnected(_) => CLOSED,
                }
                .to_owned()
            })?;
        receiver.recv().map_err(|_| CLOSED.to_owned())?
    }
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Shutdown);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Drop for ModuleUsbService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
#[path = "module_usb_tests.rs"]
mod tests;

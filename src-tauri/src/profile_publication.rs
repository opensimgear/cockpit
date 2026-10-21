//! Native CAN-only publication ownership. Portable enabled choices never carry
//! a physical target, lease, owner or recovery command across IPC.
use crate::{
    gateways::ProfilePanelIo,
    profile_session::{OwnedRun, Receipt, RunOwner},
};
use osg_profiles::{
    compiler_models::{CompileSnapshot, ExecutionPlan},
    resources::Transport,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
pub(crate) const KIND: &str = "controller-publication-lease";
const INVALID: &str = "The exact native CAN publication target changed; capture a fresh profile.";
const MAX_GATEWAYS: usize = 8;
static WORKERS: AtomicUsize = AtomicUsize::new(0);
fn uint(value: &Value) -> Result<u32, String> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(INVALID.into())
}
fn positive(value: &Value) -> Result<u32, String> {
    uint(value).and_then(|n| if n > 0 { Ok(n) } else { Err(INVALID.into()) })
}
fn capable(state: &Value) -> bool {
    state["gatewayType"] == "stm32-can-gateway"
        && state["protocolVersion"] == 3
        && state["canProtocol"] == 3
        && state["controllerPublicationOverride"]
            == json!({"schema":1,"scope":"gateway-can","nonpersistent":true,"captureAtClaim":true,"restore":"compare-if-unchanged","minLeaseMs":250,"maxLeaseMs":5000,"maxTargets":32,"reconnectRequired":true})
}
fn gateway_identity(state: &Value) -> Option<String> {
    let uid = state["gatewaySerial"].as_str()?;
    (uid.len() == 24
        && uid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && !uid.bytes().all(|b| b == b'0')
        && !uid.bytes().all(|b| b == b'f'))
    .then(|| format!("stm32:{uid}"))
}
pub(crate) fn advertised(state: &Value, module: &Value) -> Option<Value> {
    if !capable(state)
        || gateway_identity(state).is_none()
        || positive(&state["publicationRevision"]).is_err()
        || state["connected"] != true
        || module["online"] != true
        || module["directUsb"] != false
        || positive(&module["boot"]).is_err()
        || positive(&module["configurationGeneration"]).is_err()
        || module["publicationSlot"]
            .as_u64()
            .is_none_or(|slot| slot >= 32)
        || module["publicationReportId"]
            .as_u64()
            .is_none_or(|id| id == 0 || id > 32)
        || !module["configuredEnabled"].is_boolean()
    {
        return None;
    }
    Some(
        json!({"schema":1,"epoch":state["epoch"],"revision":state["publicationRevision"],"module":{"id":module["id"],"boot":module["boot"],"configurationGeneration":module["configurationGeneration"],"node":module["node"],"sourceNode":module["sourceNode"],"profile":module["profile"],"publicationSlot":module["publicationSlot"],"configuredEnabled":module["configuredEnabled"],"publicationReportId":module["publicationReportId"],"configuredDescriptorIndex":module["configuredDescriptorIndex"]}}),
    )
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Target {
    pub resource: String,
    pub inventory: String,
    pub gateway: String,
    pub module: String,
    pub epoch: u32,
    pub boot: u32,
    pub configuration: u32,
    pub node: u32,
    pub source: u32,
    pub profile: Value,
    pub slot: u8,
    pub configured_enabled: bool,
    pub report: u8,
    pub order: i32,
    pub enabled: bool,
}
fn valid_target(t: &Target) -> bool {
    let uid = |text: &str, len: usize| {
        text.len() == len
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && !text.bytes().all(|b| b == b'0')
            && !text.bytes().all(|b| b == b'f')
    };
    let p = &t.profile;
    let axes = p["axes"].as_u64();
    !t.resource.is_empty()
        && !t.inventory.is_empty()
        && t.resource.len() <= 128
        && t.inventory.len() <= 128
        && t.gateway.strip_prefix("stm32:").is_some_and(|s| uid(s, 24))
        && uid(&t.module, 32)
        && t.epoch > 0
        && t.boot > 0
        && t.configuration > 0
        && (1..=32).contains(&t.node)
        && t.source <= 127
        && t.slot < 32
        && (1..=32).contains(&t.report)
        && (if t.configured_enabled {
            (0..32).contains(&t.order)
        } else {
            t.order == -1
        })
        && p.as_object().is_some_and(|o| {
            o.len() == 6
                && [
                    "revision",
                    "buttons",
                    "axes",
                    "hats",
                    "axisUsage",
                    "neutral",
                ]
                .iter()
                .all(|key| o.contains_key(*key))
        })
        && p["revision"]
            .as_u64()
            .is_some_and(|v| (1..=65535).contains(&v))
        && p["buttons"].as_u64().is_some_and(|v| v <= 128)
        && axes.is_some_and(|v| v <= 8)
        && p["hats"].as_u64().is_some_and(|v| v <= 4)
        && p["buttons"].as_u64().unwrap_or(0) + axes.unwrap_or(0) + p["hats"].as_u64().unwrap_or(0)
            > 0
        && p["neutral"].as_array().is_some_and(|a| {
            Some(a.len() as u64) == axes && a.iter().all(|v| v.as_u64().is_some_and(|v| v <= 65535))
        })
        && p["axisUsage"].as_array().is_some_and(|a| {
            Some(a.len() as u64) == axes
                && a.iter()
                    .all(|v| v.as_u64().is_some_and(|v| (0x30..=0x38).contains(&v)))
                && a.iter()
                    .map(Value::to_string)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == a.len()
        })
}
fn valid_targets(targets: &[Target]) -> bool {
    let mut slots = BTreeSet::new();
    let mut modules = BTreeSet::new();
    let mut resources = BTreeSet::new();
    let mut inventories = BTreeSet::new();
    !targets.is_empty()
        && targets.len() <= 64
        && targets.iter().all(|t| {
            valid_target(t)
                && slots.insert((&t.gateway, t.slot))
                && modules.insert((&t.gateway, &t.module))
                && resources.insert(&t.resource)
                && inventories.insert(&t.inventory)
        })
}
#[derive(Clone, Default)]
pub(crate) struct Staged {
    pub targets: Vec<Target>,
}
impl Staged {
    pub(crate) fn prepare(
        plan: &ExecutionPlan,
        snapshot: &CompileSnapshot,
    ) -> Result<Self, String> {
        if plan.runtime_overrides.len() > 64 {
            return Err("A profile supports at most 64 publication choices, with at most 32 exact targets per gateway.".into());
        }
        if !plan.runtime_overrides.is_empty() && snapshot.read_only {
            return Err(INVALID.into());
        }
        let mut targets = vec![];
        let mut physical = BTreeSet::new();
        for intent in &plan.runtime_overrides {
            let pin=snapshot.assignments.iter().find(|a|a.resource_id==intent.resource_id).ok_or("Bind an exact native CAN controller before requesting its temporary publication.")?;
            let resource = snapshot
                .resources
                .iter()
                .find(|r| r.summary.id == pin.source_id);
            // An absent peripheral has no publication authority to acquire.
            // Reconnection goes through a fresh journaled profile preparation.
            let Some(resource) = resource else { continue; };
            if resource.summary.availability == osg_profiles::resources::Availability::Offline { continue; }
            let metadata:Value=serde_json::from_str(resource.summary.attributes.get("controllerPublicationOverride").ok_or("This resource has no advertised nonpersistent publication lease. Direct USB, Pico and older CAN firmware remain unsupported; no persistent device setting was changed.")?).map_err(|_|INVALID)?;
            let routes = resource
                .summary
                .connections
                .iter()
                .filter(|c| c.route.transport == Transport::Can && c.route.writable)
                .collect::<Vec<_>>();
            let gateway = resource
                .summary
                .attributes
                .get("gatewayId")
                .ok_or(INVALID)?
                .clone();
            let module = resource
                .summary
                .attributes
                .get("moduleId")
                .ok_or(INVALID)?
                .clone();
            let m = &metadata["module"];
            if metadata["schema"] != 1
                || routes.len() != 1
                || resource.summary.connections.iter().any(|c| {
                    matches!(c.route.transport, Transport::Usb | Transport::Hid)
                        || c.route.writable && c.route.id != routes[0].route.id
                })
                || routes[0].route.id != format!("can:{gateway}:{module}")
                || m["id"] != module
                || !physical.insert((gateway.clone(), module.clone()))
            {
                return Err(INVALID.into());
            }
            targets.push(Target {
                resource: intent.resource_id.clone(),
                inventory: resource.summary.id.clone(),
                gateway,
                module,
                epoch: positive(&metadata["epoch"])?,
                boot: positive(&m["boot"])?,
                configuration: positive(&m["configurationGeneration"])?,
                node: positive(&m["node"])?,
                source: uint(&m["sourceNode"])?,
                profile: m["profile"].clone(),
                slot: u8::try_from(uint(&m["publicationSlot"])?)
                    .ok()
                    .filter(|slot| *slot < 32)
                    .ok_or(INVALID)?,
                configured_enabled: m["configuredEnabled"].as_bool().ok_or(INVALID)?,
                report: u8::try_from(positive(&m["publicationReportId"])?).map_err(|_| INVALID)?,
                order: i32::try_from(m["configuredDescriptorIndex"].as_i64().ok_or(INVALID)?)
                    .map_err(|_| INVALID)?,
                enabled: intent.enabled,
            });
        }
        if targets
            .iter()
            .map(|t| &t.gateway)
            .collect::<BTreeSet<_>>()
            .len()
            > MAX_GATEWAYS
        {
            return Err("Too many composite publication gateways.".into());
        }
        if !targets.is_empty() && !valid_targets(&targets) {
            return Err(INVALID.into());
        }
        Ok(Self { targets })
    }
}
pub(crate) fn matches_target(t: &Target, state: &Value) -> bool {
    capable(state)
        && gateway_identity(state).as_deref() == Some(t.gateway.as_str())
        && state["connected"] == true
        && state["epoch"] == t.epoch
        && state["modules"]
            .as_array()
            .and_then(|m| m.iter().find(|m| m["id"] == t.module))
            .is_some_and(|m| {
                m["online"] == true
                    && m["directUsb"] == false
                    && m["boot"] == t.boot
                    && m["configurationGeneration"] == t.configuration
                    && m["node"] == t.node
                    && m["sourceNode"] == t.source
                    && m["profile"] == t.profile
                    && m["publicationSlot"] == t.slot
                    && m["configuredEnabled"] == t.configured_enabled
                    && m["publicationReportId"] == t.report
                    && m["configuredDescriptorIndex"] == t.order
            })
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cleanup {
    schema: u32,
    owner: RunOwner,
    gateway: String,
    epoch: u32,
    revision: u32,
    target_mask: u32,
    enabled_mask: u32,
    secret: String,
    targets: Vec<Target>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    schema: u32,
    sealed_hex: String,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() > 65536 || text.len() % 2 != 0 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(INVALID.into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| INVALID.into()))
        .collect()
}
fn sealed(value: &Cleanup) -> Result<Receipt, String> {
    let mut plain = serde_json::to_vec(value).map_err(|_| INVALID)?;
    let result = osg_companion_service::persistence::seal(&plain);
    plain.fill(0);
    Ok(Receipt {
        kind: KIND.into(),
        resource_id: value.gateway.clone(),
        state: serde_json::to_value(Envelope {
            schema: 1,
            sealed_hex: hex(&result?),
        })
        .map_err(|_| INVALID)?,
    })
}
fn open_receipt(receipt: &Receipt, owner: &RunOwner) -> Result<Cleanup, String> {
    let envelope: Envelope = serde_json::from_value(receipt.state.clone()).map_err(|_| INVALID)?;
    if receipt.kind != KIND || envelope.schema != 1 {
        return Err(INVALID.into());
    }
    let mut plain = osg_companion_service::persistence::unseal(&unhex(&envelope.sealed_hex)?)?;
    let decoded = serde_json::from_slice::<Cleanup>(&plain).map_err(|_| INVALID);
    plain.fill(0);
    let value = decoded?;
    if value.schema != 1
        || value.owner != *owner
        || owner.generation == 0
        || uuid::Uuid::parse_str(&owner.session).is_err()
        || value.gateway != receipt.resource_id
        || value.revision == 0
        || value.epoch == 0
        || !valid_targets(&value.targets)
        || value
            .targets
            .iter()
            .any(|t| t.gateway != value.gateway || t.epoch != value.epoch)
        || value.secret.len() != 32
        || !value.secret.bytes().all(|b| b.is_ascii_hexdigit())
        || value.secret.bytes().all(|b| b == b'0')
        || value.secret.bytes().all(|b| b.eq_ignore_ascii_case(&b'f'))
    {
        return Err(INVALID.into());
    }
    let target = value
        .targets
        .iter()
        .fold(0u32, |mask, t| mask | (1u32 << t.slot));
    let enabled = value
        .targets
        .iter()
        .filter(|t| t.enabled)
        .fold(0u32, |mask, t| mask | (1u32 << t.slot));
    if target == 0 || target != value.target_mask || enabled != value.enabled_mask {
        return Err(INVALID.into());
    }
    Ok(value)
}
trait Io: Send + Sync {
    fn cached(&self, gateway: &str) -> Result<Value, String>;
    fn request(
        &self,
        gateway: &str,
        request: crate::stm32::PublicationRequest,
    ) -> Result<crate::stm32::PublicationReply, String>;
}
struct NativeIo {
    panels: Arc<ProfilePanelIo>,
    control_epoch: Option<(Arc<AtomicU64>, u64)>,
}
impl Io for NativeIo {
    fn cached(&self, g: &str) -> Result<Value, String> {
        self.panels.can_snapshot(g)
    }
    fn request(
        &self,
        g: &str,
        mut r: crate::stm32::PublicationRequest,
    ) -> Result<crate::stm32::PublicationReply, String> {
        if matches!(
            r.operation,
            crate::stm32::PublicationOperation::Claim { .. }
                | crate::stm32::PublicationOperation::Renew { .. }
        ) {
            r.control_epoch = self.control_epoch.clone();
        }
        self.panels.publication(g, r)
    }
}
#[derive(Clone, Default)]
pub(crate) struct Status {
    pub active: bool,
    pub stopped: bool,
    pub cleanup_complete: bool,
    pub error: Option<String>,
}
struct Shared {
    start: AtomicBool,
    withdrawn: AtomicBool,
    authority: Arc<AtomicBool>,
    stop: AtomicBool,
    status: Mutex<Status>,
    verified: Mutex<Option<(Instant, Value)>>,
}
fn shared(recovery: bool) -> Arc<Shared> {
    Arc::new(Shared {
        start: AtomicBool::new(false),
        withdrawn: AtomicBool::new(false),
        authority: Arc::new(AtomicBool::new(false)),
        stop: AtomicBool::new(recovery),
        status: Mutex::new(Status::default()),
        verified: Mutex::new(None),
    })
}
struct Slot {
    value: Cleanup,
    receipt: Receipt,
    shared: Arc<Shared>,
    spawned: bool,
}
pub(crate) struct Overrides {
    slots: Vec<Slot>,
    io: Arc<dyn Io>,
    recovery: bool,
}
impl Overrides {
    pub(crate) fn prepare(
        staged: &Staged,
        owner: &RunOwner,
        panels: Arc<ProfilePanelIo>,
        epoch: Arc<AtomicU64>,
        expected: u64,
    ) -> Result<Self, String> {
        Self::prepare_io(
            staged,
            owner,
            Arc::new(NativeIo {
                panels,
                control_epoch: Some((epoch, expected)),
            }),
        )
    }
    fn prepare_io(staged: &Staged, owner: &RunOwner, io: Arc<dyn Io>) -> Result<Self, String> {
        if !staged.targets.is_empty() && !valid_targets(&staged.targets) {
            return Err(INVALID.into());
        }
        let mut grouped: BTreeMap<String, Vec<Target>> = BTreeMap::new();
        for t in &staged.targets {
            grouped
                .entry(t.gateway.clone())
                .or_default()
                .push(t.clone());
        }
        let mut slots = vec![];
        for (gateway, targets) in grouped {
            let state = io.cached(&gateway)?;
            if targets.iter().any(|t| !matches_target(t, &state)) {
                return Err(INVALID.into());
            }
            let value = Cleanup {
                schema: 1,
                owner: owner.clone(),
                gateway,
                epoch: positive(&state["epoch"])?,
                revision: positive(&state["publicationRevision"])?,
                target_mask: targets.iter().fold(0, |m, t| m | (1u32 << t.slot)),
                enabled_mask: targets
                    .iter()
                    .filter(|t| t.enabled)
                    .fold(0, |m, t| m | (1u32 << t.slot)),
                secret: uuid::Uuid::new_v4().simple().to_string(),
                targets,
            };
            let receipt = sealed(&value)?;
            slots.push(Slot {
                value,
                receipt,
                shared: shared(false),
                spawned: false,
            });
        }
        Ok(Self {
            slots,
            io,
            recovery: false,
        })
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    pub(crate) fn needs_cleanup(&self) -> bool {
        self.slots.iter().any(|s| {
            s.spawned
                && !s
                    .shared
                    .status
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .cleanup_complete
        })
    }
    pub(crate) fn receipts(&self) -> Vec<Receipt> {
        self.slots.iter().map(|s| s.receipt.clone()).collect()
    }
    pub(crate) fn targets(&self) -> Vec<Target> {
        self.slots
            .iter()
            .flat_map(|s| s.value.targets.clone())
            .collect()
    }
    /// Called only after outgoing owned cleanup. The logical target tuple stays
    /// immutable; only the firmware snapshot revision is recaptured and sealed.
    pub(crate) fn recapture(&mut self) -> Result<Vec<Receipt>, String> {
        if self.slots.iter().any(|s| s.spawned) {
            return Err(INVALID.into());
        }
        for slot in &mut self.slots {
            let state = self.io.cached(&slot.value.gateway)?;
            if state["publication"]["phase"] != "idle"
                || state["layoutPending"] != false
                || slot
                    .value
                    .targets
                    .iter()
                    .any(|t| !matches_target(t, &state))
            {
                return Err(INVALID.into());
            }
            slot.value.revision = positive(&state["publicationRevision"])?;
            slot.receipt = sealed(&slot.value)?;
        }
        Ok(self.receipts())
    }
    pub(crate) fn from_run(run: &OwnedRun, io: Arc<ProfilePanelIo>) -> Result<Self, String> {
        let mut slots = vec![];
        let mut gateways = BTreeSet::new();
        for receipt in run.receipts.iter().filter(|r| r.kind == KIND) {
            let value = open_receipt(receipt, &run.owner)?;
            if !gateways.insert(value.gateway.clone()) || gateways.len() > MAX_GATEWAYS {
                return Err(INVALID.into());
            }
            slots.push(Slot {
                value,
                receipt: receipt.clone(),
                shared: shared(true),
                spawned: false,
            });
        }
        Ok(Self {
            slots,
            io: Arc::new(NativeIo {
                panels: io,
                control_epoch: None,
            }),
            recovery: true,
        })
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        for slot in &self.slots {
            let state = self.io.cached(&slot.value.gateway)?;
            if slot
                .value
                .targets
                .iter()
                .any(|t| !matches_target(t, &state))
            {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }
    fn spawn(&mut self) -> Result<(), String> {
        let count = self.slots.iter().filter(|s| !s.spawned).count();
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(count).filter(|n| *n <= MAX_GATEWAYS)
            })
            .map_err(|_| "Composite publication cleanup still owns the bounded worker capacity.")?;
        for slot in self.slots.iter_mut().filter(|s| !s.spawned) {
            let io = self.io.clone();
            let value = slot.value.clone();
            let shared = slot.shared.clone();
            let recovery = self.recovery;
            slot.spawned = true;
            thread::spawn(move || {
                struct Permit;
                impl Drop for Permit {
                    fn drop(&mut self) {
                        WORKERS.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                let _permit = Permit;
                drive(io, value, shared, recovery);
            });
        }
        Ok(())
    }
    /// Durable journal ownership is required before the Host calls this method.
    pub(crate) fn acquire(&mut self) -> Result<(), String> {
        if self.recovery
            || self.slots.iter().any(|s| {
                s.spawned
                    || s.shared.withdrawn.load(Ordering::Acquire)
                    || s.shared.stop.load(Ordering::Acquire)
            })
        {
            return Err(
                "A stopped publication receipt is cleanup-only; capture a fresh Apply.".into(),
            );
        }
        self.spawn()?;
        for slot in &self.slots {
            slot.shared.authority.store(true, Ordering::Release);
            slot.shared.start.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub(crate) fn poll(&self) -> Status {
        let mut result = Status {
            active: true,
            stopped: true,
            cleanup_complete: true,
            error: None,
        };
        for slot in &self.slots {
            let s = slot
                .shared
                .status
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            result.active &= s.active;
            result.stopped &= s.stopped;
            result.cleanup_complete &= s.cleanup_complete;
            if result.error.is_none() {
                result.error = s.error;
            }
        }
        result
    }
    /// Withdraw productive authority immediately without releasing the gateway
    /// overlay before the Host has restored physical action owners beneath it.
    pub(crate) fn revoke_renewal(&self) {
        for slot in &self.slots {
            slot.shared.withdrawn.store(true, Ordering::Release);
            slot.shared.authority.store(false, Ordering::Release);
            // Preserve a completed immutable restoration snapshot across
            // repeated cleanup polling. Live proof still requires authority.
            slot.shared
                .status
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .active = false;
        }
    }
    pub(crate) fn stop(&mut self) -> Result<Status, String> {
        self.revoke_renewal();
        let retry = self.slots.iter().any(|slot| {
            let status = slot.shared.status.lock().unwrap_or_else(|p| p.into_inner());
            slot.spawned && status.stopped && !status.cleanup_complete
        });
        if retry {
            self.recovery = true;
        }
        for slot in &mut self.slots {
            let failed = {
                let status = slot.shared.status.lock().unwrap_or_else(|p| p.into_inner());
                slot.spawned && status.stopped && !status.cleanup_complete
            };
            if failed {
                // Explicit Stop/Recover can retry a finished bounded cleanup
                // after transport returns. Reuse only the sealed private
                // receipt; the new worker cannot Claim or Renew, and must
                // inspect the exact existing private lease before any Release.
                slot.shared = shared(true);
                slot.spawned = false;
            }
            slot.shared.authority.store(false, Ordering::Release);
            slot.shared.stop.store(true, Ordering::Release);
            // A prepared actor with no dispatched worker cannot own any effect.
            // Changed hardware must not make discarding that inert preparation
            // invent cleanup authority or block unrelated restoration.
            if !slot.spawned && !self.recovery {
                *slot.shared.status.lock().unwrap_or_else(|p| p.into_inner()) = Status {
                    active: false,
                    stopped: true,
                    cleanup_complete: true,
                    error: None,
                };
            }
        }
        if self.recovery || self.slots.iter().any(|s| s.spawned) {
            self.spawn()?;
        }
        Ok(self.poll())
    }
}
impl Drop for Overrides {
    fn drop(&mut self) {
        for slot in &self.slots {
            slot.shared.authority.store(false, Ordering::Release);
            slot.shared.stop.store(true, Ordering::Release);
        }
    }
}
fn request(
    io: &dyn Io,
    value: &Cleanup,
    operation: crate::stm32::PublicationOperation,
    authority: Option<Arc<AtomicBool>>,
) -> Result<crate::stm32::PublicationReply, String> {
    io.request(
        &value.gateway,
        crate::stm32::PublicationRequest {
            epoch: value.epoch,
            operation,
            authority,
            control_epoch: None,
        },
    )
}
fn candidate(value: &Cleanup, state: &Value) -> bool {
    let p = &state["publication"];
    state["epoch"] == value.epoch
        && p["claimRevision"] == value.revision
        && p["targetMask"] == format!("{:08x}", value.target_mask)
        && p["enabledMask"] == format!("{:08x}", value.enabled_mask)
        && matches!(
            p["phase"].as_str(),
            Some("owned" | "restoring" | "attention")
        )
}
fn same_publication(a: &Value, b: &Value) -> bool {
    [
        "phase",
        "outcome",
        "leaseId",
        "transition",
        "claimRevision",
        "targetMask",
        "enabledMask",
        "expectedReconnect",
    ]
    .iter()
    .all(|key| a["publication"][key] == b["publication"][key])
}
fn module_delivery<'a>(target: &Target, state: &'a Value) -> Option<(&'a Value, &'a Value)> {
    state["modules"]
        .as_array()?
        .iter()
        .find(|module| module["id"] == target.module)
        .map(|module| (&module["lease"], &module["published"]))
}
enum Inspection {
    Absent(Value),
    Foreign(Value),
    Owned(u32, Value),
}
fn inspect(io: &dyn Io, value: &Cleanup, state: &Value) -> Result<Inspection, String> {
    if gateway_identity(state).as_deref() != Some(value.gateway.as_str()) {
        return Err(INVALID.into());
    }
    if state["epoch"] != value.epoch
        || !matches!(
            state["publication"]["phase"].as_str(),
            Some("owned" | "restoring" | "attention")
        )
    {
        return Ok(Inspection::Absent(state.clone()));
    }
    let lease = positive(&state["publication"]["leaseId"])?;
    let reply = request(
        io,
        value,
        crate::stm32::PublicationOperation::Inspect {
            owner: value.secret.clone(),
            lease,
        },
        None,
    )?;
    if gateway_identity(&reply.state).as_deref() != Some(value.gateway.as_str())
        || reply.state["epoch"] != value.epoch
    {
        return Err(INVALID.into());
    }
    if reply.owned == Some(false) {
        if reply.state["publication"]["phase"] == "idle" {
            return Ok(Inspection::Absent(reply.state));
        }
        return Ok(Inspection::Foreign(reply.state));
    }
    if reply.owned != Some(true)
        || !candidate(value, &reply.state)
        || reply.state["publication"]["leaseId"] != lease
    {
        return Err(INVALID.into());
    }
    Ok(Inspection::Owned(lease, reply.state))
}
fn settled(value: &Cleanup, state: &Value) -> bool {
    state["publication"]["phase"] == "owned"
        && state["publication"]["outcome"] == "ok"
        && state["layoutPending"] == false
        && state["publication"]["expectedReconnect"] == false
        && value.targets.iter().all(|t| {
            matches_target(t, state)
                && state["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["id"] == t.module)
                    .is_some_and(|m| m["published"] == t.enabled && m["hasInput"] == true)
        })
}
fn confirmed_restored(value: &Cleanup, state: &Value) -> bool {
    state["publication"]["phase"] == "idle"
        && state["publication"]["outcome"] == "no_change"
        && state["layoutPending"] == false
        && value.targets.iter().all(|t| {
            matches_target(t, state)
                && state["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["id"] == t.module)
                    .is_some_and(|m| m["published"] == t.configured_enabled)
        })
}
fn drive(io: Arc<dyn Io>, value: Cleanup, shared: Arc<Shared>, recovery: bool) {
    let update = |status: Status| *shared.status.lock().unwrap_or_else(|p| p.into_inner()) = status;
    while !shared.start.load(Ordering::Acquire) && !shared.stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(5));
    }
    let mut error = None;
    let mut state = io.cached(&value.gateway).unwrap_or(Value::Null);
    let mut released = false;
    if !recovery && !shared.stop.load(Ordering::Acquire) && shared.authority.load(Ordering::Acquire)
    {
        if state["publicationRevision"] != value.revision
            || state["publication"]["phase"] != "idle"
            || state["layoutPending"] != false
            || value.targets.iter().any(|t| !matches_target(t, &state))
        {
            error = Some(INVALID.into());
        } else {
            match request(
                io.as_ref(),
                &value,
                crate::stm32::PublicationOperation::Claim {
                    owner: value.secret.clone(),
                    revision: value.revision,
                    target_mask: value.target_mask,
                    enabled_mask: value.enabled_mask,
                    ttl_ms: 5000,
                },
                Some(shared.authority.clone()),
            ) {
                Ok(reply) => state = reply.state,
                Err(e) => error = Some(e),
            }
        }
    }
    let started = Instant::now();
    let mut renewed = Instant::now();
    let mut cleanup_started = None;
    let mut active = false;
    let mut delivery_lease: Option<Vec<Value>> = None;
    loop {
        match request(
            io.as_ref(),
            &value,
            crate::stm32::PublicationOperation::State,
            None,
        ) {
            Ok(reply) => state = reply.state,
            Err(e) => {
                if error.is_none() {
                    error = Some(e);
                }
            }
        }
        let lease = match inspect(io.as_ref(), &value, &state) {
            Ok(Inspection::Owned(lease, proof)) => {
                state = proof;
                Some(lease)
            }
            Ok(Inspection::Absent(current)) => {
                state = current;
                None
            }
            Ok(Inspection::Foreign(_state)) => {
                shared.authority.store(false, Ordering::Release);
                shared
                    .verified
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take();
                update(Status {
                    active: false,
                    stopped: true,
                    cleanup_complete: true,
                    error: Some(
                        "An independently owned publication was verified and preserved.".into(),
                    ),
                });
                return;
            }
            Err(e) => {
                if error.is_none() {
                    error = Some(e);
                }
                None
            }
        };
        let stopping = recovery
            || shared.stop.load(Ordering::Acquire)
            || !shared.authority.load(Ordering::Acquire)
            || error.is_some();
        if stopping {
            if !recovery && !shared.stop.load(Ordering::Acquire) && error.is_none() {
                error = Some("The native publication authority was withdrawn; only ordered restoration remains available.".into());
            }
            shared.authority.store(false, Ordering::Release);
            shared
                .verified
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            active = false;
            let since = *cleanup_started.get_or_insert_with(Instant::now);
            if let Some(lease) = lease {
                if !released && (recovery || shared.stop.load(Ordering::Acquire)) {
                    released = true;
                    if let Err(e) = request(
                        io.as_ref(),
                        &value,
                        crate::stm32::PublicationOperation::Release {
                            owner: value.secret.clone(),
                            lease,
                        },
                        None,
                    ) {
                        if error.is_none() {
                            error = Some(e);
                        }
                    }
                }
            } else if gateway_identity(&state).as_deref() == Some(value.gateway.as_str())
                && state["connected"] == true
                && state["epoch"] == value.epoch
                && state["publication"]["phase"] == "idle"
                && state["layoutPending"] == false
            {
                let pristine = state["publication"]["outcome"] == "ok"
                    && state["publication"]["leaseId"] == 0
                    && value.targets.iter().all(|t| matches_target(t, &state));
                if state["publication"]["outcome"] == "preserved" {
                    error=Some("An independently changed publication was preserved; its original layout was not restored.".into());
                    update(Status {
                        active: false,
                        stopped: true,
                        cleanup_complete: true,
                        error,
                    });
                    return;
                }
                if confirmed_restored(&value, &state) || pristine {
                    // Acquisition failure remains visible until the original
                    // owned state is positively confirmed restored. It cannot
                    // turn a completed restoration into a permanent deadlock.
                    if confirmed_restored(&value, &state) {
                        error = None;
                    }
                    if error.is_none() {
                        *shared.verified.lock().unwrap_or_else(|p| p.into_inner()) =
                            Some((Instant::now(), state.clone()));
                    }
                    update(Status {
                        active: false,
                        stopped: true,
                        cleanup_complete: true,
                        error,
                    });
                    return;
                }
                if error.is_none() {
                    error=Some("Publication restoration has no exact settled readback of the sealed original targets; recovery remains pending.".into());
                }
            }
            if since.elapsed() > Duration::from_secs(8) {
                update(Status{active:false,stopped:true,cleanup_complete:false,error:Some(error.unwrap_or("Publication restoration remains unverified; retain its sealed recovery receipt.".into()))});
                return;
            }
        } else if lease.is_some() && settled(&value, &state) {
            let leases = value
                .targets
                .iter()
                .map(|t| {
                    state["modules"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|m| m["id"] == t.module)
                        .unwrap()["lease"]
                        .clone()
                })
                .collect::<Vec<_>>();
            if delivery_lease.as_ref().is_some_and(|old| old != &leases) {
                error=Some("The admitted CAN delivery lease changed independently of the owned publication.".into());
                shared.authority.store(false, Ordering::Release);
                shared
                    .verified
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take();
                active = false;
                update(Status {
                    active: false,
                    stopped: false,
                    cleanup_complete: false,
                    error: error.clone(),
                });
                // Do not publish a replacement proof or enter a potentially
                // blocking renewal after observing an independent route.
                continue;
            } else {
                delivery_lease = Some(leases);
            }
            active = true;
            *shared.verified.lock().unwrap_or_else(|p| p.into_inner()) =
                Some((Instant::now(), state.clone()));
            if renewed.elapsed() >= Duration::from_secs(1) {
                match request(
                    io.as_ref(),
                    &value,
                    crate::stm32::PublicationOperation::Renew {
                        owner: value.secret.clone(),
                        lease: lease.unwrap(),
                        ttl_ms: 5000,
                    },
                    Some(shared.authority.clone()),
                ) {
                    Ok(reply) if settled(&value, &reply.state) => renewed = Instant::now(),
                    Ok(_) => error = Some(INVALID.into()),
                    Err(e) => error = Some(e),
                }
            }
        } else if active || started.elapsed() > Duration::from_secs(3) {
            active = false;
            error =
                Some("The temporary CAN publication lost its exact settled native owner.".into());
        }
        if error.is_some() {
            shared.authority.store(false, Ordering::Release);
            active = false;
        }
        update(Status {
            active,
            stopped: false,
            cleanup_complete: false,
            error: error.clone(),
        });
        thread::sleep(Duration::from_millis(20));
    }
}

/// Only this native actor constructs an acquisition refresh permission. It is
/// bound to either current private-owner proof or completed exact restoration.
pub(crate) struct VerifiedPublication {
    targets: Vec<Target>,
    io: Arc<dyn Io>,
    states: BTreeMap<String, Value>,
    live: BTreeMap<String, Option<Arc<Shared>>>,
}

impl VerifiedPublication {
    pub(crate) fn targets(&self) -> &[Target] {
        &self.targets
    }
    pub(crate) fn delivery_lease(&self, target: &Target) -> Option<u64> {
        self.states.get(&target.gateway)?["modules"]
            .as_array()?
            .iter()
            .find(|m| m["id"] == target.module)?["lease"]
            .as_u64()
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        for (gateway, observed) in &self.states {
            if let Some(shared) = self.live.get(gateway).ok_or(INVALID)? {
                let status = shared.status.lock().unwrap_or_else(|p| p.into_inner());
                if !shared.authority.load(Ordering::Acquire)
                    || !status.active
                    || status.error.is_some()
                {
                    return Err(INVALID.into());
                }
                drop(status);
                let verified = shared.verified.lock().unwrap_or_else(|p| p.into_inner());
                if verified.as_ref().is_none_or(|(at, current)| {
                    at.elapsed() > Duration::from_millis(500)
                        || !same_publication(current, observed)
                }) {
                    return Err(INVALID.into());
                }
            } else if observed["publication"]["phase"] != "idle"
                || observed["publication"]["outcome"] != "no_change"
            {
                return Err(INVALID.into());
            }
            let current = self.io.cached(gateway)?;
            if current["epoch"] != observed["epoch"]
                || !same_publication(&current, observed)
                || current["layoutPending"] != false
                || self
                    .targets
                    .iter()
                    .filter(|t| &t.gateway == gateway)
                    .any(|t| {
                        !matches_target(t, &current)
                            || current["modules"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .find(|m| m["id"] == t.module)
                                .map(|m| (&m["lease"], &m["published"]))
                                != observed["modules"]
                                    .as_array()
                                    .and_then(|m| m.iter().find(|m| m["id"] == t.module))
                                    .map(|m| (&m["lease"], &m["published"]))
                    })
            {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }
}
impl Overrides {
    pub(crate) fn verified(&self, restored: bool) -> Result<VerifiedPublication, String> {
        let status = self.poll();
        if status.error.is_some()
            || !(if restored {
                status.stopped && status.cleanup_complete
            } else {
                status.active
            })
        {
            return Err("Publication acquisition refresh requires exact settled native ownership or its confirmed restoration.".into());
        }
        let mut states = BTreeMap::new();
        for slot in &self.slots {
            let state = self.io.cached(&slot.value.gateway)?;
            let verified = slot
                .shared
                .verified
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let (at, proof) = verified.as_ref().ok_or(INVALID)?;
            if (!restored && at.elapsed() > Duration::from_millis(500))
                || !same_publication(proof, &state)
                || slot
                    .value
                    .targets
                    .iter()
                    .any(|target| module_delivery(target, proof) != module_delivery(target, &state))
            {
                return Err(INVALID.into());
            }
            if restored {
                if !confirmed_restored(&slot.value, &state) {
                    return Err(INVALID.into());
                }
            } else {
                if !slot.shared.authority.load(Ordering::Acquire) || !settled(&slot.value, &state) {
                    return Err(INVALID.into());
                }
            }
            states.insert(slot.value.gateway.clone(), state);
        }
        let proof = VerifiedPublication {
            targets: self.targets(),
            io: self.io.clone(),
            states,
            live: self
                .slots
                .iter()
                .map(|s| {
                    (
                        s.value.gateway.clone(),
                        (!restored).then(|| s.shared.clone()),
                    )
                })
                .collect(),
        };
        proof.validate()?;
        Ok(proof)
    }
}

#[cfg(test)]
#[path = "profile_publication_rebind_tests.rs"]
mod rebind_tests;
#[cfg(test)]
#[path = "profile_publication_tests.rs"]
mod tests;

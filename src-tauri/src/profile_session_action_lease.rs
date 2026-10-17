//! Exact native endpoint leases. This primitive is synchronous and is called
//! only by the bounded profile action worker, never by Host polling or IPC.
//! Its sealed receipt contains cleanup authority, never an invocation tuple.
use crate::{
    gateways::ProfilePanelIo,
    module_usb::{ActionRequest, UsbRequest, UsbRouteToken, UsbSnapshot},
    module_usb_fleet::SlotSnapshot,
    profile_resources::Runtime,
    profile_session::{Receipt, RunOwner},
};
use osg_mapping::program::NamedArgument;
use osg_profiles::{
    compiler_models::{PlanReadiness, SinkAdapter, SinkPlan},
    resources::{Access, PortPreparation, ResourceToken, Route, Transport},
    resources_ports::{PortContract, PortDescriptor},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};

pub(crate) const TTL_MS: u32 = 5_000;
pub(crate) const MAX_ENDPOINTS: usize = 16;
const KIND: &str = "physical-action-lease";
const RECEIPT_BYTES: usize = 8_192;
const INVALID: &str = "The physical output no longer matches its exact native lease.";
const PENDING_ROUTE: &str = "Physical cleanup remains pending: the changed CAN route has no authenticated retired action receipt.";

trait Io: Send + Sync {
    fn preparation(&self, pin: &PortPreparation) -> Result<Route, String>;
    fn delivery(&self, token: &ResourceToken) -> Result<Route, String>;
    fn snapshots(&self) -> Vec<SlotSnapshot>;
    fn can_cached(&self, gateway: &str) -> Result<Value, String>;
    fn usb(&self, key: &str, request: UsbRequest) -> Result<UsbSnapshot, String>;
    fn can(&self, request: Value) -> Value;
}
struct NativeIo {
    resources: Arc<Runtime>,
    panels: Arc<ProfilePanelIo>,
}
impl Io for NativeIo {
    fn preparation(&self, p: &PortPreparation) -> Result<Route, String> {
        self.resources.validate_preparation(p)
    }
    fn delivery(&self, t: &ResourceToken) -> Result<Route, String> {
        self.resources.validate_delivery(t)
    }
    fn snapshots(&self) -> Vec<SlotSnapshot> {
        self.panels.usb_snapshots()
    }
    fn can_cached(&self, g: &str) -> Result<Value, String> {
        self.panels.can_snapshot(g)
    }
    fn usb(&self, k: &str, r: UsbRequest) -> Result<UsbSnapshot, String> {
        self.panels.usb(k, r)
    }
    fn can(&self, r: Value) -> Value {
        self.panels.can(r)
    }
}
trait Protector: Send + Sync {
    fn seal(&self, bytes: &[u8]) -> Result<Vec<u8>, String>;
    fn unseal(&self, bytes: &[u8]) -> Result<Vec<u8>, String>;
}
struct NativeProtector;
impl Protector for NativeProtector {
    fn seal(&self, b: &[u8]) -> Result<Vec<u8>, String> {
        osg_companion_service::persistence::seal(b)
    }
    fn unseal(&self, b: &[u8]) -> Result<Vec<u8>, String> {
        osg_companion_service::persistence::unseal(b)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case", deny_unknown_fields)]
enum Target {
    Usb {
        key: String,
        route: UsbRouteToken,
    },
    Can {
        gateway: String,
        module: String,
        epoch: Value,
        node: Value,
        lease: Value,
        boot: Value,
        configuration: Value,
    },
}
impl Target {
    fn from_route(route: &Route, io: &dyn Io) -> Result<Self, String> {
        match route.transport {
            Transport::Usb => {
                let key = route.id.strip_prefix("usb:").ok_or(INVALID)?;
                let slot = io
                    .snapshots()
                    .into_iter()
                    .find(|s| s.key == key && s.present && s.admissible && s.snapshot.connected)
                    .ok_or(INVALID)?;
                let native = slot.snapshot.route.ok_or(INVALID)?;
                if route.provider_generation
                    != format!(
                        "{}:{}:{}",
                        native.connection_generation, native.boot, native.configuration_generation
                    )
                {
                    return Err(INVALID.into());
                }
                Ok(Self::Usb {
                    key: key.into(),
                    route: native,
                })
            }
            Transport::Can => {
                let v: Vec<Value> =
                    serde_json::from_str(&route.provider_generation).map_err(|_| INVALID)?;
                let attr = |key: &str| route.attributes.get(key).cloned().ok_or(INVALID);
                if v.len() != 5 {
                    return Err(INVALID.into());
                }
                let target = Self::Can {
                    gateway: attr("gatewayId")?,
                    module: attr("moduleId")?,
                    epoch: v[0].clone(),
                    node: serde_json::from_str(&attr("node")?).map_err(|_| INVALID)?,
                    lease: v[2].clone(),
                    boot: v[1].clone(),
                    configuration: v[3].clone(),
                };
                if attr("boot")? != v[1].to_string() {
                    return Err(INVALID.into());
                }
                target.cached(io)?;
                Ok(target)
            }
            _ => Err("Physical actions require an exact native USB or CAN route.".into()),
        }
    }
    fn cache_from_can(&self, state: &Value) -> Result<Value, String> {
        let Self::Can {
            module,
            epoch,
            node,
            lease,
            boot,
            configuration,
            ..
        } = self
        else {
            return Err(INVALID.into());
        };
        let current = state["modules"]
            .as_array()
            .and_then(|a| a.iter().find(|m| m["id"] == *module && m["online"] == true))
            .ok_or(INVALID)?;
        let cache = &state["moduleActions"][module];
        if state["connected"] != true
            || state["epoch"] != *epoch
            || current["node"] != *node
            || current["lease"] != *lease
            || current["boot"] != *boot
            || boot.as_u64().is_none_or(|n| n == 0)
            || current["configurationGeneration"] != *configuration
            || cache["route"] != json!([epoch, node, lease, boot])
            || cache["configurationGeneration"] != *configuration
        {
            return Err(INVALID.into());
        }
        Ok(cache.clone())
    }
    fn cache_from_usb(&self, snapshot: &UsbSnapshot) -> Result<Value, String> {
        let Self::Usb { route, .. } = self else {
            return Err(INVALID.into());
        };
        if !snapshot.connected || snapshot.route.as_ref() != Some(route) {
            return Err(INVALID.into());
        }
        snapshot
            .actions
            .as_ref()
            .filter(|a| a["route"] == json!(route))
            .cloned()
            .ok_or(INVALID.into())
    }
    fn cached(&self, io: &dyn Io) -> Result<Value, String> {
        match self {
            Self::Usb { key, .. } => {
                let slot = io
                    .snapshots()
                    .into_iter()
                    .find(|s| s.key == *key && s.present && s.admissible)
                    .ok_or(INVALID)?;
                self.cache_from_usb(&slot.snapshot)
            }
            Self::Can { gateway, .. } => self.cache_from_can(&io.can_cached(gateway)?),
        }
    }
    fn request(&self, io: &dyn Io, command: Command) -> Result<Value, String> {
        // Recheck the exact endpoint before every worker call; workers also fence
        // boot/configuration and take the shared cross-transport writer gate.
        self.cached(io)?;
        match self {
            Self::Usb { key, route } => self.cache_from_usb(&io.usb(
                key,
                UsbRequest::Action {
                    route: route.clone(),
                    action: command.usb(),
                },
            )?),
            Self::Can {
                gateway,
                module,
                boot,
                ..
            } => {
                let mut r = command.can();
                r["gatewayId"] = json!(gateway);
                r["moduleId"] = json!(module);
                r["boot"] = boot.clone();
                r["clientId"] = json!("profile-owned-actions");
                let reply = io.can(r);
                if reply["ok"] != true {
                    return Err(
                        "The native physical action operation failed; it was not repeated.".into(),
                    );
                }
                self.cache_from_can(&reply["state"])
            }
        }
    }
}
// No Debug/Serialize: the private token cannot accidentally enter diagnostics.
enum Command {
    State,
    Ownership(u32),
    Claim {
        revision: u32,
        id: String,
        secret: String,
    },
    Renew {
        secret: String,
        lease: u32,
    },
    Release {
        secret: String,
        lease: u32,
    },
    Invoke {
        revision: u32,
        id: String,
        values: Vec<Value>,
        secret: String,
        lease: u32,
    },
}
impl Command {
    fn usb(self) -> ActionRequest {
        match self {
            Self::State => ActionRequest::State,
            Self::Ownership(offset) => ActionRequest::Ownership { offset },
            Self::Claim {
                revision,
                id,
                secret,
            } => ActionRequest::ClaimCurrent {
                catalogue_revision: revision,
                action_id: id,
                token: secret,
                ttl_ms: TTL_MS,
            },
            Self::Renew { secret, lease } => ActionRequest::Renew {
                token: secret,
                lease_id: lease,
                ttl_ms: TTL_MS,
            },
            Self::Release { secret, lease } => ActionRequest::Release {
                token: secret,
                lease_id: lease,
            },
            Self::Invoke {
                revision,
                id,
                values,
                secret,
                lease,
            } => ActionRequest::InvokeOwned {
                catalogue_revision: revision,
                action_id: id,
                values,
                token: secret,
                lease_id: lease,
            },
        }
    }
    fn can(self) -> Value {
        match self {
            Self::State => json!({"command":"action_state"}),
            Self::Ownership(offset) => json!({"command":"action_ownership","offset":offset}),
            Self::Claim {
                revision,
                id,
                secret,
            } => {
                json!({"command":"action_claim_current","catalogueRevision":revision,"actionId":id,"ownershipToken":secret,"ttlMs":TTL_MS})
            }
            Self::Renew { secret, lease } => {
                json!({"command":"action_renew","ownershipToken":secret,"leaseId":lease,"ttlMs":TTL_MS})
            }
            Self::Release { secret, lease } => {
                json!({"command":"action_release","ownershipToken":secret,"leaseId":lease})
            }
            Self::Invoke {
                revision,
                id,
                values,
                secret,
                lease,
            } => {
                json!({"command":"action_invoke_owned","catalogueRevision":revision,"actionId":id,"values":values,"ownershipToken":secret,"leaseId":lease})
            }
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cleanup {
    schema_version: u32,
    owner: RunOwner,
    resource: String,
    target: Target,
    revision: u32,
    endpoint: String,
    setter_id: String,
    setter_hash: String,
    secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    can_cleanup: Option<CanCleanupProof>,
}
/// This is identity evidence for cleanup only. It cannot authorize Claim,
/// Invoke or Renew, and a public expired ownership page is never a secret proof.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CanCleanupProof {
    gateway_serial: String,
    source_node: Value,
    profile: Value,
    catalogue_hash: String,
}
fn catalogue_hash(cache: &Value) -> Result<String, String> {
    catalogue(cache)?;
    let bytes = serde_json::to_vec(&json!({
        "revision": cache["catalogueRevision"],
        "actions": cache["actions"],
        "ownership": cache["ownership"],
    }))
    .map_err(|_| INVALID)?;
    Ok(hex(&Sha256::digest(bytes)))
}
impl CanCleanupProof {
    fn capture(target: &Target, io: &dyn Io) -> Option<Self> {
        let Target::Can {
            gateway, module, ..
        } = target
        else {
            return None;
        };
        let state = io.can_cached(gateway).ok()?;
        let cache = target.cache_from_can(&state).ok()?;
        let current = state["modules"]
            .as_array()?
            .iter()
            .find(|m| m["id"] == *module)?;
        let serial = state["gatewaySerial"].as_str()?;
        // Only the normalized STM32 gateway/module identity can survive a CAN
        // publication lease transition. Older receipts retain strict routing.
        if state["backend"] != "stm32"
            || serial.len() != 24
            || !serial
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || gateway != &format!("stm32:{serial}")
            || current["sourceNode"]
                .as_u64()
                .is_none_or(|n| n > u32::MAX as u64)
            || !current["profile"].is_object()
            || serde_json::to_vec(&current["profile"]).ok()?.len() > 1024
        {
            return None;
        }
        Some(Self {
            gateway_serial: serial.into(),
            source_node: current["sourceNode"].clone(),
            profile: current["profile"].clone(),
            catalogue_hash: catalogue_hash(&cache).ok()?,
        })
    }
    fn check(&self, target: &Target, state: &Value) -> Result<Value, String> {
        let Target::Can {
            gateway, module, ..
        } = target
        else {
            return Err(INVALID.into());
        };
        let current = state["modules"]
            .as_array()
            .and_then(|a| a.iter().find(|m| m["id"] == *module))
            .ok_or(INVALID)?;
        let cache = target.cache_from_can(state)?;
        if state["backend"] != "stm32"
            || state["gatewaySerial"] != self.gateway_serial
            || gateway != &format!("stm32:{}", self.gateway_serial)
            || current["sourceNode"] != self.source_node
            || current["profile"] != self.profile
            || catalogue_hash(&cache)? != self.catalogue_hash
        {
            return Err(INVALID.into());
        }
        Ok(cache)
    }
}
// The changed-route path has no representation for a productive command.
enum CleanupCommand {
    State,
    Ownership(u32),
    Release { secret: String, lease: u32 },
}
impl CleanupCommand {
    fn command(self) -> Command {
        match self {
            Self::State => Command::State,
            Self::Ownership(offset) => Command::Ownership(offset),
            Self::Release { secret, lease } => Command::Release { secret, lease },
        }
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.secret).into_bytes();
        bytes.fill(0);
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    sealed_hex: String,
}
fn canonical_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s)
        .ok()
        .map(|v| v.to_string())
        .as_deref()
        == Some(s)
}
fn owner_valid(o: &RunOwner) -> bool {
    canonical_uuid(&o.session)
        && canonical_uuid(&o.profile_id)
        && o.generation > 0
        && o.profile_revision > 0
        && o.profile_revision <= 9_007_199_254_740_991
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty()
        || value.len() % 2 != 0
        || value.len() > RECEIPT_BYTES
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid sealed physical output receipt.".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&value[i..i + 2], 16)
                .map_err(|_| "Invalid sealed physical output receipt.".into())
        })
        .collect()
}
fn portable_matches(native: &PortContract, graph: &PortContract) -> bool {
    let observed = PortDescriptor {
        id: "selected".into(),
        name: "Selected".into(),
        direction: osg_mapping::signal::Direction::Input,
        contract: native.clone(),
    };
    osg_profiles::resources_ports::graph_compatible(
        &observed,
        &osg_profiles::Port {
            id: "selected".into(),
            name: "Selected".into(),
            direction: observed.direction,
            contract: graph.clone(),
        },
    )
}
fn catalogue(cache: &Value) -> Result<(u32, Vec<PortDescriptor>), String> {
    if cache["status"] != "ready" {
        return Err("Refresh the device action state before acquiring an output.".into());
    }
    let revision = cache["catalogueRevision"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or(INVALID)?;
    let descriptors = crate::profile_resource_ports::actions(cache)?;
    if descriptors.is_empty()
        || descriptors.len() > 16
        || descriptors
            .iter()
            .map(|p| &p.id)
            .collect::<BTreeSet<_>>()
            .len()
            != descriptors.len()
    {
        return Err(INVALID.into());
    }
    Ok((revision, descriptors))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Released {
    Restored,
    Preserved,
}
/// One selected sink, one physical endpoint, one private firmware lease. A failed
/// write permanently revokes this object; a new Apply is required for output.
pub(crate) struct Lease {
    io: Arc<dyn Io>,
    cleanup: Cleanup,
    receipt: Receipt,
    route: Route,
    preparation: Option<PortPreparation>,
    token: Option<ResourceToken>,
    observed: String,
    contract: PortContract,
    descriptor: Value,
    attempted: bool,
    revoked: bool,
    release_attempted: bool,
    released: Option<Released>,
    lease: Option<u32>,
    // This postclaim evidence is intentionally absent from the preclaim journal.
    // A recovered, already-retired lease must not borrow a public fallback.
    fallback: Option<Vec<Value>>,
}
impl Lease {
    pub(crate) fn prepare(
        sink: &SinkPlan,
        owner: &RunOwner,
        resources: Arc<Runtime>,
        panels: Arc<ProfilePanelIo>,
    ) -> Result<Self, String> {
        Self::prepare_io(
            sink,
            owner,
            Arc::new(NativeIo { resources, panels }),
            &NativeProtector,
        )
    }
    fn prepare_io(
        sink: &SinkPlan,
        owner: &RunOwner,
        io: Arc<dyn Io>,
        protector: &dyn Protector,
    ) -> Result<Self, String> {
        if !owner_valid(owner)
            || sink.adapter != SinkAdapter::PhysicalAction
            || sink.readiness != PlanReadiness::Ready
        {
            return Err(
                "Physical action preparation requires an admitted exact native sink.".into(),
            );
        }
        let pin = sink.preparation.as_ref().ok_or(INVALID)?;
        let route = sink.route.as_ref().ok_or(INVALID)?;
        let observed = sink.observed_port_id.as_ref().ok_or(INVALID)?;
        let resource = sink
            .inventory_resource_id
            .as_ref()
            .filter(|s| canonical_uuid(s))
            .ok_or(INVALID)?;
        if pin.access() != Access::Invoke
            || pin.resolved().resource_id != *resource
            || pin.resolved().port.id != *observed
            || !portable_matches(&pin.resolved().port.contract, &sink.contract)
            || io.preparation(pin)? != *route
        {
            return Err(INVALID.into());
        }
        let target = Target::from_route(route, io.as_ref())?;
        let cache = target.cached(io.as_ref())?;
        let (revision, ports) = catalogue(&cache)?;
        let selected = ports
            .iter()
            .find(|p| p.id == *observed && p.contract == pin.resolved().port.contract)
            .ok_or(INVALID)?;
        let PortContract::Action { endpoint, .. } = &selected.contract else {
            return Err(INVALID.into());
        };
        let setter = ports.iter().find(|p| {
            matches!(&p.contract, PortContract::Action{endpoint:e,ownership:Some(capability),..}
                if e==endpoint && capability.max_lease_ms>=TTL_MS)
                && p.contract.supports_captured_setter()
        }).ok_or("This endpoint does not advertise readable atomic prior-state capture and compare-fenced restoration with the required bounded lease duration.")?;
        let action_id = observed.strip_prefix("action:").ok_or(INVALID)?;
        let descriptor = cache["actions"]
            .as_array()
            .and_then(|a| a.iter().find(|a| a["id"] == action_id))
            .cloned()
            .ok_or(INVALID)?;
        let can_cleanup = CanCleanupProof::capture(&target, io.as_ref());
        let cleanup = Cleanup {
            schema_version: 1,
            owner: owner.clone(),
            resource: resource.clone(),
            target,
            revision,
            endpoint: endpoint.clone(),
            setter_id: setter.id.strip_prefix("action:").ok_or(INVALID)?.into(),
            setter_hash: setter.contract_hash()?,
            secret: uuid::Uuid::new_v4().simple().to_string(),
            can_cleanup,
        };
        let mut plain = serde_json::to_vec(&cleanup).map_err(|_| INVALID)?;
        let sealed_result = protector.seal(&plain);
        plain.fill(0);
        let sealed = sealed_result?;
        let receipt = Receipt {
            kind: KIND.into(),
            resource_id: format!("{}:{}", resource, endpoint),
            state: serde_json::to_value(Envelope {
                schema_version: 1,
                sealed_hex: hex(&sealed),
            })
            .map_err(|_| INVALID)?,
        };
        if serde_json::to_vec(&receipt).map_err(|_| INVALID)?.len() > RECEIPT_BYTES {
            return Err(
                "The sealed physical output receipt exceeds its bounded recovery budget.".into(),
            );
        }
        Ok(Self {
            io,
            cleanup,
            receipt,
            route: route.clone(),
            preparation: Some(pin.clone()),
            token: None,
            observed: observed.clone(),
            contract: selected.contract.clone(),
            descriptor,
            attempted: false,
            revoked: false,
            release_attempted: false,
            released: None,
            lease: None,
            fallback: None,
        })
    }
    pub(crate) fn receipt(&self) -> Receipt {
        self.receipt.clone()
    }
    pub(crate) fn endpoint_key(&self) -> String {
        format!("{}:{}", self.cleanup.resource, self.cleanup.endpoint)
    }
    fn verify(&self, cache: &Value) -> Result<(), String> {
        let (revision, ports) = catalogue(cache)?;
        if revision != self.cleanup.revision
            || ports
                .iter()
                .find(|p| p.id == format!("action:{}", self.cleanup.setter_id))
                .is_none_or(|p| {
                    p.contract_hash().ok().as_ref() != Some(&self.cleanup.setter_hash)
                        || !p.contract.supports_captured_setter()
                })
        {
            return Err(INVALID.into());
        }
        if !self.observed.is_empty()
            && ports
                .iter()
                .find(|p| p.id == self.observed)
                .is_none_or(|p| p.contract != self.contract)
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    fn current_delivery(&self, owner: &RunOwner) -> Result<(), String> {
        if self.revoked || self.cleanup.owner != *owner {
            return Err("This physical action owner has been revoked.".into());
        }
        let token = self
            .token
            .as_ref()
            .ok_or("Bind a current native Invoke token before physical output delivery.")?;
        if self.io.delivery(token)? != self.route {
            return Err(INVALID.into());
        }
        self.verify(&self.cleanup.target.cached(self.io.as_ref())?)
    }
    pub(crate) fn claim(&mut self) -> Result<(), String> {
        if self.attempted || self.revoked {
            return Err("A physical output claim is attempted only once per native owner.".into());
        }
        let pin = self.preparation.as_ref().ok_or(INVALID)?;
        if self.io.preparation(pin)? != self.route {
            return Err(INVALID.into());
        }
        self.verify(&self.cleanup.target.cached(self.io.as_ref())?)?;
        self.attempted = true;
        let result = (|| {
            let cache = self.cleanup.target.request(
                self.io.as_ref(),
                Command::Claim {
                    revision: self.cleanup.revision,
                    id: self.cleanup.setter_id.clone(),
                    secret: self.cleanup.secret.clone(),
                },
            )?;
            self.verify(&cache)?;
            let lease = self.ack(&cache, None, false)?;
            self.lease = Some(lease);
            if self.cleanup.can_cleanup.is_some() {
                let entry = self.cleanup_endpoint(&self.cleanup.target, false)?;
                if entry["owned"] != true
                    || entry["leaseId"] != lease
                    || entry["actionId"] != self.cleanup.setter_id
                {
                    return Err(INVALID.into());
                }
                let values = entry["fallback"].as_array().ok_or(INVALID)?.clone();
                self.check_fallback(&cache, &values)?;
                self.fallback = Some(values);
            }
            Ok(())
        })();
        if result.is_err() {
            self.revoked = true;
        }
        result
    }
    fn ack(&self, cache: &Value, expected: Option<u32>, release: bool) -> Result<u32, String> {
        let r = &cache["lastLease"];
        let lease = r["leaseId"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or(INVALID)?;
        if r["endpoint"] != self.cleanup.endpoint
            || expected.is_some_and(|id| id != lease)
            || release
                && (r["released"] != true
                    || !matches!(r["fallbackStatus"].as_str(), Some("applied" | "preserved")))
            || !release
                && (!r["remainingMs"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= TTL_MS as u64)
                    || r["released"] == true)
        {
            return Err(INVALID.into());
        }
        Ok(lease)
    }
    pub(crate) fn bind_delivery(&mut self, sink: &SinkPlan) -> Result<(), String> {
        if self.revoked
            || !self.attempted
            || self.lease.is_none()
            || sink.adapter != SinkAdapter::PhysicalAction
            || sink.readiness != PlanReadiness::Ready
            || sink.inventory_resource_id.as_ref() != Some(&self.cleanup.resource)
            || sink.observed_port_id.as_ref() != Some(&self.observed)
            || !portable_matches(&self.contract, &sink.contract)
            || sink.route.as_ref() != Some(&self.route)
        {
            return Err(INVALID.into());
        }
        let token = sink.token.as_ref().ok_or(INVALID)?;
        let expected_hash = PortDescriptor {
            id: self.observed.clone(),
            name: self.descriptor["label"].as_str().ok_or(INVALID)?.into(),
            direction: osg_mapping::signal::Direction::Input,
            contract: self.contract.clone(),
        }
        .contract_hash()?;
        if token.access != Access::Invoke
            || token.resource_id != self.cleanup.resource
            || token.port_id != self.observed
            || token.contract_hash != expected_hash
            || self.io.delivery(token)? != self.route
        {
            return Err(INVALID.into());
        }
        self.token = Some(token.clone());
        Ok(())
    }
    pub(crate) fn invoke(
        &mut self,
        owner: &RunOwner,
        arguments: &[NamedArgument],
    ) -> Result<(), String> {
        if let Err(error) = self.current_delivery(owner) {
            self.revoke();
            return Err(error);
        }
        let PortContract::Action { parameters, .. } = &self.contract else {
            return Err(INVALID.into());
        };
        if arguments.len() != parameters.len()
            || arguments.iter().zip(parameters).any(|(a, p)| a.id != p.id)
        {
            return Err(
                "Physical action arguments must preserve the exact declared names and order."
                    .into(),
            );
        }
        let scalars = arguments
            .iter()
            .map(|a| a.value.clone())
            .collect::<Vec<_>>();
        if parameters
            .iter()
            .zip(&scalars)
            .any(|(p, v)| !p.schema.accepts(v))
        {
            return Err("Physical action values do not match their exact scalar contracts.".into());
        }
        let values = scalars
            .iter()
            .zip(parameters)
            .map(|(v, p)| match (&p.schema, v) {
                (
                    osg_mapping::signal::ScalarSchema::Integer { .. },
                    osg_profiles::PortValue::Number(n),
                ) => Ok(json!(*n as i64)),
                _ => serde_json::to_value(v).map_err(|_| INVALID),
            })
            .collect::<Result<Vec<_>, _>>()?;
        crate::actions::arguments(&self.descriptor, &json!(values))?;
        let result = (|| {
            let cache = self.cleanup.target.request(
                self.io.as_ref(),
                Command::Invoke {
                    revision: self.cleanup.revision,
                    id: self.observed.strip_prefix("action:").ok_or(INVALID)?.into(),
                    values,
                    secret: self.cleanup.secret.clone(),
                    lease: self.lease.ok_or(INVALID)?,
                },
            )?;
            self.verify(&cache)?;
            if !cache["states"]
                .as_array()
                .is_some_and(|a| a.iter().any(|s| s["id"] == self.descriptor["id"]))
            {
                return Err(INVALID.into());
            }
            Ok(())
        })();
        if result.is_err() {
            self.revoked = true;
        }
        result
    }
    pub(crate) fn renew(&mut self, owner: &RunOwner) -> Result<(), String> {
        if let Err(error) = self.current_delivery(owner) {
            self.revoke();
            return Err(error);
        }
        let lease = self.lease.ok_or(INVALID)?;
        let result = (|| {
            let cache = self.cleanup.target.request(
                self.io.as_ref(),
                Command::Renew {
                    secret: self.cleanup.secret.clone(),
                    lease,
                },
            )?;
            self.verify(&cache)?;
            self.ack(&cache, Some(lease), false)?;
            Ok(())
        })();
        if result.is_err() {
            self.revoked = true;
        }
        result
    }
    /// Immediately prevents new deliveries. Actual release runs on the owner
    /// worker and uses the firmware's compare-if-unchanged fallback.
    pub(crate) fn revoke(&mut self) {
        self.revoked = true;
        self.token = None;
    }
    fn check_fallback(&self, cache: &Value, values: &[Value]) -> Result<(), String> {
        let setter = cache["actions"]
            .as_array()
            .and_then(|a| a.iter().find(|a| a["id"] == self.cleanup.setter_id))
            .ok_or(INVALID)?;
        crate::actions::arguments(setter, &json!(values))?;
        Ok(())
    }
    fn cleanup_target(&self) -> Result<(Target, bool), String> {
        if self.cleanup.target.cached(self.io.as_ref()).is_ok() {
            return Ok((self.cleanup.target.clone(), false));
        }
        let proof = self.cleanup.can_cleanup.as_ref().ok_or(INVALID)?;
        let Target::Can {
            gateway,
            module,
            lease,
            ..
        } = &self.cleanup.target
        else {
            return Err(INVALID.into());
        };
        let state = self.io.can_cached(gateway)?;
        let current = state["modules"]
            .as_array()
            .and_then(|a| a.iter().find(|m| m["id"] == *module))
            .ok_or(INVALID)?;
        let current_lease = current["lease"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= u32::MAX as u64)
            .ok_or(INVALID)?;
        if current["lease"] == *lease {
            return Err(INVALID.into());
        }
        let mut target = self.cleanup.target.clone();
        let Target::Can { lease, .. } = &mut target else {
            unreachable!()
        };
        *lease = json!(current_lease);
        proof.check(&target, &state)?;
        Ok((target, true))
    }
    fn cleanup_request(
        &self,
        target: &Target,
        changed: bool,
        command: CleanupCommand,
    ) -> Result<Value, String> {
        if changed {
            let Target::Can { gateway, .. } = target else {
                return Err(INVALID.into());
            };
            self.cleanup
                .can_cleanup
                .as_ref()
                .ok_or(INVALID)?
                .check(target, &self.io.can_cached(gateway)?)?;
        }
        let cache = target.request(self.io.as_ref(), command.command())?;
        self.verify(&cache)?;
        if changed {
            let Target::Can { gateway, .. } = target else {
                return Err(INVALID.into());
            };
            let proof = self.cleanup.can_cleanup.as_ref().ok_or(INVALID)?;
            proof.check(target, &self.io.can_cached(gateway)?)?;
            if catalogue_hash(&cache)? != proof.catalogue_hash {
                return Err(INVALID.into());
            }
        }
        Ok(cache)
    }
    fn cleanup_endpoint(&self, target: &Target, changed: bool) -> Result<Value, String> {
        let mut offset = 0;
        let mut endpoint = None;
        loop {
            let cache = self.cleanup_request(target, changed, CleanupCommand::Ownership(offset))?;
            let mut wire_page = cache["ownershipPage"].clone();
            wire_page["ok"] = json!(true);
            let page = crate::output_leases::ownership_page(
                &wire_page,
                u64::from(self.cleanup.revision),
                offset as usize,
            )?;
            if page["catalogueRevision"] != self.cleanup.revision || !page["ownership"].is_array() {
                return Err(INVALID.into());
            }
            for entry in page["ownership"].as_array().unwrap() {
                if entry["endpoint"] == self.cleanup.endpoint
                    && endpoint.replace(entry.clone()).is_some()
                {
                    return Err(INVALID.into());
                }
            }
            match page["next"].as_u64() {
                None if page["next"].is_null() => break,
                Some(next) if next > offset as u64 && next <= 16 => offset = next as u32,
                _ => return Err(INVALID.into()),
            }
        }
        endpoint.ok_or(INVALID.into())
    }
    pub(crate) fn release(&mut self) -> Result<Released, String> {
        self.revoke();
        if let Some(done) = self.released {
            return Ok(done);
        }
        if !self.attempted {
            self.released = Some(Released::Preserved);
            return Ok(Released::Preserved);
        }
        if self.release_attempted {
            return Err(
                "Physical cleanup was uncertain; retain the sealed receipt for explicit recovery."
                    .into(),
            );
        }
        // Refresh is read-only. It clears uncertain host cache state, never a
        // failed mutation or the firmware's last-owned comparison authority.
        let (target, changed) = self.cleanup_target()?;
        let state = self.cleanup_request(&target, changed, CleanupCommand::State)?;
        let entry = self.cleanup_endpoint(&target, changed)?;
        if entry["owned"] == false {
            if entry["fallbackStatus"] == "failed" {
                return Err("The device reported failed physical output restoration.".into());
            }
            if changed {
                let old_lease = self.lease.ok_or(PENDING_ROUTE)?;
                let fallback = self.fallback.as_ref().ok_or(PENDING_ROUTE)?;
                self.check_fallback(&state, fallback)?;
                let actual = state["states"]
                    .as_array()
                    .and_then(|a| a.iter().find(|v| v["id"] == self.cleanup.setter_id))
                    .ok_or(PENDING_ROUTE)?;
                if entry["leaseId"] != old_lease
                    || entry["actionId"] != self.cleanup.setter_id
                    || entry["remainingMs"] != 0
                    || entry["fallbackStatus"] != "applied"
                    || entry["fallback"] != json!(fallback)
                    || actual["values"] != json!(fallback)
                    || self.cleanup_endpoint(&target, changed)? != entry
                {
                    return Err(PENDING_ROUTE.into());
                }
                self.released = Some(Released::Restored);
                return Ok(Released::Restored);
            }
            self.released = Some(Released::Preserved);
            return Ok(Released::Preserved);
        }
        let lease = entry["leaseId"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or(INVALID)?;
        if changed && entry["actionId"] != self.cleanup.setter_id {
            return Err(INVALID.into());
        }
        if self.lease.is_some_and(|id| id != lease) {
            return Err(
                "A later owner now holds this physical endpoint; its state was preserved.".into(),
            );
        }
        self.release_attempted = true;
        let cache = self.cleanup_request(
            &target,
            changed,
            CleanupCommand::Release {
                secret: self.cleanup.secret.clone(),
                lease,
            },
        )?;
        self.ack(&cache, Some(lease), true)?;
        let result = if cache["lastLease"]["fallbackStatus"] == "applied" {
            Released::Restored
        } else {
            if changed {
                return Err(PENDING_ROUTE.into());
            }
            Released::Preserved
        };
        self.released = Some(result);
        Ok(result)
    }
    pub(crate) fn recover(
        receipt: &Receipt,
        owner: &RunOwner,
        resources: Arc<Runtime>,
        panels: Arc<ProfilePanelIo>,
    ) -> Result<Self, String> {
        Self::recover_io(
            receipt,
            owner,
            Arc::new(NativeIo { resources, panels }),
            &NativeProtector,
        )
    }
    fn recover_io(
        receipt: &Receipt,
        owner: &RunOwner,
        io: Arc<dyn Io>,
        protector: &dyn Protector,
    ) -> Result<Self, String> {
        if receipt.kind != KIND
            || !owner_valid(owner)
            || serde_json::to_vec(receipt).map_err(|_| INVALID)?.len() > RECEIPT_BYTES
        {
            return Err(INVALID.into());
        }
        let envelope: Envelope =
            serde_json::from_value(receipt.state.clone()).map_err(|_| INVALID)?;
        if envelope.schema_version != 1 {
            return Err(INVALID.into());
        }
        let mut plain = protector.unseal(&unhex(&envelope.sealed_hex)?)?;
        let parsed = serde_json::from_slice::<Cleanup>(&plain);
        plain.fill(0);
        let cleanup = parsed.map_err(|_| INVALID)?;
        if cleanup.schema_version != 1
            || cleanup.owner != *owner
            || !canonical_uuid(&cleanup.resource)
            || cleanup.revision == 0
            || cleanup.secret.len() != 32
            || !cleanup.secret.bytes().all(|b| b.is_ascii_hexdigit())
            || receipt.resource_id != format!("{}:{}", cleanup.resource, cleanup.endpoint)
        {
            return Err(INVALID.into());
        }
        let route = match &cleanup.target {
            Target::Usb { key, route } => Route {
                id: format!("usb:{key}"),
                transport: Transport::Usb,
                provider_generation: format!(
                    "{}:{}:{}",
                    route.connection_generation, route.boot, route.configuration_generation
                ),
                attributes: Default::default(),
                baseline_ready: false,
                writable: false,
                priority: 0,
            },
            Target::Can {
                gateway,
                module,
                epoch,
                node,
                lease,
                boot,
                configuration,
            } => Route {
                id: format!("can:{gateway}:{module}"),
                transport: Transport::Can,
                provider_generation: json!([epoch, boot, lease, configuration, null]).to_string(),
                attributes: std::collections::BTreeMap::from([
                    ("gatewayId".into(), gateway.clone()),
                    ("moduleId".into(), module.clone()),
                    ("node".into(), node.to_string()),
                    ("boot".into(), boot.to_string()),
                ]),
                baseline_ready: false,
                writable: false,
                priority: 0,
            },
        };
        // Cleanup objects cannot be bound, claim or invoke: they are born revoked.
        Ok(Self {
            io,
            cleanup,
            receipt: receipt.clone(),
            route,
            preparation: None,
            token: None,
            observed: String::new(),
            contract: PortContract::Event {
                schema: osg_mapping::signal::ScalarSchema::Boolean {},
            },
            descriptor: Value::Null,
            attempted: true,
            revoked: true,
            release_attempted: false,
            released: None,
            lease: None,
            fallback: None,
        })
    }
}
#[cfg(test)]
#[path = "profile_session_action_lease_tests.rs"]
mod tests;

//! Persistent resource identity over provider-owned discovery and capture.
//! Reconciliation never opens a device, acquires an output or changes a profile.
use crate::resources_ports::{
    digest, text, validate_ports, PortContract, PortDescriptor, PortUpdate, MAX_CATALOGUE_PORTS,
    MAX_PORT_PAGE,
};
use crate::store::LocalAssignment;
use crate::{PortValue, Resource};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

#[path = "resources_companion.rs"]
mod companion;

pub const MAX_RESOURCES: usize = 2048;
pub const MAX_CACHED_PORTS: usize = 262_144;
pub const MAX_OBSERVATIONS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceType {
    HostController,
    OsgModule,
    Companion,
    GameIntegration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    Hid,
    Can,
    Usb,
    Network,
    Integration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceKind {
    LocalPath,
    Serial,
    VerifiedPhysical,
    CompanionInstance,
    GameScope,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityEvidence {
    pub kind: EvidenceKind,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route {
    pub id: String,
    pub transport: Transport,
    pub provider_generation: String,
    pub attributes: BTreeMap<String, String>,
    pub baseline_ready: bool,
    pub writable: bool,
    pub priority: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub key: String,
    pub name: String,
    pub resource_type: ResourceType,
    pub evidence: Vec<IdentityEvidence>,
    pub attributes: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
    #[serde(deserialize_with = "required_option")]
    pub route: Option<Route>,
    #[serde(deserialize_with = "required_option")]
    pub ports: Option<PortUpdate>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderBatch {
    pub scope: String,
    pub session: String,
    pub revision: u64,
    pub complete: bool,
    pub observations: Vec<Observation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    Online,
    Offline,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    Read,
    Write,
    Invoke,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceToken {
    pub inventory_session: String,
    pub resource_id: String,
    pub generation: String,
    pub capability_revision: String,
    pub route_id: String,
    pub provider_session: String,
    pub provider_generation: String,
    pub port_id: String,
    pub contract_hash: String,
    pub access: Access,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub scope: String,
    pub key: String,
    pub route: Route,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSummary {
    pub id: String,
    pub name: String,
    pub resource_type: ResourceType,
    pub availability: Availability,
    pub generation: String,
    pub capability_revision: String,
    pub port_count: usize,
    pub cached_port_count: usize,
    pub connections: Vec<Connection>,
    pub attributes: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePage {
    pub session: String,
    pub revision: String,
    pub offset: usize,
    pub total: usize,
    pub resources: Vec<ResourceSummary>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedPort {
    pub descriptor: PortDescriptor,
    pub contract_hash: String,
    pub route_ids: Vec<String>,
    pub current: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDetails {
    pub session: String,
    pub revision: String,
    pub resource: ResourceSummary,
    pub evidence: Vec<IdentityEvidence>,
    pub ports: Vec<ObservedPort>,
    pub offset: usize,
    pub total: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortExpectation {
    pub profile_port_id: String,
    pub observed_port_id: String,
    pub expected_contract_hash: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentDiagnostic {
    pub code: String,
    pub resource_id: String,
    pub port_id: Option<String>,
    pub message: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentReport {
    pub valid: bool,
    pub diagnostics: Vec<AssignmentDiagnostic>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    pub session: String,
    pub revision: String,
    pub changed: Vec<String>,
    pub redirects: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct ResolvedRoute {
    pub resource_id: String,
    pub scope: String,
    pub key: String,
    pub route: Route,
    pub port: PortDescriptor,
}
/// Native-only acquisition pin. This is deliberately neither serializable nor
/// a delivery token: it can prepare a provider without manufacturing a sample.
#[derive(Clone, Debug)]
pub struct PortPreparation {
    pin: PreparationPin,
    resolved: ResolvedRoute,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct PreparationPin {
    inventory_session: String,
    resource_id: String,
    generation: String,
    capability_revision: String,
    provider_session: String,
    scope: String,
    key: String,
    route: Route,
    port_id: String,
    contract_hash: String,
    access: Access,
    selection_scope: Option<String>,
    selection_catalogue_revision: Option<String>,
    companion: Option<companion::CompanionPortPin>,
}
impl PortPreparation {
    pub fn resolved(&self) -> &ResolvedRoute {
        &self.resolved
    }
    pub fn inventory_session(&self) -> &str {
        &self.pin.inventory_session
    }
    pub fn capability_revision(&self) -> &str {
        &self.pin.capability_revision
    }
    pub fn provider_session(&self) -> &str {
        &self.pin.provider_session
    }
    pub fn access(&self) -> Access {
        self.pin.access
    }
    pub fn requires_baseline(&self) -> bool {
        self.pin.access == Access::Read && !self.resolved.route.baseline_ready
    }
    pub fn requires_provider_acquisition(&self) -> bool {
        self.pin.access != Access::Read && !self.resolved.route.writable
    }
    pub fn fingerprint(&self) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(&self.pin).map_err(|error| error.to_string())?)
        ))
    }
    pub(crate) fn matches_capture(
        &self,
        session: &str,
        resource: &ResourceSummary,
        port: &ObservedPort,
        access: Access,
    ) -> bool {
        if self.pin.companion.is_some() {
            return self.companion_matches_capture(session, resource)
                && self.pin.port_id == port.descriptor.id
                && self.pin.contract_hash == port.contract_hash
                && self.resolved.port == port.descriptor
                && self.pin.access == access;
        }
        self.pin.inventory_session == session
            && self.pin.resource_id == resource.id
            && (self.pin.selection_scope.is_some() || self.pin.generation == resource.generation)
            && (self.pin.selection_scope.is_some()
                || self.pin.capability_revision == resource.capability_revision)
            && self.pin.port_id == port.descriptor.id
            && self.pin.contract_hash == port.contract_hash
            && self.pin.access == access
            && port.current
            && port.route_ids.contains(&self.pin.route.id)
            && resource.connections.iter().any(|connection| {
                connection.scope == self.resolved.scope
                    && connection.key == self.resolved.key
                    && connection.route == self.pin.route
            })
    }
}
#[derive(Clone, Debug)]
pub struct ValidatedInvocation {
    token: ResourceToken,
    resolved: ResolvedRoute,
    arguments: Vec<PortValue>,
}
impl ValidatedInvocation {
    pub fn token(&self) -> &ResourceToken {
        &self.token
    }
    pub fn resolved(&self) -> &ResolvedRoute {
        &self.resolved
    }
    pub fn arguments(&self) -> &[PortValue] {
        &self.arguments
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedCatalogue {
    pub revision: String,
    pub total: u32,
    pub complete: bool,
    pub ports: Vec<PortDescriptor>,
    pub hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedSighting {
    pub scope: String,
    pub key: String,
    pub evidence: Vec<IdentityEvidence>,
    pub attributes: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
    #[serde(deserialize_with = "required_option")]
    pub catalogue: Option<SavedCatalogue>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedResource {
    pub id: String,
    pub name: String,
    pub resource_type: ResourceType,
    pub sightings: Vec<SavedSighting>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedRegistry {
    pub schema_version: u32,
    pub resources: Vec<SavedResource>,
    pub redirects: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct Live {
    route: Route,
    provider_session: String,
    catalogue_revision: Option<String>,
}
#[derive(Clone, Debug)]
struct Entry {
    saved: SavedResource,
    live: BTreeMap<(String, String), Live>,
    generation: u64,
    // A registry transaction shares this immutable projection with its clone.
    // Only changed entries discard it; reading a page never walks or hashes an
    // entire simulator catalogue again.
    observed: OnceLock<Result<Arc<Vec<ObservedPort>>, String>>,
    validated_saved: OnceLock<Result<Arc<ValidatedSaved>, String>>,
    sighting_validation: BTreeMap<(String, String), Arc<OnceLock<Result<Arc<ValidatedSighting>, String>>>>,
}
#[derive(Debug)]
struct ValidatedSighting {
    hash: String,
    serialized_bytes: usize,
    contract_hashes: Vec<String>,
}
#[derive(Debug)]
struct ValidatedSaved {
    hash: String,
    serialized_bytes: usize,
    port_count: usize,
}
impl Entry {
    fn new(saved: SavedResource) -> Self {
        let sighting_validation = saved.sightings.iter().map(|sighting|
            ((sighting.scope.clone(),sighting.key.clone()),Arc::new(OnceLock::new()))).collect();
        Self {saved,live:BTreeMap::new(),generation:0,observed:OnceLock::new(),validated_saved:OnceLock::new(),sighting_validation}
    }
    fn invalidate_observed(&mut self) {
        self.observed.take();
    }
    fn invalidate_saved(&mut self) {
        self.invalidate_observed();
        self.validated_saved.take();
    }
    fn validated_saved(&self) -> Result<Arc<ValidatedSaved>, String> {
        self.validated_saved.get_or_init(|| {
            canonical_uuid(&self.saved.id)?;
            text(&self.saved.name,320)?;
            if self.saved.sightings.is_empty() || self.saved.sightings.len()>64 {
                return Err("Invalid resource identity or sighting count.".into());
            }
            let mut physical=BTreeSet::new();
            let mut sightings=Vec::new();
            let mut port_count=0usize;
            let mut serialized_bytes=0usize;
            for sighting in &self.saved.sightings {
                let validated=self.validated_sighting(sighting)?;
                physical.extend(verified(&sighting.evidence));
                sightings.push(validated.hash.clone());
                port_count=port_count.saturating_add(validated.contract_hashes.len());
                serialized_bytes=serialized_bytes.saturating_add(validated.serialized_bytes);
            }
            if physical.len()>1 {return Err("Conflicting verified physical identity evidence.".into());}
            // An empty sightings array contributes its brackets already. Add the
            // exact cached element bytes and the separating commas only.
            let mut limit = InventoryLimit { written: 0 };
            serde_json::to_writer(&mut limit, &SavedResource {id:self.saved.id.clone(),name:self.saved.name.clone(),resource_type:self.saved.resource_type,sightings:vec![]})
                .map_err(|_| "Resource inventory exceeds its 128 MiB serialized envelope.".to_owned())?;
            serialized_bytes=serialized_bytes.saturating_add(limit.written).saturating_add(self.saved.sightings.len().saturating_sub(1));
            Ok(Arc::new(ValidatedSaved {
                hash: digest(&(&self.saved.id,&self.saved.name,self.saved.resource_type,sightings))?,
                serialized_bytes,
                port_count,
            }))
        }).clone()
    }
    fn validated_sighting(&self, sighting: &SavedSighting) -> Result<Arc<ValidatedSighting>,String> {
        // Kind/evidence compatibility is cheap and must follow a promotion or
        // identity merge even when this exact sighting did not change.
        validate_sighting_metadata(sighting,self.saved.resource_type)?;
        let cache=self.sighting_validation.get(&(sighting.scope.clone(),sighting.key.clone()))
            .ok_or("Missing immutable resource sighting validation.")?;
        cache.get_or_init(|| {
            validate_sighting(sighting,self.saved.resource_type)?;
            let mut limit=InventoryLimit {written:0};
            serde_json::to_writer(&mut limit,sighting).map_err(|_|"Resource inventory exceeds its 128 MiB serialized envelope.".to_owned())?;
            let contract_hashes=sighting.catalogue.as_ref().map(|catalogue|catalogue.ports.iter().map(PortDescriptor::contract_hash).collect::<Result<Vec<_>,_>>()).transpose()?.unwrap_or_default();
            Ok(Arc::new(ValidatedSighting {hash:digest(sighting)?,serialized_bytes:limit.written,contract_hashes}))
        }).clone()
    }
}
#[derive(Clone, Debug)]
struct Scope {
    session: String,
    revision: u64,
    retired: BTreeSet<String>,
}
#[derive(Clone, Debug)]
pub struct Registry {
    session: String,
    revision: u64,
    entries: BTreeMap<String, Arc<Entry>>,
    redirects: BTreeMap<String, String>,
    scopes: BTreeMap<String, Scope>,
    failures: BTreeMap<String, String>,
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

impl Default for SavedRegistry {
    fn default() -> Self {
        Self {
            schema_version: 1,
            resources: vec![],
            redirects: BTreeMap::new(),
        }
    }
}
impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
impl Registry {
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn revision(&self) -> String {
        self.revision.to_string()
    }
    pub fn new() -> Self {
        Self {
            session: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            entries: BTreeMap::new(),
            redirects: BTreeMap::new(),
            scopes: BTreeMap::new(),
            failures: BTreeMap::new(),
        }
    }
    pub fn restore(saved: SavedRegistry) -> Result<Self, String> {
        saved.validate()?;
        let mut registry = Self::new();
        registry.redirects = saved.redirects;
        registry.entries = saved
            .resources
            .into_iter()
            .map(|saved| {
                (
                    saved.id.clone(),
                    Arc::new(Entry::new(saved)),
                )
            })
            .collect();
        Ok(registry)
    }
    pub fn saved(&self) -> SavedRegistry {
        SavedRegistry {
            schema_version: 1,
            resources: self.entries.values().map(|e| e.saved.clone()).collect(),
            redirects: self.redirects.clone(),
        }
    }
    /// Cached persisted metadata identity, independent of live routes and tokens.
    pub fn saved_fingerprint(&self) -> Result<String, String> {
        self.saved_signature()
    }
    fn saved_signature(&self) -> Result<String, String> {
        let resources = self.entries.iter().map(|(id, entry)|
            Ok((id.as_str(), entry.validated_saved()?.hash.clone())))
            .collect::<Result<Vec<_>, String>>()?;
        digest(&(resources, &self.redirects))
    }
    /// Entries are immutable behind Arc until a transaction changes them. Their
    /// complete persisted bytes and catalogue contracts were already validated;
    /// only the small cross-entry identity/envelope checks run again here.
    fn validate_saved(&self) -> Result<(), String> {
        if self.entries.len() > MAX_RESOURCES || self.redirects.len() > MAX_RESOURCES {
            return Err("Unsupported or oversized resource inventory envelope.".into());
        }
        let mut aliases = BTreeSet::new();
        let mut proofs = BTreeMap::new();
        let mut count = 0usize;
        let mut bytes = serde_json::to_vec(&SavedRegistry {
            schema_version: 1, resources: vec![], redirects: self.redirects.clone(),
        }).map_err(|error| error.to_string())?.len();
        bytes = bytes.saturating_add(self.entries.len().saturating_sub(1));
        for (id, entry) in &self.entries {
            let validated = entry.validated_saved()?;
            if id != &entry.saved.id { return Err("Invalid resource identity or sighting count.".into()); }
            count = count.saturating_add(validated.port_count);
            bytes = bytes.saturating_add(validated.serialized_bytes);
            let mut physical = BTreeSet::new();
            for sighting in &entry.saved.sightings {
                if !aliases.insert((&sighting.scope, &sighting.key)) {
                    return Err("A provider identity aliases several persistent resources.".into());
                }
                physical.extend(verified(&sighting.evidence));
            }
            for proof in physical {
                if proofs.insert(proof, id).is_some() {
                    return Err("Verified physical identity is duplicated instead of correlated.".into());
                }
            }
        }
        if count > MAX_CACHED_PORTS { return Err("Resource inventory exceeds 262144 cached ports.".into()); }
        if bytes > crate::resources_store::MAX_INVENTORY_BYTES {
            return Err("Resource inventory exceeds its 128 MiB serialized envelope.".into());
        }
        for (alias, target) in &self.redirects {
            canonical_uuid(alias)?;
            if self.entries.contains_key(alias) || !self.entries.contains_key(target) {
                return Err("Invalid resource redirect; cycles and dangling aliases are rejected.".into());
            }
        }
        Ok(())
    }
    pub fn reconcile(&mut self, batch: ProviderBatch) -> Result<ReconcileReport, String> {
        batch.validate()?;
        if let Some(scope) = self.scopes.get(&batch.scope) {
            if (scope.session == batch.session && batch.revision <= scope.revision)
                || scope.retired.contains(&batch.session)
            {
                return Err(
                    "Stale resource provider response; the observation was not adopted.".into(),
                );
            }
        }
        let before = self
            .entries
            .iter()
            .map(|(id, e)| Ok((id.clone(), fingerprint(e)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let before_saved = self.saved_signature()?;
        let prior_live = self
            .entries
            .values()
            .flat_map(|entry| {
                entry
                    .live
                    .iter()
                    .map(|(key, live)| (key.clone(), live.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let mut next = self.clone();
        let old_scope = next.scopes.get(&batch.scope).cloned();
        let session_changed = old_scope
            .as_ref()
            .is_some_and(|s| s.session != batch.session);
        let mut retired = old_scope.map(|s| s.retired).unwrap_or_default();
        if session_changed {
            retired.insert(next.scopes[&batch.scope].session.clone());
            if retired.len() > 64 {
                return Err("Resource provider session history exceeds its bound.".into());
            }
        }
        if batch.complete || session_changed {
            for entry in next.entries.values_mut() {
                if !entry.live.keys().any(|(scope,_)| scope == &batch.scope) { continue; }
                let entry = Arc::make_mut(entry);
                let before = entry.live.len();
                entry.live.retain(|(scope, _), _| scope != &batch.scope);
                if before != entry.live.len() { entry.invalidate_observed(); }
            }
        }
        next.failures.remove(&batch.scope);
        for observation in &batch.observations {
            let id = next.locate(&batch.scope, observation)?;
            let entry = Arc::make_mut(next.entries.get_mut(&id).unwrap());
            entry.invalidate_saved();
            entry.saved.name = observation.name.clone();
            if observation.resource_type == ResourceType::OsgModule {
                entry.saved.resource_type = ResourceType::OsgModule;
            }
            let index = entry
                .saved
                .sightings
                .iter()
                .position(|s| s.scope == batch.scope && s.key == observation.key);
            let previous = index.map(|i| entry.saved.sightings[i].clone());
            let catalogue = match (
                &observation.ports,
                previous.as_ref().and_then(|s| s.catalogue.as_ref()),
            ) {
                (Some(update), previous) => Some(adopt_ports(update, previous)?),
                (None, previous) => previous.cloned(),
            };
            let sighting = SavedSighting {
                scope: batch.scope.clone(),
                key: observation.key.clone(),
                evidence: observation.evidence.clone(),
                attributes: observation.attributes.clone(),
                diagnostics: observation.diagnostics.clone(),
                catalogue,
            };
            entry.sighting_validation.insert((batch.scope.clone(),observation.key.clone()),Arc::new(OnceLock::new()));
            if let Some(index) = index {
                entry.saved.sightings[index] = sighting;
            } else {
                entry.saved.sightings.push(sighting);
            }
            let key = (batch.scope.clone(), observation.key.clone());
            if let Some(route) = &observation.route {
                let catalogue_revision = observation
                    .ports
                    .as_ref()
                    .map(|p| p.revision.clone())
                    .or_else(|| {
                        prior_live
                            .get(&key)
                            .filter(|prior| {
                                prior.provider_session == batch.session && prior.route == *route
                            })
                            .and_then(|prior| prior.catalogue_revision.clone())
                    });
                entry.live.insert(
                    key,
                    Live {
                        route: route.clone(),
                        provider_session: batch.session.clone(),
                        catalogue_revision,
                    },
                );
            } else {
                entry.live.remove(&key);
            }
        }
        let mut changed = Vec::new();
        // These native scopes enumerate remembered configuration, including
        // paired/offline devices. Omission in a complete successful response
        // means explicit removal, unlike discovery scopes such as USB unplug.
        if batch.complete {
            let keys = batch.observations.iter().map(|item| item.key.as_str()).collect::<BTreeSet<_>>();
            next.entries.retain(|id, entry| {
                let authoritative = match entry.saved.resource_type {
                    ResourceType::Companion => matches!(batch.scope.as_str(),"companions"|"paired-companions"),
                    ResourceType::HostController => batch.scope == "host-controllers",
                    ResourceType::OsgModule => batch.scope == "host-controllers" || batch.scope.starts_with("gateway:"),
                    ResourceType::GameIntegration => false,
                };
                if authoritative {
                    if entry.saved.sightings.iter().any(|sighting| sighting.scope == batch.scope && !keys.contains(sighting.key.as_str())) {
                        let entry = Arc::make_mut(entry);
                        entry.invalidate_saved();
                        entry.saved.sightings.retain(|sighting| sighting.scope != batch.scope || keys.contains(sighting.key.as_str()));
                        entry.sighting_validation.retain(|(scope,key),_|scope!=&batch.scope || keys.contains(key.as_str()));
                    }
                }
                let retained = !entry.saved.sightings.is_empty();
                if !retained { changed.push(id.clone()); }
                retained
            });
            next.redirects.retain(|_, target| next.entries.contains_key(target));
        }
        next.scopes.insert(
            batch.scope.clone(),
            Scope {
                session: batch.session,
                revision: batch.revision,
                retired,
            },
        );
        next.validate_saved()?;
        for (id, entry) in &mut next.entries {
            if before.get(id) != Some(&fingerprint(entry)?) {
                let entry = Arc::make_mut(entry);
                entry.generation = entry
                    .generation
                    .checked_add(1)
                    .ok_or("Resource generation exhausted.")?;
                changed.push(id.clone());
            }
        }
        if !changed.is_empty()
            || before_saved != next.saved_signature()?
            || self.failures != next.failures
        {
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or("Resource inventory revision exhausted.")?;
        }
        let report = ReconcileReport {
            session: next.session.clone(),
            revision: next.revision.to_string(),
            changed,
            redirects: next.redirects.clone(),
        };
        *self = next;
        Ok(report)
    }
    pub fn provider_failed(&mut self, scope: &str, reason: &str) {
        let message = reason
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect::<String>();
        let already_failed = self.failures.get(scope) == Some(&message);
        self.failures.insert(scope.to_owned(), message);
        let mut changed = false;
        for entry in self.entries.values_mut() {
            if !entry.live.keys().any(|(s,_)| s == scope) { continue; }
            let entry = Arc::make_mut(entry);
            let before = entry.live.len();
            entry.live.retain(|(s, _), _| s != scope);
            if before != entry.live.len() {
                entry.invalidate_observed();
                entry.generation = entry.generation.saturating_add(1);
                changed = true;
            }
        }
        if changed || !already_failed {
            self.revision = self.revision.saturating_add(1);
        }
    }
    pub fn list(&self, offset: usize, limit: usize) -> Result<ResourcePage, String> {
        page(offset, limit, self.entries.len())?;
        Ok(ResourcePage {
            session: self.session.clone(),
            revision: self.revision.to_string(),
            offset,
            total: self.entries.len(),
            resources: self
                .entries
                .values()
                .skip(offset)
                .take(limit)
                .map(|e| self.summary(e))
                .collect::<Result<_, _>>()?,
        })
    }
    pub fn inspect(
        &self,
        id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ResourceDetails, String> {
        let entry = self.entry(id)?;
        let ports = observed_ports(entry)?;
        page(offset, limit, ports.len())?;
        let mut evidence = Vec::new();
        for sighting in &entry.saved.sightings {
            for item in &sighting.evidence {
                if !evidence.contains(item) {
                    evidence.push(item.clone());
                }
            }
        }
        Ok(ResourceDetails {
            session: self.session.clone(),
            revision: self.revision.to_string(),
            resource: self.summary_with_port_count(entry, ports.len())?,
            evidence,
            total: ports.len(),
            offset,
            ports: ports.iter().skip(offset).take(limit).cloned().collect(),
        })
    }
    /// Native capture projection. Enumerate/hash the catalogue once rather than
    /// repeating a complete catalogue walk for each page or selected port.
    pub fn inspect_selected(
        &self,
        id: &str,
        expectations: &[PortExpectation],
    ) -> Result<ResourceDetails, String> {
        if expectations.len() > crate::validation::MAX_PORTS {
            return Err("A compile capture may select at most 1024 exact ports.".into());
        }
        let entry = self.entry(id)?;
        let ports = observed_ports(entry)?;
        let mut selected = BTreeMap::new();
        for expected in expectations {
            let variants = port_variants(&ports, &expected.observed_port_id);
            // Prefer the current exact variant, then a changed current variant
            // so the compiler reports a changed contract rather than silence.
            let port = variants
                .iter()
                .find(|port| port.current && port.contract_hash == expected.expected_contract_hash)
                .or_else(|| variants.iter().find(|port| port.current))
                .or_else(|| {
                    variants
                        .iter()
                        .find(|port| port.contract_hash == expected.expected_contract_hash)
                })
                .or_else(|| variants.first());
            if let Some(port) = port {
                selected.insert(
                    (port.descriptor.id.clone(), port.contract_hash.clone()),
                    port.clone(),
                );
            }
        }
        let mut evidence = Vec::new();
        for sighting in &entry.saved.sightings {
            for item in &sighting.evidence {
                if !evidence.contains(item) {
                    evidence.push(item.clone());
                }
            }
        }
        Ok(ResourceDetails {
            session: self.session.clone(),
            revision: self.revision.to_string(),
            resource: self.summary_with_port_count(entry, ports.len())?,
            evidence,
            offset: 0,
            total: ports.len(),
            ports: selected.into_values().collect(),
        })
    }
    pub fn token(&self, id: &str, port_id: &str, access: Access) -> Result<ResourceToken, String> {
        let entry = self.entry(id)?;
        let resolved = self.choose(entry, port_id, access)?;
        let live = entry
            .live
            .get(&(resolved.scope.clone(), resolved.key.clone()))
            .unwrap();
        Ok(ResourceToken {
            inventory_session: self.session.clone(),
            resource_id: entry.saved.id.clone(),
            generation: entry.generation.to_string(),
            capability_revision: capability_revision(entry)?,
            route_id: resolved.route.id.clone(),
            provider_session: live.provider_session.clone(),
            provider_generation: resolved.route.provider_generation.clone(),
            port_id: port_id.to_owned(),
            contract_hash: resolved.port.contract_hash()?,
            access,
        })
    }
    /// Current identity/catalogue admission only. Game provider setup and native
    /// process authority must still be checked by its acquisition adapter.
    pub fn prepare_port(
        &self,
        id: &str,
        port_id: &str,
        expected_contract_hash: &str,
        access: Access,
    ) -> Result<PortPreparation, String> {
        self.prepare_port_selection(id, port_id, expected_contract_hash, access, None)
    }
    /// Native game acquisition always pins catalogue metadata, never a live
    /// outgoing profile's reader. The metadata scope cannot be a delivery scope.
    pub fn prepare_game_metadata_port(
        &self,
        id: &str,
        port_id: &str,
        expected_contract_hash: &str,
        access: Access,
        game_id: &str,
    ) -> Result<PortPreparation, String> {
        let entry = self.entry(id)?;
        if entry.saved.resource_type != ResourceType::GameIntegration {
            return Err("Only game resources have game metadata acquisition scopes.".into());
        }
        let scopes = entry.saved.sightings.iter().flat_map(|sighting|&sighting.evidence)
            .filter(|e| e.kind == EvidenceKind::GameScope)
            .filter(|e| e.value == game_id || game_id == "dcs" && e.value.strip_prefix("dcs:").is_some_and(|module| {
                !module.is_empty() && module.trim()==module && module.len() <= 63 && module.bytes().all(|b| b.is_ascii_alphanumeric() || b"_- ".contains(&b))
            }))
            .map(|e|format!("game:{}",e.value))
            .collect::<BTreeSet<_>>();
        if scopes.len() != 1 { return Err("The game resource has no single matching native catalogue scope.".into()); }
        self.prepare_port_selection(
            id,
            port_id,
            expected_contract_hash,
            access,
            scopes.into_iter().next(),
        )
    }
    fn prepare_port_selection(
        &self,
        id: &str,
        port_id: &str,
        expected_contract_hash: &str,
        access: Access,
        selection_scope: Option<String>,
    ) -> Result<PortPreparation, String> {
        let entry = self.entry(id)?;
        let resolved =
            self.choose_mode_scope(entry, port_id, access, true, selection_scope.as_deref())?;
        let hash = resolved.port.contract_hash()?;
        if hash != expected_contract_hash {
            return Err("Resource contract changed before preparation; reconcile its exact local port expectation.".into());
        }
        let live = entry
            .live
            .get(&(resolved.scope.clone(), resolved.key.clone()))
            .unwrap();
        Ok(PortPreparation {
            pin: PreparationPin {
                inventory_session: self.session.clone(),
                resource_id: entry.saved.id.clone(),
                generation: entry.generation.to_string(),
                capability_revision: capability_revision(entry)?,
                provider_session: live.provider_session.clone(),
                scope: resolved.scope.clone(),
                key: resolved.key.clone(),
                route: resolved.route.clone(),
                port_id: port_id.into(),
                contract_hash: hash,
                access,
                selection_catalogue_revision: selection_scope
                    .as_ref()
                    .and(live.catalogue_revision.clone()),
                selection_scope,
                companion: None,
            },
            resolved,
        })
    }
    pub fn validate_preparation(
        &self,
        preparation: &PortPreparation,
    ) -> Result<ResolvedRoute, String> {
        if let Some(companion) = &preparation.pin.companion {
            let current = self.prepare_companion_port(
                &preparation.pin.resource_id,
                &companion.instance_id,
                &companion.publication_hash,
                &companion.capability_fingerprint,
                preparation.resolved.port.clone(),
                preparation.pin.access,
            )?;
            if current.pin != preparation.pin {
                return Err("The paired companion route changed after preparation.".into());
            }
            return Ok(current.resolved);
        }
        let mut current = self.prepare_port_selection(
            &preparation.pin.resource_id,
            &preparation.pin.port_id,
            &preparation.pin.contract_hash,
            preparation.pin.access,
            preparation.pin.selection_scope.clone(),
        )?;
        if preparation.pin.selection_scope.is_some() {
            // A separate outgoing/incoming delivery sighting may change the
            // aggregate entry generation. Metadata admission pins only its own
            // provider session, exact route and complete catalogue revision.
            current.pin.generation = preparation.pin.generation.clone();
            current.pin.capability_revision = preparation.pin.capability_revision.clone();
        }
        if current.pin != preparation.pin {
            return Err(
                "Resource identity, route, catalogue or generation changed after preparation."
                    .into(),
            );
        }
        Ok(current.resolved)
    }
    pub fn validate_token(
        &self,
        token: &ResourceToken,
        port_id: &str,
        access: Access,
    ) -> Result<ResolvedRoute, String> {
        if token.inventory_session != self.session
            || token.port_id != port_id
            || token.access != access
        {
            return Err("Stale inventory session or resource operation token.".into());
        }
        let current = self.token(&token.resource_id, port_id, access)?;
        if &current != token {
            return Err(
                "Resource route, capabilities or generation changed; capture a fresh token.".into(),
            );
        }
        self.choose(self.entry(&token.resource_id)?, port_id, access)
    }
    pub fn validate_invocation(
        &self,
        token: &ResourceToken,
        port_id: &str,
        arguments: &[PortValue],
    ) -> Result<ValidatedInvocation, String> {
        let resolved = self.validate_token(token, port_id, Access::Invoke)?;
        resolved.port.validate_arguments(arguments)?;
        Ok(ValidatedInvocation {
            token: token.clone(),
            resolved,
            arguments: arguments.to_vec(),
        })
    }
    pub fn check_assignment(
        &self,
        logical: &Resource,
        assignment: &LocalAssignment,
        expectations: &[PortExpectation],
    ) -> AssignmentReport {
        let mut diagnostics = Vec::new();
        let mut issue = |code: &str, port: Option<&str>, message: &str| {
            diagnostics.push(AssignmentDiagnostic {
                code: code.into(),
                resource_id: logical.id.clone(),
                port_id: port.map(str::to_owned),
                message: message.into(),
            })
        };
        if assignment.resource_id != logical.id {
            issue(
                "wrongLogicalResource",
                None,
                "The local assignment belongs to a different logical resource.",
            );
        }
        match self.entry(&assignment.source_id) {
            Err(_) => issue(
                "resourceMissing",
                None,
                "The assigned resource is remembered by the profile but is absent from inventory.",
            ),
            Ok(entry) => {
                let kind_matches = match logical.kind {
                    crate::ResourceKind::Device => matches!(
                        entry.saved.resource_type,
                        ResourceType::HostController | ResourceType::OsgModule
                    ),
                    crate::ResourceKind::Companion => {
                        entry.saved.resource_type == ResourceType::Companion
                    }
                    crate::ResourceKind::Game => {
                        entry.saved.resource_type == ResourceType::GameIntegration
                    }
                };
                if !kind_matches {
                    issue(
                        "resourceKindChanged",
                        None,
                        "The assigned resource has a different kind.",
                    );
                }
                if entry.live.is_empty() {
                    issue("resourceOffline",None,"The assigned resource is offline or unavailable; its blocks and bindings are retained.");
                }
                let ports = observed_ports(entry).unwrap_or_default();
                let mut seen = BTreeSet::new();
                for expected in expectations {
                    let logical_port = logical
                        .ports
                        .iter()
                        .find(|p| p.id == expected.profile_port_id);
                    if !seen.insert(expected.profile_port_id.as_str()) || logical_port.is_none() {
                        issue(
                            "invalidPortExpectation",
                            Some(&expected.profile_port_id),
                            "Port expectations must reference distinct logical ports.",
                        );
                        continue;
                    }
                    let port = port_variants(&ports, &expected.observed_port_id).iter()
                        .find(|p| p.contract_hash == expected.expected_contract_hash);
                    match port {
                        None=>issue("portContractChanged",Some(&expected.profile_port_id),"The assigned port is missing or its advertised contract changed; reconcile this binding."),
                        Some(port) if !crate::resources_ports::graph_compatible(&port.descriptor, logical_port.unwrap())=>issue("unrepresentablePortContract",Some(&expected.profile_port_id),"The portable port cannot represent this exact contract; structured actions and constrained text require an explicit later schema/processor."),
                        Some(port) if !port.current=>issue("portOffline",Some(&expected.profile_port_id),"This port is retained from an offline route."),
                        Some(_)=>{},
                    }
                }
                for port in &logical.ports {
                    if !seen.contains(port.id.as_str()) {
                        issue(
                            "portUnassigned",
                            Some(&port.id),
                            "Assign this logical port to an exact observed port and contract hash.",
                        );
                    }
                }
            }
        }
        AssignmentReport {
            valid: diagnostics.is_empty(),
            diagnostics,
        }
    }
    pub fn duplicate_sources(&self, assignments: &[LocalAssignment]) -> Vec<AssignmentDiagnostic> {
        let mut grouped: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for assignment in assignments {
            if let Ok(entry) = self.entry(&assignment.source_id) {
                grouped
                    .entry(entry.saved.id.clone())
                    .or_default()
                    .push(&assignment.resource_id);
            }
        }
        grouped.values().filter(|logical|logical.len()>1).flat_map(|logical|logical.iter().map(|id|AssignmentDiagnostic {code:"duplicatePhysicalSource".into(),resource_id:(*id).to_owned(),port_id:None,message:"Several logical resources resolve to the same proven physical source; choose one capture assignment.".into()})).collect()
    }
    fn entry(&self, id: &str) -> Result<&Entry, String> {
        let canonical = self.redirects.get(id).map(String::as_str).unwrap_or(id);
        self.entries
            .get(canonical)
            .map(Arc::as_ref)
            .ok_or_else(|| "Unknown persistent resource identity.".into())
    }
    fn locate(&mut self, scope: &str, observation: &Observation) -> Result<String, String> {
        let proof = verified(&observation.evidence);
        let game_proof = observation
            .evidence
            .iter()
            .filter(|e| e.kind == EvidenceKind::GameScope)
            .map(|e| e.value.as_str())
            .collect::<BTreeSet<_>>();
        let mut candidates = BTreeSet::new();
        for (id, entry) in &self.entries {
            if entry
                .saved
                .sightings
                .iter()
                .any(|s| s.scope == scope && s.key == observation.key)
            {
                let previous = entry
                    .saved
                    .sightings
                    .iter()
                    .flat_map(|s| verified(&s.evidence))
                    .collect::<BTreeSet<_>>();
                if !previous.is_empty() && !proof.is_empty() && previous.is_disjoint(&proof) {
                    return Err("Physical identity changed for a retained provider key; explicit reconciliation is required.".into());
                }
                candidates.insert(id.clone());
            }
            // Catalogue metadata and a native acquired reader are sightings of
            // one game integration. This evidence never joins physical devices.
            if observation.resource_type == ResourceType::GameIntegration
                && entry.saved.resource_type == ResourceType::GameIntegration
                && !game_proof.is_empty()
                && entry.saved.sightings.iter().any(|s| {
                    s.evidence.iter().any(|e| {
                        e.kind == EvidenceKind::GameScope && game_proof.contains(e.value.as_str())
                    })
                })
            {
                candidates.insert(id.clone());
            }
            if !proof.is_empty()
                && entry
                    .saved
                    .sightings
                    .iter()
                    .any(|s| !verified(&s.evidence).is_disjoint(&proof))
            {
                candidates.insert(id.clone());
            }
        }
        let id = if let Some(id) = candidates.first() {
            id.clone()
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            self.entries.insert(
                id.clone(),
                Arc::new(Entry::new(SavedResource {
                        id: id.clone(),
                        name: observation.name.clone(),
                        resource_type: observation.resource_type,
                        sightings: vec![],
                    })),
            );
            id
        };
        for other in candidates.iter().filter(|other| *other != &id) {
            let removed = self.entries.remove(other).unwrap();
            let removed = Arc::try_unwrap(removed).unwrap_or_else(|entry| (*entry).clone());
            let entry = Arc::make_mut(self.entries.get_mut(&id).unwrap());
            entry.invalidate_saved();
            if !compatible_kind(entry.saved.resource_type, removed.saved.resource_type) {
                return Err("Verified identity proof conflicts with resource kinds.".into());
            }
            if removed.saved.resource_type == ResourceType::OsgModule {
                entry.saved.resource_type = ResourceType::OsgModule;
            }
            entry.saved.sightings.extend(removed.saved.sightings);
            entry.sighting_validation.extend(removed.sighting_validation);
            entry.live.extend(removed.live);
            entry.generation = entry.generation.max(removed.generation);
            for target in self.redirects.values_mut() {
                if target == other {
                    *target = id.clone();
                }
            }
            self.redirects.insert(other.clone(), id.clone());
        }
        if !compatible_kind(
            self.entries[&id].saved.resource_type,
            observation.resource_type,
        ) {
            return Err("Provider key changed its resource kind.".into());
        }
        Ok(id)
    }
    fn choose(
        &self,
        entry: &Entry,
        port_id: &str,
        access: Access,
    ) -> Result<ResolvedRoute, String> {
        self.choose_mode(entry, port_id, access, false)
    }
    fn choose_mode(
        &self,
        entry: &Entry,
        port_id: &str,
        access: Access,
        preparing: bool,
    ) -> Result<ResolvedRoute, String> {
        self.choose_mode_scope(entry, port_id, access, preparing, None)
    }
    fn choose_mode_scope(
        &self,
        entry: &Entry,
        port_id: &str,
        access: Access,
        preparing: bool,
        selection_scope: Option<&str>,
    ) -> Result<ResolvedRoute, String> {
        if access != Access::Read {
            if let Some(reason) = write_conflict(entry) {
                return Err(reason.into());
            }
        }
        let mut candidates = Vec::new();
        for sighting in &entry.saved.sightings {
            if selection_scope.is_some_and(|scope| sighting.scope != scope) {
                continue;
            }
            let Some(live) = entry
                .live
                .get(&(sighting.scope.clone(), sighting.key.clone()))
            else {
                continue;
            };
            let Some(port) = sighting
                .catalogue
                .as_ref()
                .filter(|catalogue| live.catalogue_revision.as_ref() == Some(&catalogue.revision))
                .and_then(|c| c.ports.iter().find(|p| p.id == port_id))
            else {
                continue;
            };
            let may_acquire_game = preparing
                && entry.saved.resource_type == ResourceType::GameIntegration
                && live.route.transport == Transport::Integration;
            let allowed = match access {
                Access::Read => {
                    port.direction == crate::Direction::Output
                        && !matches!(port.contract, PortContract::Action { .. })
                        && (live.route.baseline_ready || preparing)
                }
                Access::Write => {
                    port.direction == crate::Direction::Input
                        && !matches!(port.contract, PortContract::Action { .. })
                        && (live.route.writable || may_acquire_game)
                }
                Access::Invoke => {
                    matches!(port.contract, PortContract::Action { .. })
                        && (live.route.writable || may_acquire_game)
                }
            };
            if allowed {
                candidates.push(ResolvedRoute {
                    resource_id: entry.saved.id.clone(),
                    scope: sighting.scope.clone(),
                    key: sighting.key.clone(),
                    route: live.route.clone(),
                    port: port.clone(),
                });
            }
        }
        candidates.sort_by(|a, b| {
            b.route
                .priority
                .cmp(&a.route.priority)
                .then(a.route.id.cmp(&b.route.id))
        });
        let selected = candidates
            .first()
            .ok_or("The resource port is offline, baseline-pending, read-only or unsupported.")?;
        if candidates
            .get(1)
            .is_some_and(|next| next.route.priority == selected.route.priority)
        {
            return Err("Ambiguous simultaneous resource routes; select one authoritative route before access.".into());
        }
        Ok(selected.clone())
    }
    fn summary(&self, entry: &Entry) -> Result<ResourceSummary, String> {
        let ports = observed_ports(entry)?;
        self.summary_with_port_count(entry, ports.len())
    }
    fn summary_with_port_count(
        &self,
        entry: &Entry,
        cached_port_count: usize,
    ) -> Result<ResourceSummary, String> {
        let mut attributes = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for sighting in &entry.saved.sightings {
            attributes.extend(sighting.attributes.clone());
            diagnostics.extend(sighting.diagnostics.clone());
            if let Some(message) = self.failures.get(&sighting.scope) {
                diagnostics.push(message.clone());
            }
            if entry
                .live
                .get(&(sighting.scope.clone(), sighting.key.clone()))
                .is_some_and(|live| live.catalogue_revision.is_none())
            {
                diagnostics.push("Wait for current-route capability verification; retained ports do not authorize this new connection.".into());
            }
        }
        if let Some(reason) = write_conflict(entry) {
            diagnostics.push(reason.into());
        }
        diagnostics.sort();
        diagnostics.dedup();
        let unavailable = entry
            .saved
            .sightings
            .iter()
            .any(|s| self.failures.contains_key(&s.scope));
        Ok(ResourceSummary {
            id: entry.saved.id.clone(),
            name: entry.saved.name.clone(),
            resource_type: entry.saved.resource_type,
            availability: if !entry.live.is_empty() {
                Availability::Online
            } else if unavailable {
                Availability::Unavailable
            } else {
                Availability::Offline
            },
            generation: entry.generation.to_string(),
            capability_revision: capability_revision(entry)?,
            port_count: cached_port_count.max(
                entry
                    .saved
                    .sightings
                    .iter()
                    .filter_map(|s| s.catalogue.as_ref().map(|c| c.total as usize))
                    .max()
                    .unwrap_or(0),
            ),
            cached_port_count,
            connections: entry
                .saved
                .sightings
                .iter()
                .filter_map(|s| {
                    entry
                        .live
                        .get(&(s.scope.clone(), s.key.clone()))
                        .map(|live| Connection {
                            scope: s.scope.clone(),
                            key: s.key.clone(),
                            route: live.route.clone(),
                        })
                })
                .collect(),
            attributes,
            diagnostics,
        })
    }
}

impl ProviderBatch {
    pub fn validate(&self) -> Result<(), String> {
        text(&self.scope, 256)?;
        text(&self.session, 256)?;
        if self.revision == 0 || self.observations.len() > MAX_OBSERVATIONS {
            return Err("Invalid resource provider revision or observation bound.".into());
        }
        let mut keys = BTreeSet::new();
        let mut routes = BTreeSet::new();
        for observation in &self.observations {
            validate_sighting(
                &SavedSighting {
                    scope: self.scope.clone(),
                    key: observation.key.clone(),
                    evidence: observation.evidence.clone(),
                    attributes: observation.attributes.clone(),
                    diagnostics: observation.diagnostics.clone(),
                    catalogue: None,
                },
                observation.resource_type,
            )?;
            text(&observation.name, 320)?;
            if !keys.insert(&observation.key) {
                return Err("Duplicate provider observation key.".into());
            }
            if let Some(route) = &observation.route {
                text(&route.id, 512)?;
                text(&route.provider_generation, 256)?;
                metadata(&route.attributes)?;
                if !routes.insert(&route.id) {
                    return Err("Duplicate provider route identity.".into());
                }
            }
            if let Some(ports) = &observation.ports {
                ports.validate()?;
            }
        }
        Ok(())
    }
}
impl SavedRegistry {
    pub fn validate(&self) -> Result<(), String> {
        // Count serialization without allocating a potentially enormous JSON
        // buffer. Structural limits alone permit large enum/action catalogues.
        let mut limit = InventoryLimit { written: 0 };
        serde_json::to_writer(&mut limit, self)
            .map_err(|_| "Resource inventory exceeds its 128 MiB serialized envelope.".to_owned())?;
        if self.schema_version != 1
            || self.resources.len() > MAX_RESOURCES
            || self.redirects.len() > MAX_RESOURCES
        {
            return Err("Unsupported or oversized resource inventory envelope.".into());
        }
        let mut ids = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        let mut proofs = BTreeMap::new();
        let mut count = 0;
        for resource in &self.resources {
            canonical_uuid(&resource.id)?;
            text(&resource.name, 320)?;
            if !ids.insert(resource.id.as_str())
                || resource.sightings.is_empty()
                || resource.sightings.len() > 64
            {
                return Err("Invalid resource identity or sighting count.".into());
            }
            let mut physical = BTreeSet::new();
            for sighting in &resource.sightings {
                validate_sighting(sighting, resource.resource_type)?;
                if !aliases.insert((&sighting.scope, &sighting.key)) {
                    return Err("A provider identity aliases several persistent resources.".into());
                }
                physical.extend(verified(&sighting.evidence));
                if let Some(catalogue) = &sighting.catalogue {
                    count += catalogue.ports.len();
                }
            }
            if physical.len() > 1 {
                return Err("Conflicting verified physical identity evidence.".into());
            }
            for proof in physical {
                if proofs.insert(proof, &resource.id).is_some() {
                    return Err(
                        "Verified physical identity is duplicated instead of correlated.".into(),
                    );
                }
            }
        }
        if count > MAX_CACHED_PORTS {
            return Err("Resource inventory exceeds 262144 cached ports.".into());
        }
        for (alias, target) in &self.redirects {
            canonical_uuid(alias)?;
            if ids.contains(alias.as_str()) || !ids.contains(target.as_str()) {
                return Err(
                    "Invalid resource redirect; cycles and dangling aliases are rejected.".into(),
                );
            }
        }
        Ok(())
    }
}
struct InventoryLimit {
    written: usize,
}
impl std::io::Write for InventoryLimit {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > crate::resources_store::MAX_INVENTORY_BYTES.saturating_sub(self.written) {
            return Err(std::io::Error::other("Inventory byte limit"));
        }
        self.written += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn canonical_uuid(id: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id) {
        Ok(())
    } else {
        Err("Resource identity must be a canonical UUID.".into())
    }
}
fn metadata(values: &BTreeMap<String, String>) -> Result<(), String> {
    if values.len() > 32 {
        return Err("Too many resource attributes.".into());
    }
    for (k, v) in values {
        text(k, 80)?;
        text(v, 4096)?;
    }
    Ok(())
}
fn validate_sighting(sighting: &SavedSighting, kind: ResourceType) -> Result<(), String> {
    validate_sighting_metadata(sighting,kind)?;
    if let Some(catalogue) = &sighting.catalogue {
        text(&catalogue.revision, 128)?;
        validate_ports(&catalogue.ports)?;
        if catalogue.total as usize > MAX_CATALOGUE_PORTS
            || catalogue.ports.len() > catalogue.total as usize
            || (catalogue.complete && catalogue.ports.len() != catalogue.total as usize)
            || catalogue.hash != crate::resources_ports::catalogue_hash(&catalogue.ports)?
        {
            return Err("Corrupt resource port catalogue or fingerprint.".into());
        }
    }
    Ok(())
}
fn validate_sighting_metadata(sighting: &SavedSighting, kind: ResourceType) -> Result<(), String> {
    text(&sighting.scope, 256)?;
    text(&sighting.key, 512)?;
    metadata(&sighting.attributes)?;
    if sighting.evidence.len() > 16 || sighting.diagnostics.len() > 16 {
        return Err("Resource evidence or diagnostics exceed their bounds.".into());
    }
    let mut seen = BTreeSet::new();
    for evidence in &sighting.evidence {
        text(&evidence.value, 2048)?;
        if !seen.insert((evidence.kind, &evidence.value)) {
            return Err("Duplicate resource identity evidence.".into());
        }
        if evidence.kind == EvidenceKind::GameScope && kind != ResourceType::GameIntegration {
            return Err("Game identity evidence belongs to a game integration.".into());
        }
        if evidence.kind == EvidenceKind::VerifiedPhysical
            && !matches!(kind, ResourceType::HostController | ResourceType::OsgModule)
        {
            return Err("Physical identity proof belongs to a physical resource.".into());
        }
    }
    if verified(&sighting.evidence).len() > 1 {
        return Err("Conflicting physical identity claims.".into());
    }
    for message in &sighting.diagnostics {
        text(message, 2048)?;
    }
    Ok(())
}
fn verified(evidence: &[IdentityEvidence]) -> BTreeSet<&str> {
    evidence
        .iter()
        .filter(|e| e.kind == EvidenceKind::VerifiedPhysical)
        .map(|e| e.value.as_str())
        .collect()
}
fn compatible_kind(a: ResourceType, b: ResourceType) -> bool {
    a == b
        || matches!(
            (a, b),
            (ResourceType::HostController, ResourceType::OsgModule)
                | (ResourceType::OsgModule, ResourceType::HostController)
        )
}
fn write_conflict(entry: &Entry) -> Option<&'static str> {
    if !entry
        .saved
        .sightings
        .iter()
        .any(|s| !verified(&s.evidence).is_empty())
    {
        return None;
    }
    let physical = entry
        .live
        .values()
        .filter(|l| matches!(l.route.transport, Transport::Can | Transport::Usb))
        .collect::<Vec<_>>();
    if physical
        .iter()
        .filter(|l| l.route.transport == Transport::Can)
        .count()
        > 1
    {
        return Some(
            "Ambiguous physical identity on multiple live CAN routes; routed writes are blocked.",
        );
    }
    let boots = physical
        .iter()
        .filter_map(|l| l.route.attributes.get("boot"))
        .collect::<BTreeSet<_>>();
    if boots.len() > 1 {
        return Some("Conflicting USB/CAN boot evidence for one physical identity; routed writes are blocked.");
    }
    None
}
fn page(offset: usize, limit: usize, total: usize) -> Result<(), String> {
    if offset > total || limit == 0 || limit > MAX_PORT_PAGE {
        Err("Invalid bounded resource inventory page.".into())
    } else {
        Ok(())
    }
}
fn capability_revision(entry: &Entry) -> Result<String, String> {
    let revisions = entry
        .saved
        .sightings
        .iter()
        .filter_map(|s| s.catalogue.as_ref().map(|c| (&c.revision, c.total)))
        .collect::<BTreeSet<_>>();
    digest(&revisions)
}
fn fingerprint(entry: &Entry) -> Result<String, String> {
    let routes = entry
        .live
        .iter()
        .map(|(key, live)| {
            (
                key,
                &live.route,
                &live.provider_session,
                &live.catalogue_revision,
            )
        })
        .collect::<Vec<_>>();
    digest(&(routes, capability_revision(entry)?))
}
fn observed_ports(entry: &Entry) -> Result<Arc<Vec<ObservedPort>>, String> {
    entry.observed.get_or_init(|| derive_observed_ports(entry).map(Arc::new)).clone()
}
// Variants are ordered by (native port identity, exact contract hash), so
// selected capture/assignment lookup is logarithmic without rebuilding an
// index or cloning unrelated descriptors on each request.
fn port_variants<'a>(ports: &'a [ObservedPort], id: &str) -> &'a [ObservedPort] {
    let start = ports.partition_point(|port| port.descriptor.id.as_str() < id);
    let end = ports[start..].partition_point(|port| port.descriptor.id == id) + start;
    &ports[start..end]
}
fn derive_observed_ports(entry: &Entry) -> Result<Vec<ObservedPort>, String> {
    let mut ports: BTreeMap<(String, String), ObservedPort> = BTreeMap::new();
    for sighting in &entry.saved.sightings {
        if let Some(catalogue) = &sighting.catalogue {
            let validated=entry.validated_sighting(sighting)?;
            for (descriptor,hash) in catalogue.ports.iter().zip(&validated.contract_hashes) {
                let hash=hash.clone();
                let port = ports
                    .entry((descriptor.id.clone(), hash.clone()))
                    .or_insert_with(|| ObservedPort {
                        descriptor: descriptor.clone(),
                        contract_hash: hash,
                        route_ids: vec![],
                        current: false,
                    });
                if let Some(live) = entry
                    .live
                    .get(&(sighting.scope.clone(), sighting.key.clone()))
                    .filter(|live| live.catalogue_revision.as_ref() == Some(&catalogue.revision))
                {
                    port.route_ids.push(live.route.id.clone());
                    port.current = true;
                }
            }
        }
    }
    for port in ports.values_mut() {
        port.route_ids.sort();
        port.route_ids.dedup();
    }
    Ok(ports.into_values().collect())
}
fn adopt_ports(
    update: &PortUpdate,
    previous: Option<&SavedCatalogue>,
) -> Result<SavedCatalogue, String> {
    if let Some(previous) =
        previous.filter(|p| p.revision == update.revision && p.complete && update.complete)
    {
        if previous.hash != crate::resources_ports::catalogue_hash(&update.ports)? {
            return Err(
                "Provider changed a complete catalogue without changing its revision.".into(),
            );
        }
    }
    let mut ports = BTreeMap::new();
    if let Some(previous) = previous.filter(|c| c.revision == update.revision) {
        if previous.total != update.total {
            return Err("Provider reused a catalogue revision with a different total.".into());
        }
        for port in &previous.ports {
            ports.insert(port.id.clone(), port.clone());
        }
    }
    for port in &update.ports {
        if ports
            .get(&port.id)
            .is_some_and(|old| old.contract_hash().ok() != port.contract_hash().ok())
        {
            return Err(
                "Provider changed a port contract without changing its catalogue revision.".into(),
            );
        }
        ports.insert(port.id.clone(), port.clone());
    }
    if update.complete {
        ports = update
            .ports
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
    }
    if ports.len() > update.total as usize {
        return Err("Port pages exceed their declared catalogue total.".into());
    }
    let ports = ports.into_values().collect::<Vec<_>>();
    Ok(SavedCatalogue {
        revision: update.revision.clone(),
        total: update.total,
        complete: ports.len() == update.total as usize,
        hash: crate::resources_ports::catalogue_hash(&ports)?,
        ports,
    })
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "resources_preparation_tests.rs"]
mod preparation_tests;

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::resources_ports::{ScalarSchema, PortContract};

    #[test]
    fn small_delivery_reuses_large_sighting_validation_and_preserves_exact_metadata_fences() {
        let mut registry = Registry::new();
        let mut metadata = batch("game:x-plane-12",1,63_607);
        metadata.observations[0].evidence[0].value="x-plane-12".into();
        let start=std::time::Instant::now();
        registry.reconcile(metadata).unwrap();
        println!("Large initial reconcile {:?}",start.elapsed());
        let id=registry.list(0,1).unwrap().resources[0].id.clone();
        registry.saved_fingerprint().unwrap();
        let key=("game:x-plane-12".into(),"game:x-plane-12".into());
        let cached=registry.entries[&id].sighting_validation[&key].clone();
        let hash=registry.entries[&id].saved.sightings[0].catalogue.as_ref().unwrap().ports[0].contract_hash().unwrap();
        let preparation=registry.prepare_game_metadata_port(&id,"channel:00000",&hash,Access::Read,"x-plane-12").unwrap();
        let mut delivery=batch("profile-game:x-plane-12",1,15);
        delivery.observations[0].evidence[0].value="x-plane-12".into();
        let start=std::time::Instant::now();
        registry.reconcile(delivery.clone()).unwrap();
        println!("Small selected delivery reconcile {:?}",start.elapsed());
        assert!(Arc::ptr_eq(&cached,&registry.entries[&id].sighting_validation[&key]));
        assert_eq!(registry.validate_preparation(&preparation).unwrap().scope,"game:x-plane-12");
        assert_eq!(registry.entries[&id].validated_saved().unwrap().serialized_bytes,serde_json::to_vec(&registry.entries[&id].saved).unwrap().len());
        let start=std::time::Instant::now();
        registry.inspect(&id,0,15).unwrap();
        println!("First observed delivery projection {:?}",start.elapsed());
        delivery.revision=2;
        let start=std::time::Instant::now();
        registry.reconcile(delivery).unwrap();
        println!("Repeated selected delivery reconcile {:?}",start.elapsed());
        assert!(Arc::ptr_eq(&cached,&registry.entries[&id].sighting_validation[&key]));
        assert_eq!(registry.validate_preparation(&preparation).unwrap().scope,"game:x-plane-12");
        let restored=Registry::restore(registry.saved()).unwrap();
        assert_eq!(restored.saved_fingerprint().unwrap(),registry.saved_fingerprint().unwrap());
    }

    fn batch(scope: &str, revision: u64, count: usize) -> ProviderBatch {
        let ports = (0..count).map(|index| PortDescriptor {
            id: format!("channel:{index:05}"), name: format!("Native channel {index}"),
            direction: crate::Direction::Output,
            contract: PortContract::State { schema: ScalarSchema::Number { min:-10.,max:10.,unit:None }, freshness_ms:1500, fallback:None },
        }).collect();
        ProviderBatch { scope:scope.into(),session:"cache-provider".into(),revision,complete:true,observations:vec![Observation {
            key:scope.into(),name:scope.into(),resource_type:ResourceType::GameIntegration,
            evidence:vec![IdentityEvidence {kind:EvidenceKind::GameScope,value:scope.into()}],attributes:BTreeMap::new(),diagnostics:vec![],
            route:Some(Route {id:format!("route:{scope}"),transport:Transport::Integration,provider_generation:"1".into(),attributes:BTreeMap::new(),baseline_ready:true,writable:false,priority:0}),
            ports:Some(PortUpdate::complete(ports).unwrap()),
        }] }
    }
    #[test]
    fn large_catalogue_pages_and_unrelated_transactions_share_immutable_projections() {
        let mut registry = Registry::new();
        registry.reconcile(batch("large-sdk",1,65_536)).unwrap();
        let id = registry.list(0,1).unwrap().resources[0].id.clone();
        let entry = registry.entries[&id].clone();
        let captured = observed_ports(&entry).unwrap();
        let validated = entry.validated_saved().unwrap();
        let token = registry.token(&id,"channel:65535",Access::Read).unwrap();
        for page in 0..100 {
            let offset = page * 128;
            let details = registry.inspect(&id,offset,128).unwrap();
            assert_eq!(details.total,65_536);
            assert_eq!(details.ports.len(),128);
            assert_eq!(details.ports[0].descriptor.id,format!("channel:{offset:05}"));
            assert_eq!(registry.list(0,1).unwrap().resources[0].cached_port_count,65_536);
        }
        let selected = registry.inspect_selected(&id,&[PortExpectation {
            profile_port_id:"selected".into(),observed_port_id:"channel:65535".into(),expected_contract_hash:captured.last().unwrap().contract_hash.clone(),
        }]).unwrap();
        assert_eq!(selected.ports.len(),1);
        assert!(Arc::ptr_eq(&captured,&observed_ports(registry.entry(&id).unwrap()).unwrap()));
        registry.reconcile(batch("unrelated",1,1)).unwrap();
        assert!(Arc::ptr_eq(&entry,&registry.entries[&id]));
        assert!(Arc::ptr_eq(&validated,&registry.entry(&id).unwrap().validated_saved().unwrap()));
        assert!(Arc::ptr_eq(&captured,&observed_ports(registry.entry(&id).unwrap()).unwrap()));
        registry.validate_token(&token,"channel:65535",Access::Read).unwrap();
        registry.provider_failed("unrelated","Offline fixture");
        assert!(Arc::ptr_eq(&entry,&registry.entries[&id]));
        registry.provider_failed("large-sdk","SDK fixture disconnected");
        let offline = observed_ports(registry.entry(&id).unwrap()).unwrap();
        assert!(!Arc::ptr_eq(&captured,&offline));
        assert!(offline.iter().all(|port| !port.current && port.route_ids.is_empty()));
        assert!(registry.validate_token(&token,"channel:65535",Access::Read).is_err());
    }
    #[test]
    fn cache_never_accepts_a_same_revision_changed_contract_or_changes_failed_transaction() {
        let mut registry = Registry::new();
        let original = batch("sdk",1,2);
        registry.reconcile(original.clone()).unwrap();
        let id = registry.list(0,1).unwrap().resources[0].id.clone();
        let before = registry.entries[&id].clone();
        let cached = observed_ports(&before).unwrap();
        let revision = registry.revision();
        let mut changed = original;
        changed.revision = 2;
        changed.observations[0].ports.as_mut().unwrap().ports[0].name = "Changed native label".into();
        // The semantic contract itself must also be checked; labels alone are
        // intentionally outside its exact contract fingerprint.
        if let PortContract::State {freshness_ms,..} = &mut changed.observations[0].ports.as_mut().unwrap().ports[0].contract { *freshness_ms=2000; }
        assert!(registry.reconcile(changed).is_err());
        assert_eq!(registry.revision(),revision);
        assert!(Arc::ptr_eq(&before,&registry.entries[&id]));
        assert!(Arc::ptr_eq(&cached,&observed_ports(registry.entry(&id).unwrap()).unwrap()));
        registry.saved().validate().unwrap();
    }
}

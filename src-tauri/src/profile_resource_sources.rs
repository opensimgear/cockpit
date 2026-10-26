//! Read-only provider composition. Transport topology remains resource metadata.
#[path = "profile_hid_publication.rs"]
pub(crate) mod hid_publication;
use crate::{
    profile_resource_ports as ports,
    profile_resources::{attributes, batch, Collect},
};
use osg_profiles::resources::{
    EvidenceKind, IdentityEvidence, Observation, ProviderBatch, ResourceType, Route, Transport,
};
use osg_profiles::resources_ports::PortUpdate;
use osg_profiles::Direction;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub(crate) type Gateways = Arc<dyn Fn() -> Vec<(String, Value)> + Send + Sync>;
pub(crate) type Panel = Arc<dyn Fn(Value) -> Value + Send + Sync>;
#[derive(Clone)]
struct GameCatalogue {
    setup_revision: String,
    catalogue_revision: String,
    ports: PortUpdate,
    aircraft_module: Option<String>,
    vehicle_aliases: Option<String>,
}
impl GameCatalogue {
    fn parse(catalogue: Value) -> Result<Self, String> {
        let revision = |field: &str| {
            catalogue[field]
                .as_str()
                .filter(|value| {
                    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
                })
                .map(str::to_owned)
                .ok_or_else(|| format!("The game-owned {field} is missing."))
        };
        Ok(Self {
            setup_revision: revision("providerSetupRevision")?,
            catalogue_revision: revision("catalogueRevision")?,
            ports: PortUpdate::complete(ports::integration(&catalogue)?)?,
            aircraft_module: catalogue["aircraftModule"].as_str().map(str::to_owned),
            vehicle_aliases: catalogue["vehicleAliases"]
                .as_array()
                .map(|aliases| json!(aliases).to_string()),
        })
    }
}
struct Sources {
    session: String,
    controllers: Arc<osg_controller_input::ControllerService>,
    companions: Arc<osg_mapping_host::companions::CompanionProvider>,
    host: Arc<crate::companion_host::Runtime>,
    games: Arc<crate::games::GamesRuntime>,
    gateways: Gateways,
    panel: Panel,
    native: bool,
    usb: Arc<crate::module_usb_fleet::Fleet>,
    can_scopes: BTreeSet<String>,
    can_retained: BTreeMap<String, Vec<Observation>>,
    refreshing: Arc<Mutex<BTreeSet<String>>>,
    catalogues: BTreeMap<String, (Instant, String, Result<GameCatalogue, String>)>,
}
pub(crate) fn collector(
    controllers: Arc<osg_controller_input::ControllerService>,
    companions: Arc<osg_mapping_host::companions::CompanionProvider>,
    host: Arc<crate::companion_host::Runtime>,
    games: Arc<crate::games::GamesRuntime>,
    gateways: Gateways,
    panel: Panel,
    usb: Arc<crate::module_usb_fleet::Fleet>,
    native: bool,
) -> Collect {
    let sources = Mutex::new(Sources {
        session: uuid::Uuid::new_v4().to_string(),
        controllers,
        companions,
        host,
        games,
        gateways,
        panel,
        native,
        usb,
        can_scopes: BTreeSet::new(),
        can_retained: BTreeMap::new(),
        refreshing: Arc::new(Mutex::new(BTreeSet::new())),
        catalogues: BTreeMap::new(),
    });
    Arc::new(move |revision| {
        sources
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .collect(revision)
    })
}
fn evidence(kind: EvidenceKind, value: String) -> IdentityEvidence {
    IdentityEvidence { kind, value }
}
fn observation(key: String, name: String, resource_type: ResourceType) -> Observation {
    Observation {
        key,
        name,
        resource_type,
        evidence: vec![],
        attributes: BTreeMap::new(),
        diagnostics: vec![],
        route: None,
        ports: None,
    }
}
include!("games/profiles/inventory_collection.rs");
include!("games/profiles/inventory_devices.rs");
include!("games/profiles/inventory_can.rs");

fn hex(value: &Option<String>) -> Option<u16> {
    value
        .as_deref()
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
}

#[cfg(test)]
#[path = "profile_resource_sources_tests.rs"]
mod tests;

// The ignored browser proof uses the production projection, not a fabricated
// registry descriptor or token. This exposes nothing in the production build.
#[cfg(test)]
pub(crate) fn companion_observation(
    source: osg_mapping_host::companions::ResourceDescriptor,
    publication: Result<osg_companion_service::Publication, String>,
    transport: Option<osg_companion_service::SourceTransport>,
) -> Result<Observation, String> {
    Sources::companion_observation(source, publication, transport)
}

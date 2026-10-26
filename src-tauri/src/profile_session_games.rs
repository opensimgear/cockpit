//! Read-only acquisition and context-fenced commands over the existing native
//! simulator transports. All bindings are derived from the captured profile.
use crate::{
    dcs_profile::ExportSnapshot,
    game_context::ContextToken,
    games::{GamesRuntime, PreparedGame},
};
use osg_mapping::{
    engine::{
        typed::{Change, Effect, Frame},
        Counter, Reason, ReportReason,
    },
    program::{SignalSample, SignalValue},
};
use osg_profiles::{
    compiler_models::{ExecutionPlan, SinkAdapter},
    resources::{
        EvidenceKind, IdentityEvidence, Observation, ProviderBatch, ResourceType, Route, Transport,
    },
    resources_ports::{PortContract, PortDescriptor, PortUpdate, PortValue},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

struct SourcePort {
    logical: String,
    native: String,
    contract: PortContract,
}
#[derive(Default)]
struct SourceState {
    ports: Vec<SourcePort>,
    generation: u64,
    sequence: u64,
    epoch: u64,
    counter: Option<u16>,
    available: bool,
    physics_witness: Option<PhysicsWitness>,
    physics_epoch: Option<(Option<String>, u64)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct PhysicsWitness {
    session: Option<String>,
    epoch: u64,
    sequence: Option<u32>,
    fresh: bool,
}
struct PhysicsMerge {
    witness: PhysicsWitness,
    keys: BTreeSet<String>,
    stale: BTreeSet<String>,
}
/// A complete ownship frame is independent of the BIOS counter. Missing or
/// unqualified frames withdraw the family; a missing component of a qualified
/// frame stays Unknown without discarding its valid neighbours.
fn merge_dcs_physics(
    snapshot: &mut ExportSnapshot,
    outputs: &[Value],
    physics: Option<&crate::dcs_physics::Snapshot>,
) -> PhysicsMerge {
    let fresh = snapshot.fresh
        && physics.is_some_and(|value| value.fresh && value.aircraft == snapshot.aircraft);
    let witness = PhysicsWitness {
        session: physics.and_then(|value| value.session.clone()),
        epoch: physics.map_or(0, |value| value.epoch),
        sequence: physics.and_then(|value| value.sequence),
        fresh,
    };
    let keys = outputs
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for key in &keys {
        snapshot.values.remove(key);
    }
    if fresh {
        let values = crate::game_channels::values("dcs", &physics.unwrap().values);
        for output in outputs {
            if let (Some(id), Some(value)) = (
                output["id"].as_str(),
                output["channel"]
                    .as_str()
                    .and_then(|channel| values.get(channel)),
            ) {
                snapshot.values.insert(id.into(), value.clone());
            }
        }
    }
    PhysicsMerge {
        witness,
        stale: if fresh { BTreeSet::new() } else { keys.clone() },
        keys,
    }
}
struct CommandPort {
    descriptor: PortDescriptor,
    command: String,
}
struct DeliveryCatalogue {
    aircraft_module: Option<String>,
    revision: String,
    ports: BTreeMap<String, PortDescriptor>,
}
enum Reader {
    Dcs {
        network: crate::dcs_bios::Network,
        transport: Option<crate::dcs_bios::Transport>,
        decoder: crate::dcs_profile::ExportSession,
        catalogue_guard: crate::dcs_profile::DcsCatalogueGuard,
        physics_outputs: Vec<Value>,
    },
    Xplane {
        connection: Value,
        outputs: Vec<Value>,
        canonical_outputs: Vec<Value>,
        source: Option<crate::xplane::Source>,
    },
    Acr {
        outputs: Vec<Value>,
        source: Option<Box<dyn crate::game_telemetry::Source>>,
        decoder: crate::game_telemetry::Session,
    },
}
pub(crate) struct GameSources {
    game: Option<PreparedGame>,
    reader: Option<Reader>,
    sources: BTreeMap<String, SourceState>,
    commands: BTreeMap<(String, String), CommandPort>,
    delivery_catalogues: BTreeMap<String, DeliveryCatalogue>,
    context: Option<ContextToken>,
    fresh: bool,
    lost: bool,
    epoch: u64,
    revision: u64,
    last_command_sequence: u64,
    revoked_commands: BTreeSet<(String, String)>,
    witness: Option<(u64, u16)>,
    witnessed_progress: bool,
    reader_session: String,
    acquired: bool,
    published: Option<(u64, bool, bool)>,
    physics: Option<PhysicsMerge>,
}
#[derive(Default)]
pub(crate) struct GamePoll {
    pub frames: Vec<Frame>,
    pub diagnostics: Vec<String>,
    pub context_lost: bool,
    pub fresh: bool,
}
fn native_key(resource: &str, port: &str) -> String {
    format!("{resource}/{port}")
}
fn game_route(route: Option<&Route>, game: &str) -> bool {
    route.is_some_and(|route| {
        route.transport == Transport::Integration
            && (route.id == format!("game:{game}")
                || route.attributes.get("gameId").is_some_and(|id| id == game))
    })
}
fn compatible_game_route(route: Option<&Route>, game: &PreparedGame) -> bool {
    game_route(route, &game.proof.game_id)
        && (game.integration_key != "dcs"
            || route
                .and_then(|route| route.attributes.get("aircraftModule"))
                .is_none_or(|module| {
                    module == "CommonData" || Some(module) == game.aircraft_module.as_ref()
                }))
}
fn scalar(value: &Value) -> Option<PortValue> {
    if let Some(value) = value.as_bool() {
        Some(PortValue::Boolean(value))
    } else if let Some(value) = value.as_f64().filter(|value| value.is_finite()) {
        Some(PortValue::Number(value))
    } else {
        value.as_str().map(|value| PortValue::Text(value.into()))
    }
}
fn semantic_contract(mut contract: PortContract) -> PortContract {
    if let PortContract::Action { parameters, .. } | PortContract::Invocation { parameters, .. } =
        &mut contract
    {
        for parameter in parameters {
            parameter.name = parameter.id.clone();
        }
    }
    contract
}
include!("games/profiles/session_prepare.rs");
include!("games/profiles/session_read.rs");
include!("games/profiles/session_delivery.rs");

impl Drop for GameSources {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
pub(crate) fn test_baseline_reader() -> (GameSources, std::net::SocketAddr, tempfile::TempDir) {
    tests::baseline_reader()
}
#[cfg(test)]
pub(crate) fn test_baseline_export(counter: u8, on: bool) -> Vec<u8> {
    tests::bytes(counter, on)
}

#[cfg(test)]
#[path = "profile_session_games_tests.rs"]
mod tests;

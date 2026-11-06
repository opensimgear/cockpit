//! Read-only acquisition and context-fenced commands over the existing native
//! simulator transports. All bindings are derived from the captured profile.
use crate::{
    dcs_profile::ExportSnapshot,
    game_context::ContextToken,
    games::{GamesRuntime, PreparedGame},
};
use osg_mapping::{
    engine::{
        typed::{Change, Effect, Frame, native::*},
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
    family: String,
    policy: NativePolicy,
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
    physics_epoch: Option<(Option<String>, Option<String>, u64)>,
    families: BTreeMap<String, native_admission::FamilyState>,
    replacement: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct PhysicsWitness {
    producer_id: Option<String>,
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
    let fresh = physics.is_some_and(|value| value.fresh && value.aircraft.is_some() && snapshot.aircraft.as_ref().is_none_or(|aircraft| Some(aircraft) == value.aircraft.as_ref()));
    let witness = PhysicsWitness {
        producer_id: physics.map(|value| value.producer_id.clone()),
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
        let values = physics.unwrap().captured_values(Instant::now());
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
        transport: Option<crate::dcs_bios::decoded::Source>,
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
    clock: osg_controller_input::stream::Clock,
    fatal: Option<String>,
    bios_retry_at: Option<Instant>,
    notifications: source_wakes::SourceWakes,
    #[cfg(test)]
    authored_fixture: bool,
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
    validation: live_validation::Validation,
    catalogue_validation: live_validation::Validation,
}
#[derive(Default)]
pub(crate) struct GamePoll {
    pub frames: Vec<Admission>,
    pub diagnostics: Vec<String>,
    pub fatal: Option<String>,
    pub context_lost: bool,
    pub fresh: bool,
}
impl GamePoll {
    /// Keep already-numbered native facts in the same atomic drain as their
    /// later revocation. The reducer installs every loss before replaying work.
    pub(crate) fn revoked_by(mut self, mut loss: Self) -> Self {
        self.frames.append(&mut loss.frames);
        self.diagnostics.append(&mut loss.diagnostics);
        loss.frames = self.frames;
        loss.diagnostics = self.diagnostics;
        loss.fatal = loss.fatal.or(self.fatal);
        loss.context_lost |= self.context_lost;
        loss
    }
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
fn scalar(captured: &crate::native_capture::CapturedValue) -> Option<PortValue> {
    if !captured.is_current(Instant::now()) {return None;}
    scalar_current(captured)
}
// The native frame builder already checked the paired value's validity. The
// reducer rechecks original deadlines at admission, without a second proof walk.
fn scalar_current(captured: &crate::native_capture::CapturedValue) -> Option<PortValue> {
    let value=captured.value();
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
#[path = "games/profiles/session_validation.rs"]
mod live_validation;
#[path = "games/profiles/native_admission.rs"]
mod native_admission;
#[path = "games/profiles/native_tables.rs"]
mod native_tables;
#[path = "games/profiles/source_wakes.rs"]
mod source_wakes;
include!("games/profiles/session_prepare.rs");
include!("games/profiles/session_read.rs");
include!("games/profiles/session_delivery.rs");

impl GameSources {
    fn authored_transport_fixture(&self) -> bool {
        #[cfg(test)]
        { self.authored_fixture }
        #[cfg(not(test))]
        { false }
    }
}

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
#[cfg(test)]
pub(crate) use tests::{watched_assetto_scheduling_fixture, watched_xplane_scheduling_fixture};

#[cfg(test)]
#[path = "games/profiles/native_admission_tests.rs"]
mod native_admission_tests;

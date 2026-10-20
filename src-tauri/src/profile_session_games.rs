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
}
struct CommandPort {
    descriptor: PortDescriptor,
    command: String,
}
enum Reader {
    Dcs {
        network: crate::dcs_bios::Network,
        transport: Option<crate::dcs_bios::Transport>,
        decoder: crate::dcs_profile::ExportSession,
        catalogue_guard: crate::dcs_profile::DcsCatalogueGuard,
    },
    Xplane {
        connection: Value,
        outputs: Vec<Value>,
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
    descriptors: Vec<PortDescriptor>,
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
impl GameSources {
    pub(crate) fn needs_context(&self) -> bool { self.game.is_none() || self.lost }
    pub(crate) fn preparation_session(&self)->Option<&str>{self.game.as_ref().map(|game|game.proof.session.as_str())}
    pub(crate) fn accepts_replacement(&self, game: &PreparedGame) -> bool {
        self.game.as_ref().is_none_or(|old| old.proof.session != game.proof.session)
    }
    pub(crate) fn inherit_source_counters(&mut self, previous: &Self) {
        for (id, source) in &mut self.sources {
            if let Some(old)=previous.sources.get(id) {
                source.generation=old.generation.saturating_add(1);
                source.sequence=old.sequence;
            }
        }
    }
    pub(crate) fn can_command(&self) -> bool { !self.lost && self.fresh && self.context.is_some() }
    pub(crate) fn has_delivery_publication(&self)->bool{self.revision>0}
    pub(crate) fn prepare(
        plan: &ExecutionPlan,
        game: Option<PreparedGame>,
    ) -> Result<Self, String> {
        let mut result = Self {
            game,
            reader: None,
            sources: BTreeMap::new(),
            commands: BTreeMap::new(),
            descriptors: vec![],
            context: None,
            fresh: false,
            lost: false,
            epoch: 1,
            revision: 0,
            last_command_sequence: 0,
            revoked_commands: BTreeSet::new(),
            witness: None,
            witnessed_progress: false,
            reader_session: uuid::Uuid::new_v4().to_string(),
            acquired: false,
            published: None,
        };
        let Some(game) = &result.game else {
            return Ok(result);
        };
        let catalogue = crate::profile_resource_ports::integration(&game.catalogue)?
            .into_iter()
            .map(|port| (port.id.clone(), port))
            .collect::<BTreeMap<_, _>>();
        let mut rows = vec![];
        let mut descriptors = BTreeMap::new();
        for source in &plan.sources {
            if !game_route(source.route.as_ref(), &game.proof.game_id) {
                continue;
            }
            let native = source
                .observed_port_id
                .as_ref()
                .ok_or("Game source has no exact captured native port.")?;
            let descriptor = catalogue
                .get(native)
                .ok_or("The game source disappeared from its captured catalogue.")?;
            if semantic_contract(descriptor.contract.clone())
                != semantic_contract(source.contract.clone())
            {
                return Err("The game source contract changed before acquisition.".into());
            }
            let key = native_key(&source.endpoint.node_id, &source.endpoint.port_id);
            if game.proof.game_id == "dcs" {
                let (channel, index) = native
                    .strip_prefix("channel:")
                    .and_then(|value| value.rsplit_once(':'))
                    .ok_or("Invalid captured DCS channel.")?;
                let index = index
                    .parse::<usize>()
                    .map_err(|_| "Invalid captured DCS output index.")?;
                rows.push(json!({"id":key,"channel":channel,"output":index}));
            } else {
                let channel = native
                    .strip_prefix("channel:")
                    .ok_or("Invalid captured telemetry channel.")?;
                rows.push(json!({"id":key,"channel":channel}));
            }
            result
                .sources
                .entry(source.endpoint.node_id.clone())
                .or_insert_with(|| SourceState {
                    generation: 1,
                    ..Default::default()
                })
                .ports
                .push(SourcePort {
                    logical: source.endpoint.port_id.clone(),
                    native: key,
                    contract: source.contract.clone(),
                });
            descriptors.insert(native.clone(), descriptor.clone());
        }
        for sink in plan.sinks.iter().filter(|sink| {
            sink.adapter == SinkAdapter::SimulatorCommand
                && game_route(sink.route.as_ref(), &game.proof.game_id)
        }) {
            if game.proof.game_id != "dcs" {
                return Err("This provider does not advertise native simulator commands.".into());
            }
            let native = sink
                .observed_port_id
                .as_ref()
                .ok_or("Game command has no exact native port.")?;
            let descriptor = catalogue
                .get(native)
                .ok_or("The game command disappeared from its captured catalogue.")?;
            if semantic_contract(descriptor.contract.clone())
                != semantic_contract(sink.contract.clone())
            {
                return Err("The game command contract changed before acquisition.".into());
            }
            let PortContract::Action { endpoint, .. } = &descriptor.contract else {
                return Err("The game command is not an action port.".into());
            };
            result.commands.insert(
                (sink.endpoint.node_id.clone(), sink.endpoint.port_id.clone()),
                CommandPort {
                    descriptor: descriptor.clone(),
                    command: endpoint.clone(),
                },
            );
            descriptors.insert(native.clone(), descriptor.clone());
        }
        result.descriptors = descriptors.into_values().collect();
        if result.sources.is_empty() && result.commands.is_empty() {
            return Ok(result);
        }
        result.reader = Some(match game.proof.game_id.as_str() {
            "dcs" => {
                let module = game
                    .aircraft_module
                    .as_deref()
                    .ok_or("Select the DCS aircraft catalogue in game details.")?;
                let path = Path::new(
                    game.connection["cataloguePath"]
                        .as_str()
                        .ok_or("DCS catalogue path is missing.")?,
                );
                let (sources, catalogue_guard, catalogue) =
                    crate::dcs_profile::guarded_telemetry_sources(
                        path,
                        module,
                        &rows,
                        game.catalogue["sourceRevision"]
                            .as_str()
                            .ok_or("The DCS source catalogue has no captured native revision.")?,
                    )?;
                let network = crate::dcs_bios::Network::parse(&game.connection)?;
                let decoder = crate::dcs_profile::ExportSession::from_sources(
                    catalogue,
                    network.freshness_ms,
                    sources,
                )?;
                Reader::Dcs {
                    network,
                    transport: None,
                    decoder,
                    catalogue_guard,
                }
            }
            "x-plane-11" | "x-plane-12" => {
                let mut connection = game.connection.clone();
                connection["processIdentity"] = game.proof.process.value();
                Reader::Xplane {
                    connection,
                    outputs: rows,
                    source: None,
                }
            }
            "assetto-corsa-rally" => {
                let freshness = game.connection["freshnessMs"]
                    .as_u64()
                    .ok_or("Missing telemetry freshness policy.")?;
                Reader::Acr {
                    outputs: rows,
                    source: None,
                    decoder: crate::game_telemetry::Session::new(freshness)?,
                }
            }
            _ => return Err("Unsupported native game provider.".into()),
        });
        Ok(result)
    }
    pub(crate) fn acquire(&mut self, games: &GamesRuntime) -> Result<(), String> {
        self.acquire_with_acr(games, |pid,image|crate::acr_windows::WindowsSource::new_for_image(pid,image)
            .map(|source|Box::new(source) as Box<dyn crate::game_telemetry::Source>))
    }
    fn acquire_with_acr(&mut self, games:&GamesRuntime,
        mut open_acr:impl FnMut(u32,&str)->Result<Box<dyn crate::game_telemetry::Source>,String>,
    )->Result<(),String>{
        self.validate_catalogue(true)?;
        let Some(game) = &self.game else {
            return Ok(());
        };
        games.validate_game_preparation(&game.proof)?;
        if self.acquired {
            return Err("This native game reader was already acquired; prepare a fresh reader instead of replaying acquisition.".into());
        }
        self.acquired = true;
        if self.reader.is_none() && (!self.sources.is_empty() || !self.commands.is_empty()) {
            return Err(
                "The source was stopped; prepare a new native game session before reacquiring."
                    .into(),
            );
        }
        match self.reader.as_mut() {
            Some(Reader::Dcs {
                network, transport, ..
            }) => *transport = Some(crate::dcs_bios::Transport::open_network(network)?),
            Some(Reader::Xplane {
                connection,
                outputs,
                source,
            }) => *source = Some(crate::xplane::Source::new(connection, outputs.clone())?),
            Some(Reader::Acr { source, .. }) => {
                let path=&game.proof.process.path;
                let image=path.rsplit(['\\','/']).next().unwrap_or(path);
                *source=Some(open_acr(game.proof.process.pid,image)?);
            }
            None => {}
        }
        self.lost = false;
        Ok(())
    }
    pub(crate) fn admit(&mut self, games: &GamesRuntime) -> Result<(), String> {
        self.validate_catalogue(true)?;
        let Some(game) = &self.game else {
            return Ok(());
        };
        let context = games.admit_prepared_game_context(&game.proof)?;
        if self.context.as_ref().is_some_and(|old| old != &context) {
            return Err(
                "This game reader belongs to an earlier applied owner; prepare a new session."
                    .into(),
            );
        }
        self.context = Some(context);
        Ok(())
    }
    fn validate_catalogue(&mut self, force: bool) -> Result<(), String> {
        if let Some(Reader::Dcs {
            catalogue_guard, ..
        }) = &mut self.reader
        {
            catalogue_guard.validate(force)?;
        }
        Ok(())
    }
    fn validate(&mut self, games: &GamesRuntime) -> Result<(), String> {
        self.validate_catalogue(false)?;
        let Some(game) = &self.game else {
            return Ok(());
        };
        if let Some(context) = &self.context {
            games.validate_admitted_game_context(context, &game.proof)
        } else {
            games.validate_game_preparation(&game.proof)
        }
    }
    pub(crate) fn poll(&mut self, games: &GamesRuntime, now_us: u64) -> GamePoll {
        self.poll_with_policy(games, now_us, false)
    }
    fn poll_with_policy(
        &mut self,
        games: &GamesRuntime,
        now_us: u64,
        baseline: bool,
    ) -> GamePoll {
        if self.lost {
            return GamePoll {
                context_lost: true,
                ..Default::default()
            };
        }
        if let Err(error) = self.validate(games) {
            return self.lose(now_us, error);
        }
        let snapshot = self.read(now_us, baseline);
        match snapshot {
            Ok(Some(snapshot)) => self.frames(snapshot, now_us),
            Ok(None) => GamePoll::default(),
            Err(error) => self.lose(now_us, error),
        }
    }
    /// Drain native acquisition into one current baseline immediately before
    /// arming. Pre-Start transitions never acquire event/one-shot authority.
    pub(crate) fn baseline(&mut self, games: &GamesRuntime, now_us: u64) -> GamePoll {
        if let Err(error) = self.validate_catalogue(true) {
            return self.lose(now_us, error);
        }
        for source in self.sources.values_mut() {
            source.available = false;
        }
        self.poll_baseline(games, now_us)
    }
    /// One bounded pre-arm read. The host owns waiting, cancellation and output
    /// heartbeats, including after discarding a queued pre-activation backlog.
    pub(crate) fn poll_baseline(&mut self, games: &GamesRuntime, now_us: u64) -> GamePoll {
        self.poll_with_policy(games, now_us, true)
    }
    pub(crate) fn requires_baseline(&self) -> bool {
        self.reader.is_some()
    }
    pub(crate) fn fail(&mut self, now_us: u64, error: String) -> GamePoll {
        self.lose(now_us, error)
    }
    fn read(&mut self, now_us: u64, baseline: bool) -> Result<Option<ExportSnapshot>, String> {
        match self.reader.as_mut() {
            Some(Reader::Dcs {
                network,
                transport,
                decoder,
                ..
            }) => {
                let Some(receiver) = transport.as_ref() else {
                    return Ok(None);
                };
                let mut drained = false;
                for _ in 0..64 {
                    let Some(bytes) = receiver.receive()? else {
                        drained = true;
                        break;
                    };
                    decoder.feed(&bytes, now_us / 1000);
                }
                if !drained && receiver.receive()?.is_some() {
                    decoder.reset_stream(now_us / 1000);
                    if baseline {
                        // Activation can spend longer than the receive bound in
                        // other phases. Drop queued pre-activation datagrams and
                        // require new aircraft identity and advancing counters.
                        // Live polling still disarms on the same overflow.
                        drop(transport.take());
                        *transport = Some(crate::dcs_bios::Transport::open_network(network)?);
                        self.witness = None;
                        self.witnessed_progress = false;
                        self.fresh = false;
                        return Ok(None);
                    }
                    return Err("The native simulator receive queue exceeded its bound; the branch was disarmed instead of replaying a backlog.".into());
                }
                decoder.tick(now_us / 1000);
                Ok(Some(decoder.snapshot(now_us / 1000)))
            }
            Some(Reader::Xplane {
                source: Some(source),
                ..
            }) => Ok(Some(source.poll(Instant::now()).0)),
            Some(Reader::Acr {
                source: Some(source),
                decoder,
                outputs,
            }) => {
                let snapshot = decoder.poll(source.as_mut(), now_us / 1000);
                let channels = snapshot.values();
                let values = outputs
                    .iter()
                    .filter_map(|output| {
                        channels
                            .get(output["channel"].as_str()?)
                            .map(|value| (output["id"].as_str().unwrap().to_owned(), value.clone()))
                    })
                    .collect();
                Ok(Some(ExportSnapshot {
                    epoch: snapshot.epoch,
                    aircraft: snapshot
                        .context
                        .as_ref()
                        .map(|context| context.car_model.clone()),
                    mission_active: snapshot.fresh,
                    fresh: snapshot.fresh,
                    age_ms: snapshot.age_ms,
                    counter: snapshot.packet_id.map(|id| (id & 65535) as u16),
                    values,
                    malformed_frames: 0,
                }))
            }
            _ => Ok(None),
        }
    }
    fn frames(&mut self, mut snapshot: ExportSnapshot, now_us: u64) -> GamePoll {
        if snapshot.fresh {
            if let Some(counter) = snapshot.counter {
                match self.witness {
                    Some((epoch, previous)) if epoch == snapshot.epoch => {
                        if counter != previous {
                            self.witnessed_progress = true;
                        }
                    }
                    _ => {
                        self.witnessed_progress = false;
                    }
                }
                self.witness = Some((snapshot.epoch, counter));
                snapshot.fresh = self.witnessed_progress;
            } else {
                snapshot.fresh = false;
            }
        } else {
            self.witness = None;
            self.witnessed_progress = false;
        }
        self.fresh = snapshot.fresh;
        self.epoch = snapshot.epoch.max(1);
        let mut result = GamePoll {
            fresh: self.fresh,
            ..Default::default()
        };
        for (resource, state) in &mut self.sources {
            let baseline = snapshot.fresh && (!state.available || state.epoch != snapshot.epoch);
            let update = baseline
                || state.available != snapshot.fresh
                || snapshot.fresh && state.counter != snapshot.counter;
            if !update {
                continue;
            }
            if baseline {
                state.generation = state.generation.saturating_add(1);
                state.epoch = snapshot.epoch;
            }
            state.sequence = state.sequence.saturating_add(1);
            state.available = snapshot.fresh;
            state.counter = snapshot.counter;
            let changes=state.ports.iter().map(|port|Change {port_id:port.logical.clone(),sample:if !snapshot.fresh {SignalSample::Stale} else {snapshot.values.get(&port.native).and_then(scalar).map(SignalSample::scalar).filter(|sample|matches!(sample,SignalSample::Known(value)if osg_mapping::engine::typed::accepts(&port.contract,value))).unwrap_or(SignalSample::Unknown)}}).collect();
            result.frames.push(Frame {
                source: resource.clone(),
                generation: Counter(state.generation),
                sequence: Counter(state.sequence),
                captured_us: Counter(now_us),
                reason: if baseline {
                    Reason::Baseline
                } else {
                    Reason::Change
                },
                changes,
            });
        }
        result
    }
    fn lose(&mut self, now_us: u64, error: String) -> GamePoll {
        self.stop();
        self.lost = true;
        let mut result = GamePoll {
            context_lost: true,
            diagnostics: vec![error],
            ..Default::default()
        };
        for (resource, state) in &mut self.sources {
            state.sequence = state.sequence.saturating_add(1);
            state.available = false;
            result.frames.push(Frame {
                source: resource.clone(),
                generation: Counter(state.generation),
                sequence: Counter(state.sequence),
                captured_us: Counter(now_us),
                reason: Reason::Disconnect,
                changes: vec![],
            });
        }
        result
    }
    pub(crate) fn delivery_batch(&mut self) -> Option<ProviderBatch> {
        let game = self.game.as_ref()?;
        if !self.acquired
            || self.lost
            || self.reader.is_none()
            || (!self.fresh && self.revision == 0)
        {
            return None;
        }
        let id = &game.proof.game_id;
        let writable = id == "dcs" && self.fresh && self.context.is_some();
        let publication = (self.epoch, self.fresh, writable);
        if self.published == Some(publication) {
            return None;
        }
        let revision = self.revision.checked_add(1)?;
        let attributes = BTreeMap::from([
            ("gameId".into(), id.clone()),
            (
                "providerSetupRevision".into(),
                game.proof.provider_setup_revision.clone(),
            ),
            (
                "catalogueRevision".into(),
                game.proof.catalogue_revision.clone(),
            ),
            ("gamePreparationSession".into(), game.proof.session.clone()),
            ("gameReaderSession".into(), self.reader_session.clone()),
        ]);
        let batch = ProviderBatch {
            scope: format!("profile-game:{id}"),
            session: self.reader_session.clone(),
            revision,
            complete: true,
            observations: vec![Observation {
                key: id.clone(),
                name: id.clone(),
                resource_type: ResourceType::GameIntegration,
                evidence: vec![IdentityEvidence {
                    kind: EvidenceKind::GameScope,
                    value: id.clone(),
                }],
                attributes: attributes.clone(),
                diagnostics: vec![],
                route: Some(Route {
                    id: format!("profile-game:{id}"),
                    transport: Transport::Integration,
                    provider_generation: format!("{}:{}", self.reader_session, self.epoch),
                    attributes,
                    baseline_ready: self.fresh,
                    writable,
                    priority: 100,
                }),
                ports: Some(PortUpdate::complete(self.descriptors.clone()).ok()?),
            }],
        };
        self.revision = revision;
        self.published = Some(publication);
        Some(batch)
    }
    pub(crate) fn provider_identity(&self) -> Option<(&str, &str)> {
        self.game
            .as_ref()
            .map(|game| (game.proof.game_id.as_str(), self.reader_session.as_str()))
    }
    pub(crate) fn invoke(&mut self, games: &GamesRuntime, effect: &Effect) -> Result<(), String> {
        if self.command_revoked(effect) {
            return Err("Native simulator command publication authority was withdrawn; prepare a new reader to command again.".into());
        }
        self.validate(games)?;
        let (command, argument) = self.command_request(effect)?;
        let Some(Reader::Dcs {
            transport: Some(transport),
            ..
        }) = &self.reader
        else {
            return Err("The native simulator command transport is unavailable.".into());
        };
        transport.send_control(&command, &argument)
    }
    fn command_request(&mut self, effect: &Effect) -> Result<(String, String), String> {
        if self.command_revoked(effect) {
            return Err("Native simulator command publication authority was withdrawn; prepare a new reader to command again.".into());
        }
        self.validate_catalogue(true)?;
        let context = self
            .context
            .as_ref()
            .ok_or("Simulator commands require their applied profile context.")?;
        if self.lost
            || !self.fresh
            || effect.run.session != context.owner.session
            || effect.run.generation.0 != context.owner.run_generation
            || effect.sequence.0 <= self.last_command_sequence
        {
            return Err(
                "Simulator command owner or live baseline expired; the command was not replayed."
                    .into(),
            );
        }
        let command = self
            .commands
            .get(&(
                effect.endpoint.node_id.clone(),
                effect.endpoint.port_id.clone(),
            ))
            .ok_or("The effect is not an assigned native simulator command.")?;
        let SignalSample::Known(SignalValue::ActionPack(pack)) = &effect.sample else {
            return Err("Simulator commands require a complete known action tuple.".into());
        };
        let PortContract::Action {
            parameters,
            semantics,
            ..
        } = &command.descriptor.contract
        else {
            unreachable!()
        };
        if *semantics == osg_mapping::signal::ActionSemantics::OneShot
            && matches!(
                effect.reason,
                ReportReason::Baseline
                    | ReportReason::Start
                    | ReportReason::SourceLost
                    | ReportReason::Stop
            )
        {
            return Err(
                "A baseline or fallback cannot replay a one-shot simulator command.".into(),
            );
        }
        if pack.arguments.len() != parameters.len()
            || pack
                .arguments
                .iter()
                .zip(parameters)
                .any(|(argument, parameter)| argument.id != parameter.id)
        {
            return Err("Simulator action arguments changed identity or order.".into());
        }
        let arguments = pack
            .arguments
            .iter()
            .map(|argument| argument.value.clone())
            .collect::<Vec<_>>();
        command.descriptor.validate_arguments(&arguments)?;
        let argument = match arguments.as_slice() {
            [PortValue::Number(value)] if value.fract() == 0.0 => format!("{value:.0}"),
            [PortValue::Text(value)] => value.clone(),
            _ => {
                return Err(
                    "The native DCS command requires its one exact advertised argument.".into(),
                )
            }
        };
        if let Some(Reader::Dcs { decoder, .. }) = &mut self.reader {
            decoder.tick(effect.at_us.0 / 1000);
            if !decoder.snapshot(effect.at_us.0 / 1000).fresh {
                return Err(
                    "The simulator export baseline expired before command dispatch.".into(),
                );
            }
        }
        self.last_command_sequence = effect.sequence.0;
        Ok((command.command.clone(), argument))
    }

    fn command_revoked(&self, effect: &Effect) -> bool {
        self.revoked_commands.iter().any(|(resource, port)| {
            resource == &effect.endpoint.node_id && port == &effect.endpoint.port_id
        })
    }
    /// Publication loss withdraws only the affected logical command endpoints.
    /// Telemetry and unrelated commands retain their reader and freshness; a
    /// withdrawn endpoint cannot rearm within this reader instance.
    pub(crate) fn revoke_commands(&mut self, endpoints: &[osg_profiles::Endpoint]) {
        for key in self.commands.keys() {
            if endpoints
                .iter()
                .any(|endpoint| endpoint.node_id == key.0 && endpoint.port_id == key.1)
            {
                self.revoked_commands.insert(key.clone());
            }
        }
    }
    pub(crate) fn stop(&mut self) {
        if let Some(Reader::Acr {
            source: Some(source),
            ..
        }) = &mut self.reader
        {
            source.close();
        }
        self.reader = None;
        self.context = None;
        self.fresh = false;
        self.lost = true;
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

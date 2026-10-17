//! Concrete native profile adapters. Volatile readers/workers are keyed by the
//! coordinator owner; durable manifests contain only compare/restore receipts.
#[path = "profile_session_companion_host.rs"]
mod companion;
#[path = "profile_session_visibility_host.rs"]
mod visibility;
#[path = "profile_session_launch_host.rs"]
mod launch;
#[path = "profile_session_action_host.rs"]
mod action;

use crate::profile_session::{
    Admission, AdmissionBlocker, ApplyPhase, BlockerKind, Candidate, Health, Host, OwnedRun,
    Prepared, Receipt, RunOwner,
};
use osg_mapping::{
    engine::{typed::Phase, Counter},
    runtime::RecordingSink,
};
use osg_profiles::{
    compiler_models::{CompilationReport, ExecutionPlan, PlanReadiness, SinkAdapter},
    resources::{ResourceToken, Transport},
};
use serde_json::json;
use std::{collections::BTreeMap, sync::{Arc, atomic::{AtomicU64, Ordering}}, time::Instant};

struct NativePreparation {
    prepared: Prepared,
    report: CompilationReport,
    backend: osg_profiles::compiler_models::BackendCapabilities,
    bindings: Vec<osg_virtual_controller::vjoy::Binding>,
    scripts: crate::profile_session_scripts::Scripts,
    game: crate::profile_session_games::GameSources,
    game_preparation: Option<crate::games::PreparedGame>,
    companions: crate::profile_session_companions::CompanionSources,
    companion_catalogues: Vec<crate::profile_resources::CompanionCatalogue>,
    visibility: crate::profile_session_visibility::StagedVisibility,
}
struct NativeRun {
    owner: RunOwner,
    admission_epoch: u64,
    report: CompilationReport,
    assignments: Vec<osg_profiles::store::LocalAssignment>,
    document: osg_profiles::ProfileDocument,
    backend: osg_profiles::compiler_models::BackendCapabilities,
    delivery: Option<ExecutionPlan>,
    inputs: Option<crate::profile_session_inputs::HidInputs>,
    panels: crate::profile_session_panels_inputs::PanelInputs,
    actions: crate::profile_session_actions::PanelActions,
    baselines: Vec<osg_mapping::engine::typed::Frame>,
    reducer: Option<Phase>,
    recording: RecordingSink,
    started: Instant,
    clock_origin: u64,
    bindings: Vec<osg_virtual_controller::vjoy::Binding>,
    output: Option<osg_virtual_controller::session::Session>,
    scripts: crate::profile_session_scripts::Scripts,
    game: crate::profile_session_games::GameSources,
    game_preparation: Option<crate::games::PreparedGame>,
    child_game_owner: Option<crate::game_context::ProfileOwner>,
    launch_epoch_bound: bool,
    launch_context_installed: bool,
    companions: crate::profile_session_companions::CompanionSources,
    companion_catalogues: Vec<crate::profile_resources::CompanionCatalogue>,
    companion_retry: Option<u64>,
    visibility: crate::profile_session_visibility::Visibility,
}
pub(crate) struct NativeProfileHost {
    resources: Arc<crate::profile_resources::Runtime>,
    controllers: Arc<osg_controller_input::ControllerService>,
    games: Arc<crate::games::GamesRuntime>,
    companions: Arc<crate::companion_host::Runtime>,
    panels: Arc<crate::gateways::ProfilePanelIo>,
    native: bool,
    admission_epoch: Arc<AtomicU64>,
    authority: Option<RunOwner>,
    prepared: BTreeMap<String, NativeRun>,
    recovery_visibility: BTreeMap<String, crate::profile_session_visibility::Visibility>,
    recovery_actions: BTreeMap<String, crate::profile_session_actions::PanelActions>,
}
fn key(owner: &RunOwner) -> String {
    format!("{}:{}", owner.session, owner.generation)
}
impl NativeProfileHost {
    pub(crate) fn new(
        resources: Arc<crate::profile_resources::Runtime>,
        controllers: Arc<osg_controller_input::ControllerService>,
        games: Arc<crate::games::GamesRuntime>,
        companions: Arc<crate::companion_host::Runtime>,
        panels: Arc<crate::gateways::ProfilePanelIo>,
        native: bool,
        admission_epoch: Arc<AtomicU64>,
    ) -> Self {
        Self {
            resources,
            controllers,
            games,
            companions,
            panels,
            native,
            admission_epoch,
            authority: None,
            prepared: BTreeMap::new(),
            recovery_visibility: BTreeMap::new(),
            recovery_actions: BTreeMap::new(),
        }
    }
    fn run(&mut self, owner: &RunOwner) -> Result<&mut NativeRun, String> {
        self.prepared
            .get_mut(&key(owner))
            .filter(|run| run.owner == *owner)
            .ok_or_else(|| "The native prepared run expired; capture a fresh profile plan.".into())
    }
    fn panel_baseline(&mut self, owner: &RunOwner, delivery: &ExecutionPlan) -> Result<Vec<osg_mapping::engine::typed::Frame>, String> {
        let resources = self.resources.clone();
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        loop {
            self.check_transition(owner)?;
            let state = self.run(owner)?;
            if let Some(frames) = state.panels.baseline(delivery, |token| Self::validate_token(&resources, token))? {
                return Ok(frames);
            }
            if Instant::now() >= deadline {
                return Err("The selected native panels did not supply fresh baselines before activation.".into());
            }
            if let Some(output) = state.output.as_mut() { output.keep_alive()?; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn check_epoch(epoch: &AtomicU64, expected: u64) -> Result<(), String> {
        if epoch.load(Ordering::Acquire) != expected {
            return Err("Profile takeover was cancelled by a newer native control request.".into());
        }
        Ok(())
    }
    fn check_transition(&self, owner: &RunOwner) -> Result<(), String> {
        let state = self.prepared.get(&key(owner)).filter(|state| state.owner == *owner)
            .ok_or("The native profile transition expired.")?;
        Self::check_epoch(&self.admission_epoch, state.admission_epoch)
    }
    fn validate_token(
        resources: &crate::profile_resources::Runtime,
        token: &ResourceToken,
    ) -> Result<(), String> {
        resources.validate_delivery(token).map(|_| ())
    }
    fn fence(&self, owner: &RunOwner) -> Result<(), String> {
        if self.authority.as_ref() != Some(owner) {
            Err("The native profile delivery owner was revoked.".into())
        } else {
            Ok(())
        }
    }
    fn blockers(plan: &ExecutionPlan) -> Vec<AdmissionBlocker> {
        let mut blockers = vec![];
        // Every enabled adapter must have its concrete native implementation.
        // Capability gaps are explicit; they never fall back to browser values.
        for source in &plan.sources {
            if source.readiness == PlanReadiness::Ready
                && source.route.as_ref().is_some_and(|route| {
                    !matches!(route.transport, Transport::Hid | Transport::Integration)
                        && !(route.transport == Transport::Usb && route.id.starts_with("usb:"))
                        && !(route.transport == Transport::Can && route.id.starts_with("can:"))
                        && !(matches!(route.transport, Transport::Usb | Transport::Network) && route.id.starts_with("companion:"))
                })
            {
                blockers.push(AdmissionBlocker {
                    kind: BlockerKind::Backend,
                    message: format!(
                        "Native source acquisition is not yet installed for {}.",
                        source.endpoint.node_id
                    ),
                });
            }
        }
        for sink in &plan.sinks {
            if sink.readiness == PlanReadiness::Ready
                && !matches!(
                    sink.adapter,
                    SinkAdapter::VirtualController | SinkAdapter::SimulatorCommand | SinkAdapter::CompanionFeedback | SinkAdapter::PhysicalAction
                )
            {
                blockers.push(AdmissionBlocker {
                    kind: BlockerKind::Backend,
                    message: format!(
                        "Native output acquisition is not yet installed for {}.",
                        sink.endpoint.node_id
                    ),
                });
            }
        }
        if let Err(message)=crate::profile_session_overrides::validate(plan) {
            blockers.push(AdmissionBlocker {kind:BlockerKind::Backend,message});
        }
        if plan.program.nodes.iter().any(|node| {
            matches!(
                node.operation,
                osg_mapping::program::Operation::Script { .. }
            )
        }) && !osg_mapping_host::service::SCRIPT_LIVE_QUALIFIED
        {
            blockers.push(AdmissionBlocker {
                kind: BlockerKind::Script,
                message:
                    "Live script output remains behind its existing native qualification gate."
                        .into(),
            });
        }
        blockers
    }
    fn game_capture(
        snapshot: &mut osg_profiles::compiler_models::CompileSnapshot,
        game: Option<&crate::games::PreparedGame>,
    ) {
        snapshot.game_context =
            game.map(|game| osg_profiles::compiler_models::VerifiedGameContext {
                game_id: game.proof.game_id.clone(),
                session: game.proof.session.clone(),
                provider_setup_revisions: snapshot
                    .resources
                    .iter()
                    .filter(|resource| {
                        resource.game_id.as_deref() == Some(&game.proof.game_id)
                            && resource.provider_setup_revision.as_deref()
                                == Some(&game.proof.provider_setup_revision)
                            && resource.summary.attributes.get("catalogueRevision")
                                == Some(&game.proof.catalogue_revision)
                    })
                    .map(|resource| {
                        (
                            resource.summary.id.clone(),
                            game.proof.provider_setup_revision.clone(),
                        )
                    })
                    .collect(),
            });
    }
    fn withdraw_game(resources: &crate::profile_resources::Runtime, state: &NativeRun) -> Result<(), String> {
        if let Some((game, reader)) = state.game.provider_identity() {
            resources.withdraw_profile_game_delivery(game, reader)?;
        }
        Ok(())
    }
    fn game_poll(
        resources: &crate::profile_resources::Runtime,
        games: &crate::games::GamesRuntime,
        state: &mut NativeRun,
        now_us: u64,
    ) -> Result<crate::profile_session_games::GamePoll, String> {
        let mut sample = state.game.poll(games, now_us);
        if let Some(batch) = state.game.delivery_batch() {
            if let Err(error) = resources.publish_profile_game_delivery(batch) {
                sample = state.game.fail(now_us, error);
            }
        }
        if !sample.context_lost && state.game.requires_baseline() {
            let validation = state.game.provider_identity()
                .ok_or_else(|| "The acquired game reader has no native inventory owner.".to_owned())
                .and_then(|(game, reader)| resources.validate_profile_game_delivery(game, reader));
            if let Err(error) = validation { sample = state.game.fail(now_us, error); }
        }
        if sample.context_lost {
            Self::withdraw_game(resources, state)?;
            if sample.diagnostics.is_empty() {
                sample.diagnostics.push("The game branch is disarmed; prepare a fresh verified game session to resume it.".into());
            }
        } else if state.game.requires_baseline() && !sample.fresh {
            sample.diagnostics.push("Game telemetry is stale; its branches await a fresh native baseline.".into());
        }
        Ok(sample)
    }
    fn dispatch_game(
        resources: &crate::profile_resources::Runtime,
        games: &crate::games::GamesRuntime,
        state: &mut NativeRun,
        effects: Vec<osg_mapping::engine::typed::Effect>,
    ) -> Result<Vec<String>, String> {
        let mut diagnostics = vec![];
        for effect in effects {
            let sink = state.delivery.as_ref().and_then(|plan| plan.sinks.iter().find(|sink|
                sink.endpoint.node_id == effect.endpoint.node_id && sink.endpoint.port_id == effect.endpoint.port_id)).cloned();
            let Some(sink) = sink.filter(|sink| sink.adapter == SinkAdapter::SimulatorCommand) else {
                // Virtual-controller effects are submitted by the existing
                // recording/isolated-feeder adapter. Other adapters are blocked
                // by native admission until their concrete owners are installed.
                continue;
            };
            if !matches!(effect.sample, osg_mapping::program::SignalSample::Known(_)) {
                continue;
            }
            let result = (|| {
                let stale = sink.readiness != PlanReadiness::Ready || sink.token.as_ref()
                    .is_none_or(|token| resources.validate_delivery(token).is_err());
                if stale {
                    // A fresh reader baseline may replace only cached delivery
                    // tokens, never the immutable graph or its local assignment.
                    let mut snapshot = resources.snapshot_for_compile(state.assignments.clone())?;
                    snapshot.backend = state.backend.clone();
                    Self::game_capture(&mut snapshot, state.game_preparation.as_ref());
                    let report = osg_profiles::compiler::compile(&state.document, &snapshot);
                    if report.program_hash != state.report.program_hash {
                        return Err("The native game delivery program identity changed.".into());
                    }
                    state.delivery = report.plan;
                }
                let sink = state.delivery.as_ref().and_then(|plan| plan.sinks.iter().find(|sink|
                    sink.endpoint.node_id == effect.endpoint.node_id && sink.endpoint.port_id == effect.endpoint.port_id))
                    .filter(|sink| sink.adapter == SinkAdapter::SimulatorCommand && sink.readiness == PlanReadiness::Ready)
                    .ok_or("The simulator action is dormant or no longer ready.")?;
                let token = sink.token.as_ref().filter(|token| token.access == osg_profiles::resources::Access::Invoke)
                    .ok_or("The simulator action has no current native invocation token.")?;
                resources.validate_delivery(token)?;
                state.game.invoke(games, &effect)
            })();
            if let Err(error) = result {
                let loss = state.game.fail(effect.at_us.0, error);
                diagnostics.extend(loss.diagnostics);
                Self::withdraw_game(resources, state)?;
                if let Some(reducer) = state.reducer.as_mut() {
                    for frame in loss.frames { reducer.accept(frame)?; }
                    state.recording.record(reducer.drain_reports());
                    // Disconnection effects cannot become deferred simulator
                    // commands. Ordinary controller reports still reach feeder.
                    reducer.drain();
                }
                break;
            }
        }
        Ok(diagnostics)
    }
    pub(crate) fn stage(
        resources: &crate::profile_resources::Runtime,
        games: &crate::games::GamesRuntime,
        companions: &crate::companion_host::Runtime,
        controllers: Arc<osg_controller_input::ControllerService>,
        native: bool,
        candidate: &Candidate,
    ) -> Result<Arc<crate::profile_session::Stage>, String> {
        let game_preparation = match candidate.game_id.as_deref() {
            Some(id) => games.prepare_profile_game_context(id)?,
            None => None,
        };
        if !candidate.assignments.is_empty() {
            resources.refresh_native()?;
        }
        let (initial, _) = resources.snapshot_for_preparation(candidate.assignments.clone())?;
        let (companion_catalogues, companion_bindings) = if candidate.document.companion_assignments.is_empty() {
            (vec![], vec![])
        } else {
            companion::prepare_catalogues(&companions.profile_metadata()?, &initial, candidate)?
        };
        let (mut snapshot, preparations) = resources.snapshot_for_preparation_with_companions(initial.assignments, &companion_catalogues)?;
        Self::game_capture(&mut snapshot, game_preparation.as_ref());
        if candidate.document.visibility.iter().any(|intent|intent.mode==osg_profiles::VisibilityMode::HiddenWhileActive) {
            let status=controllers.request(&json!({"command":"controller_visibility_status"}))?;
            snapshot.backend.visibility_available=native && cfg!(windows) && status["visibility"]["modes"].as_array()
                .is_some_and(|modes|modes.iter().any(|mode|mode["mode"]=="hide-all"&&mode["supported"]==true));
            snapshot.backend.visibility_qualified=native && status["visibility"]["qualified"]==true;
        }

        let slots = if !candidate.document.virtual_outputs.is_empty() && native {
            osg_virtual_controller::inspection::current().ok()
        } else {
            None
        };
        snapshot.backend.name = osg_mapping_host::service::BACKEND.into();
        snapshot.backend.available = slots.is_some();
        snapshot.backend.qualified =
            native && osg_mapping_host::service::LIVE_OUTPUT_QUALIFIED;
        let report = osg_profiles::compiler::compile_for_preparation(
            &candidate.document,
            &snapshot,
            &preparations,
        );
        let plan = report
            .plan
            .clone()
            .ok_or("The profile compiler did not produce an executable plan.")?;
        let mut admission = Admission::default();
        if matches!(
            report.readiness,
            PlanReadiness::Invalid | PlanReadiness::Blocked
        ) {
            admission.blockers.extend(
                report
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| {
                        diagnostic.severity
                            == osg_profiles::compiler_models::DiagnosticSeverity::Error
                    })
                    .map(|diagnostic| AdmissionBlocker {
                        kind: if report.readiness == PlanReadiness::Invalid {
                            BlockerKind::Structural
                        } else {
                            BlockerKind::RequiredResource
                        },
                        message: diagnostic.message.clone(),
                    }),
            );
            if admission.blockers.is_empty() {
                admission.blockers.push(AdmissionBlocker {
                    kind: BlockerKind::RequiredResource,
                    message: "The profile requires a current compatible native resource.".into(),
                });
            }
        }
        admission.blockers.extend(Self::blockers(&plan));
        admission.waiting_baselines = plan
            .sources
            .iter()
            .filter(|source| {
                source
                    .preparation
                    .as_ref()
                    .is_some_and(|pin| pin.requires_baseline())
            })
            .map(|source| format!("{}:{}", source.endpoint.node_id, source.endpoint.port_id))
            .collect();
        let bindings = if !plan.program.controllers.is_empty() && slots.is_some() {
            match crate::profile_session_virtual::bindings(
                &plan.program.controllers,
                slots.as_deref().unwrap(),
            ) {
                Ok(bindings) => bindings,
                Err(message) => {
                    admission.blockers.push(AdmissionBlocker {
                        kind: BlockerKind::Layout,
                        message,
                    });
                    vec![]
                }
            }
        } else {
            vec![]
        };
        admission.dormant_branches = plan
            .branches
            .iter()
            .filter(|branch| branch.readiness == PlanReadiness::Dormant)
            .map(|branch| format!("{}:{}", branch.sink.node_id, branch.sink.port_id))
            .collect();
        admission.optional_unavailable = plan
            .branches
            .iter()
            .filter(|branch| !branch.required && branch.readiness == PlanReadiness::Blocked)
            .map(|branch| {
                format!(
                    "{}:{} is unavailable.",
                    branch.sink.node_id, branch.sink.port_id
                )
            })
            .collect();
        let prepared = Prepared {
            plan,
            admission,
            program_hash: report
                .program_hash
                .clone()
                .ok_or("The compiled profile has no program identity.")?,
            capture_fingerprint: report.capture_fingerprint.clone(),
        };
        let scripts = crate::profile_session_scripts::Scripts::prepare(
            &prepared.plan.program,
            &std::env::current_exe().map_err(|error| error.to_string())?,
        )?;
        let game = crate::profile_session_games::GameSources::prepare(
            &prepared.plan,
            game_preparation.clone(),
        )?;
        let companions = crate::profile_session_companions::CompanionSources::prepare(&prepared.plan, companion_bindings)?;
        let visibility=crate::profile_session_visibility::Visibility::stage(&prepared.plan,&snapshot,controllers)?;
        crate::profile_session::Stage::new(candidate, NativePreparation {
            prepared, report, backend: snapshot.backend, bindings, scripts, game, game_preparation,
            companions, companion_catalogues, visibility,
        })
    }
}
impl Host for NativeProfileHost {
    fn inspect_topology(&self, run: &OwnedRun, request: &crate::profile_session::topology::Request) -> Result<crate::profile_session::topology::Page, String> {
        self.fence(&run.owner)?;
        let state=self.prepared.get(&key(&run.owner)).filter(|state|state.owner==run.owner)
            .ok_or("The applied native profile is no longer available.")?;
        let reducer=state.reducer.as_ref().filter(|reducer|reducer.active())
            .ok_or("The applied native reducer is stopped.")?;
        crate::profile_session::topology::capture(&state.document,reducer,run,request)
    }
    fn inspect(&self, run: &OwnedRun, request: &crate::profile_session::inspection::Request) -> Result<crate::profile_session::inspection::Page, String> {
        self.fence(&run.owner)?;
        let state = self.prepared.get(&key(&run.owner)).filter(|state| state.owner == run.owner)
            .ok_or("The applied native profile is no longer available.")?;
        let reducer = state.reducer.as_ref().filter(|reducer| reducer.active())
            .ok_or("The applied native reducer is stopped.")?;
        crate::profile_session::inspection::capture(&state.document, reducer, run, request)
    }
    fn prepare(&mut self, candidate: &Candidate, owner: &RunOwner) -> Result<Prepared, String> {
        let admission_epoch = candidate.admission_epoch.ok_or("Native profile preparation is missing its captured cancellation fence.")?;
        Self::check_epoch(&self.admission_epoch, admission_epoch)?;
        let NativePreparation {mut prepared, report, backend, bindings, scripts, game, game_preparation, companions, companion_catalogues, visibility} = candidate.stage.as_ref()
            .ok_or("A fresh native profile preparation is required.")?
            .take::<NativePreparation>(candidate)?;
        let panels = crate::profile_session_panels_inputs::PanelInputs::prepare(&prepared.plan, self.panels.clone())?;
        let visibility=visibility.bind(owner)?;
        let actions=crate::profile_session_actions::PanelActions::prepare(&prepared.plan, owner, self.resources.clone(), self.panels.clone())?;
        prepared.admission.optional_unavailable.extend(actions.optional_unavailable().iter().cloned());
        self.prepared.insert(key(owner), NativeRun {
            owner: owner.clone(), admission_epoch, report, assignments: candidate.assignments.clone(),
            document: candidate.document.clone(), backend, delivery: None, inputs: None, panels,
            baselines: vec![], reducer: None, recording: RecordingSink::default(),
            started: Instant::now(), clock_origin: 0, bindings, output: None, scripts, game, game_preparation,
            child_game_owner: None, launch_epoch_bound: false, launch_context_installed: false,
            companions, companion_catalogues, companion_retry: None, visibility,
            actions,
        });
        self.validate_prepared(&prepared, owner)?;
        Ok(prepared)
    }
    fn validate_prepared(&mut self, prepared: &Prepared, owner: &RunOwner) -> Result<(), String> {
        self.check_transition(owner)?;
        let assignments = self.run(owner)?.assignments.clone();
        let proof = self
            .run(owner)?
            .game_preparation
            .as_ref()
            .map(|game| game.proof.clone());
        if let Some(proof) = proof {
            self.games.validate_game_preparation(&proof)?;
        }
        let (current, _) = self.resources.snapshot_for_preparation(assignments)?;
        let companions = self.companions.clone();
        let run = self.run(owner)?;
        run.companions.validate_preparation(&companions)?;
        if current.read_only
            || current.inventory_session.is_empty()
            || run.report.capture_fingerprint != prepared.capture_fingerprint
            || run.report.program_hash.as_deref() != Some(&prepared.program_hash)
        {
            return Err("The native prepared profile identity changed.".into());
        }
        for pin in prepared
            .plan
            .sources
            .iter()
            .filter_map(|source| source.preparation.as_ref())
            .chain(
                prepared
                    .plan
                    .sinks
                    .iter()
                    .filter_map(|sink| sink.preparation.as_ref()),
            )
        {
            self.resources.validate_preparation(pin)?;
        }
        Ok(())
    }
    fn capture(
        &mut self,
        prepared: &Prepared,
        owner: &RunOwner,
        previous: Option<&OwnedRun>,
    ) -> Result<Vec<Receipt>, String> {
        self.validate_prepared(prepared, owner)?;
        let runtime = self.companions.clone();
        let state = self.run(owner)?;
        let previous = previous.map_or(&[][..], |run|run.receipts.as_slice());
        let mut receipts = if !state.companions.is_empty() || companion::has_receipts(previous) {
            companion::capture(&runtime.profile_metadata()?, &state.companions, previous)?
        } else { vec![] };
        receipts.extend(state.visibility.receipts()?);
        receipts.extend(state.actions.capture_receipts());
        if !state.bindings.is_empty() {
            receipts.push(Receipt { kind: "virtual-output".into(), resource_id:"virtual-output".into(),
                state: json!({"journal":crate::profile_session_virtual::journal_name(owner)?}) });
        }
        Ok(receipts)
    }

    fn set_authority(&mut self, owner: Option<&RunOwner>) -> Result<(), String> {
        let context = self.effective_context_owner(owner)?;
        let previous_context = self.effective_context_owner(self.authority.as_ref())?;
        let retained_child = owner.is_none() && self.prepared.values().any(|run|run.child_game_owner.is_some());
        if self.authority.as_ref() == owner && !retained_child {
            return Ok(());
        }
        if previous_context.is_none() && context.is_none() && !retained_child {
            self.authority = owner.cloned();
            return Ok(());
        }
        self.authority = None;
        self.games.set_profile_context_owner(context)?;
        self.authority = owner.cloned();
        Ok(())
    }
    fn neutralize(&mut self, run: &OwnedRun) -> Result<(), String> {
        let resources = self.resources.clone();
        let companions = self.companions.clone();
        let mut adapter_result=Ok(());
        if let Some(state) = self.prepared.get_mut(&key(&run.owner)) {
            if let Some(reducer) = state.reducer.as_mut() {
                reducer.stop();
                state.recording.record(reducer.drain_reports());
                reducer.drain();
            }
            let _=state.actions.stop();
            state.scripts.stop();
            state.game.stop();
            state.panels.stop();
            let withdrawal = Self::withdraw_game(&resources, state);
            let companion_stop = state.companions.stop_native(&companions);
            let mut output_result = Ok(());
            if let Some(output) = state.output.as_mut() {
                let flush = output.poll(&state.recording);
                let stop = output.stop();
                output_result = flush.and(stop);
            }
            adapter_result=withdrawal.and(output_result).and(companion_stop);
        }
        let visibility_result=self.restore_visibility(run);
        let action_result=self.restore_actions(run,false);
        adapter_result.and(visibility_result).and(action_result)
    }
    fn enter(
        &mut self,
        phase: ApplyPhase,
        prepared: &Prepared,
        run: &OwnedRun,
    ) -> Result<(), String> {
        self.check_transition(&run.owner)?;
        match phase {
            ApplyPhase::Overrides => Ok(()),
            ApplyPhase::Acquire => {
                let native = self.native;
                let origin = self.controllers.stream_page("", 0).captured_us;
                let state = self.run(&run.owner)?;
                state.clock_origin = origin;
                state.panels.acquire(origin)?;
                if !state.bindings.is_empty() {
                    if !native || !osg_mapping_host::service::LIVE_OUTPUT_QUALIFIED {
                        return Err(osg_mapping_host::service::QUALIFICATION.into());
                    }
                    let path = crate::dcs_runtime::config_path()
                        .with_file_name(crate::profile_session_virtual::journal_name(&run.owner)?);
                    state.output = Some(
                        osg_virtual_controller::session::Session::start_with_journal(
                            osg_virtual_controller::entry::factory(),
                            osg_mapping::engine::RunToken {
                                session: run.owner.session.clone(),
                                generation: Counter(run.owner.generation),
                            },
                            state.bindings.clone(),
                            Counter(0),
                            Some(path),
                        )?,
                    );
                }
                let games = self.games.clone();
                self.run(&run.owner)?.game.acquire(&games)?;
                let receipts = companion::receipts(&run.receipts)?;
                let runtime = self.companions.clone();
                if receipts.iter().any(|(release,_)|*release) {
                    companion::restore(&runtime.profile_metadata()?, &receipts, true)?;
                }
                let selected = receipts.into_iter().filter_map(|(release,receipt)|(!release).then_some(receipt)).collect::<Vec<_>>();
                self.run(&run.owner)?.companions.acquire(&runtime, &selected)?;
                self.acquire_actions(&run.owner)
            }
            ApplyPhase::Baseline => {
                let games = self.games.clone();
                let resources = self.resources.clone();
                let controllers = self.controllers.clone();
                let deadline = Instant::now() + std::time::Duration::from_secs(3);
                let mut game_frames = vec![];
                while self.run(&run.owner)?.game.requires_baseline() {
                    self.check_transition(&run.owner)?;
                    let now = controllers.stream_page("", 0).captured_us;
                    let state = self.run(&run.owner)?;
                    let sample = state.game.poll(&games, now);
                    if let Some(batch) = state.game.delivery_batch() {
                        resources.publish_profile_game_delivery(batch)?;
                    }
                    if sample.context_lost {
                        return Err(
                            "The verified game context changed during baseline acquisition.".into(),
                        );
                    }
                    if sample.fresh {
                        game_frames = sample.frames;
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err("The game provider did not produce an advancing fresh baseline before activation.".into());
                    }
                    if let Some(output) = state.output.as_mut() {
                        output.keep_alive()?;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                self.check_transition(&run.owner)?;
                let (delivery, companion_frames) = self.companion_baseline(&run.owner, &prepared.plan)?;
                let panel_frames = self.panel_baseline(&run.owner, &delivery)?;
                let (inputs, baselines) = crate::profile_session_inputs::HidInputs::capture(
                    &delivery,
                    self.controllers.as_ref(),
                    |token| Self::validate_token(&self.resources, token),
                )?;
                let state = self.run(&run.owner)?;
                state.inputs = Some(inputs);
                state.baselines = baselines;
                state.baselines.extend(game_frames);
                state.baselines.extend(panel_frames);
                state.baselines.extend(companion_frames);
                state.delivery = Some(delivery);
                self.refresh_action_binding(&run.owner)
            }
            ApplyPhase::Visibility => self.start_visibility(&run.owner),
            ApplyPhase::Start => {
                self.check_transition(&run.owner)?;
                self.fence(&run.owner)?;
                let games = self.games.clone();
                let resources = self.resources.clone();
                let controllers = self.controllers.clone();
                let (assignments, document, backend, game_preparation) = {
                    let state = self.run(&run.owner)?;
                    (
                        state.assignments.clone(),
                        state.document.clone(),
                        state.backend.clone(),
                        state.game_preparation.clone(),
                    )
                };
                {
                    let state = self.run(&run.owner)?;
                    // Acquisition/visibility can outlast telemetry freshness.
                    // Recheck the real reader before exposing any writable route.
                    let baseline = state.game.baseline(&games, controllers.stream_page("", 0).captured_us);
                    if baseline.context_lost || state.game.requires_baseline() && !baseline.fresh {
                        return Err("The game baseline expired before command admission.".into());
                    }
                    state.game.admit(&games)?;
                    if let Some(batch) = state.game.delivery_batch() {
                        resources.publish_profile_game_delivery(batch)?;
                    }
                }
                // Visibility and panel setup may have invalidated the incoming
                // panel session. Reacquire actual baselines before final compile.
                self.companion_baseline(&run.owner, &prepared.plan)?;
                let mut snapshot = resources.snapshot_for_compile(assignments)?;
                snapshot.backend = backend;
                Self::game_capture(&mut snapshot, game_preparation.as_ref());
                let delivery = osg_profiles::compiler::compile(&document, &snapshot);
                if matches!(
                    delivery.readiness,
                    PlanReadiness::Invalid | PlanReadiness::Blocked
                ) || delivery.program_hash.as_deref() != Some(&prepared.program_hash)
                {
                    return Err("The final native delivery plan changed or remained blocked after acquisition.".into());
                }
                let delivery = delivery
                    .plan
                    .ok_or("The compiler did not produce the final native delivery plan.")?;
                let panel_frames = self.panel_baseline(&run.owner, &delivery)?;
                self.bind_actions(&run.owner, &delivery)?;
                let companion_hub = if self.run(&run.owner)?.companions.is_empty() { None } else { Some(self.companions.profile_metadata()?) };
                let (inputs, baselines) = crate::profile_session_inputs::HidInputs::capture(
                    &delivery,
                    controllers.as_ref(),
                    |token| Self::validate_token(&resources, token),
                )?;
                let state = self.run(&run.owner)?;
                let game_baseline = state
                    .game
                    .baseline(&games, controllers.stream_page("", 0).captured_us);
                if game_baseline.context_lost
                    || state.game.requires_baseline() && !game_baseline.fresh
                {
                    return Err("The game baseline expired before arming the profile.".into());
                }
                state.companions.bind_delivery_plan(&delivery)?;
                let companion_frames = match companion_hub {
                    Some(ref hub) => state.companions.baseline(hub, |token|Self::validate_token(&resources,token), controllers.stream_page("",0).captured_us)?
                        .ok_or("The companion baseline expired before arming the profile.")?,
                    None => vec![],
                };
                state.inputs = Some(inputs);
                state.baselines = baselines;
                state.baselines.extend(game_baseline.frames);
                state.baselines.extend(panel_frames);
                state.baselines.extend(companion_frames);
                state.delivery = Some(delivery);
                let mut reducer = Phase::new(
                    prepared.plan.program.clone(),
                    Counter(run.owner.generation),
                    run.owner.session.clone(),
                )?;
                for baseline in state.baselines.drain(..) {
                    state.clock_origin = state.clock_origin.max(baseline.captured_us.0);
                    reducer.accept(baseline)?;
                }
                visibility::active(&mut state.visibility)?;
                state.companions.arm(reducer.run_token())?;
                state.scripts.poll(&mut reducer);
                state.recording.record(reducer.drain_reports());
                let effects = reducer.drain();
                state.reducer = Some(reducer);
                state.panels.start()?;
                action::start(state)?;
                state.started = Instant::now();
                let _physical_diagnostics = action::dispatch(state,&effects);
                let diagnostics = Self::dispatch_game(&resources, &games, state, effects)?;
                if !diagnostics.is_empty() { return Err(diagnostics.join("; ")); }
                if let Some(output) = state.output.as_mut() {
                    output.poll(&state.recording)?;
                }
                Ok(())
            }
        }
    }
    fn restore(&mut self, run: &OwnedRun, recovery: bool) -> Result<(), String> {
        if let Some(state) = self.prepared.get(&key(&run.owner)) {
            Self::withdraw_game(&self.resources, state)?;
        }
        let captured = companion::receipts(&run.receipts)?;
        let visibility_receipt=crate::profile_session_visibility::Visibility::from_receipts(run,self.controllers.clone())?;
        for receipt in &run.receipts {
            if companion::has_receipts(std::slice::from_ref(receipt)) || action::has_receipts(std::slice::from_ref(receipt)) || receipt.kind=="controller-visibility" && visibility_receipt.is_some() { continue; }
            if receipt.kind != "virtual-output" || receipt.resource_id != "virtual-output"
                || receipt.state != json!({"journal":crate::profile_session_virtual::journal_name(&run.owner)?}) {
                return Err("Unknown native recovery receipts require inspection; external state was preserved.".into());
            }
        }
        self.restore_visibility(run)?;
        self.restore_actions(run,recovery)?;
        if run.attempted.contains(&ApplyPhase::Acquire) {
            if run.receipts.iter().any(|r|r.kind=="virtual-output") { crate::profile_session_virtual::recover(&run.owner)?; }
            if !captured.is_empty() { companion::restore(&self.companions.profile_metadata()?, &captured, false)?; }
        }
        self.prepared.remove(&key(&run.owner));
        Ok(())
    }
    fn health(&mut self, run: &OwnedRun) -> Result<Health, String> {
        self.fence(&run.owner)?;
        let controllers = self.controllers.clone();
        let resources = self.resources.clone();
        let games = self.games.clone();
        let companions = self.companions.clone();
        let state = self.run(&run.owner)?;
        visibility::active(&mut state.visibility)?;
        let (frames, mut degraded) = state
            .inputs
            .as_mut()
            .ok_or("The native source reader was not acquired.")?
            .poll(controllers.as_ref(), |token| {
                Self::validate_token(&resources, token)
            })?;
        // Native context loss reaches the reducer before timers/scripts can
        // emit effects, withdrawing only branches that depend on that game.
        let game = Self::game_poll(&resources, &games, state, controllers.stream_page("", 0).captured_us)?;
        degraded.extend(game.diagnostics);
        let panels = state.panels.poll(|token| Self::validate_token(&resources, token))?;
        degraded.extend(panels.diagnostics);
        let companion = Self::companion_poll(&companions,&resources,state,controllers.stream_page("",0).captured_us);
        degraded.extend(companion.diagnostics);
        let reducer = state
            .reducer
            .as_mut()
            .ok_or("The profile reducer is stopped.")?;
        degraded.extend(reducer.accept_batch(frames.into_iter().chain(game.frames).chain(panels.frames).chain(companion.frames))?);
        let elapsed = u64::try_from(state.started.elapsed().as_micros())
            .map_err(|_| "Profile runtime clock exhausted.")?;
        reducer.advance(Counter(
            reducer.at_us().0.max(
                state
                    .clock_origin
                    .checked_add(elapsed)
                    .ok_or("Profile runtime clock exhausted.")?,
            ),
        ))?;
        degraded.extend(state.scripts.poll(reducer));
        state.recording.record(reducer.drain_reports());
        let effects = reducer.drain();
        let stop = reducer.fault().map(str::to_owned);
        if !state.companions.is_empty() {
            match companions.profile_metadata() {
                Ok(hub) => degraded.extend(state.companions.publish_feedback(&hub,reducer,|token|Self::validate_token(&resources,token),reducer.at_us().0)),
                Err(error) => degraded.push(error),
            }
        }
        degraded.extend(action::dispatch(state,&effects));
        degraded.extend(Self::dispatch_game(&resources, &games, state, effects)?);
        if let Some(output) = state.output.as_mut() {
            output.poll(&state.recording)?;
        }
        Ok(Health {
            degraded,
            stop,
        })
    }
    fn supports_launch_hold(&self) -> bool { true }
    fn hold_health(&mut self, run:&OwnedRun)->Result<Health,String> {self.held_health(run)}
    fn bind_launch_epoch(&mut self, run:&OwnedRun, epoch:u64)->Result<(),String> {self.bind_held_epoch(run,epoch)}
    fn install_launch_context(&mut self, run:&OwnedRun, candidate:&Candidate)->Result<Prepared,String> {self.install_held_context(run,candidate)}
    fn discard_preparation(&mut self, owner: &RunOwner) {
        if let Some(mut state) = self.prepared.remove(&key(owner)) {
            let _=state.actions.stop();
            state.scripts.stop();
            state.game.stop();
            state.panels.stop();
            let _ = state.companions.stop_native(&self.companions);
            let _ = Self::withdraw_game(&self.resources, &state);
            if state.visibility.needs_cleanup() { self.recovery_visibility.insert(key(owner),state.visibility); }
            self.discard_actions(owner,state.actions);
        }
    }
}

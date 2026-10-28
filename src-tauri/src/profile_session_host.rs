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
#[path = "profile_session_publication_host.rs"]
mod publication;
#[path = "profile_session_dcs_hid.rs"]
mod dcs_hid;

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

type ProgressCallback = Arc<dyn Fn(u64, &str, &str) + Send + Sync>;

struct NativePreparation {
    document: osg_profiles::ProfileDocument,
    assignments: Vec<osg_profiles::store::LocalAssignment>,
    vehicle_selection: Option<String>,
    prepared: Prepared,
    report: CompilationReport,
    backend: osg_profiles::compiler_models::BackendCapabilities,
    bindings: Vec<osg_virtual_controller::vjoy::Binding>,
    output_handoff: Option<crate::profile_session_virtual::PreparedHandoff>,
    scripts: crate::profile_session_scripts::Scripts,
    game: crate::profile_session_games::GameSources,
    game_preparation: Option<crate::games::PreparedGame>,
    companions: crate::profile_session_companions::CompanionSources,
    companion_catalogues: Vec<crate::profile_resources::CompanionCatalogue>,
    visibility: crate::profile_session_visibility::StagedVisibility,
    publication: crate::profile_session_overrides::Staged,
    dcs_hid: dcs_hid::Guard,
    desktop_scope: Option<osg_desktop_output::Scope>,
    unavailable_peripherals: BTreeMap<String, String>,
}
struct NativeRun {
    owner: RunOwner,
    admission_epoch: u64,
    report: CompilationReport,
    assignments: Vec<osg_profiles::store::LocalAssignment>,
    document: osg_profiles::ProfileDocument,
    source_document: osg_profiles::ProfileDocument,
    source_assignments: Vec<osg_profiles::store::LocalAssignment>,
    vehicle_selection: Option<String>,
    vehicle_paused: bool,
    vehicle_error: Option<String>,
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
    desktop: Option<crate::profile_session_desktop::Desktop>,
    desktop_scope: Option<osg_desktop_output::Scope>,
    output_handoff: Option<crate::profile_session_virtual::PreparedHandoff>,
    scripts: crate::profile_session_scripts::Scripts,
    game: crate::profile_session_games::GameSources,
    game_preparation: Option<crate::games::PreparedGame>,
    child_game_owner: Option<crate::game_context::ProfileOwner>,
    game_retry: Option<u64>,
    game_resume: Option<std::sync::mpsc::Receiver<Result<Option<NativeGameResume>,String>>>,
    launch_epoch_bound: bool,
    launch_context_installed: bool,
    companions: crate::profile_session_companions::CompanionSources,
    companion_catalogues: Vec<crate::profile_resources::CompanionCatalogue>,
    companion_retry: Option<u64>,
    companion_initial_operations: BTreeMap<String,serde_json::Value>,
    visibility: crate::profile_session_visibility::Visibility,
    publication: crate::profile_session_overrides::Overrides,
    dcs_hid: dcs_hid::Guard,
    acquisition_plan: Option<ExecutionPlan>,
    admission_degradation: Vec<String>,
    unavailable_peripherals: BTreeMap<String, String>,
}
struct NativeGameResume {
    game: crate::profile_session_games::GameSources,
    proof: crate::games::PreparedGame,
    plan: ExecutionPlan,
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
    recovery_publications: BTreeMap<String, crate::profile_session_overrides::Overrides>,
    last_desktop_status: serde_json::Value,
    progress: Option<ProgressCallback>,
    cleanup_epoch: Option<u64>,
}
fn key(owner: &RunOwner) -> String {
    format!("{}:{}", owner.session, owner.generation)
}
impl NativeProfileHost {
    fn unavailable_peripherals(snapshot: &osg_profiles::compiler_models::CompileSnapshot, document: &osg_profiles::ProfileDocument) -> BTreeMap<String, String> {
        document.resources.iter().filter(|resource| matches!(resource.kind, osg_profiles::ResourceKind::Device | osg_profiles::ResourceKind::Companion))
            .filter_map(|resource| {
                let assignment = snapshot.assignments.iter().find(|assignment| assignment.resource_id == resource.id)?;
                let absent = snapshot.resources.iter().find(|captured| captured.summary.id == assignment.source_id)
                    .is_none_or(|captured| captured.summary.availability == osg_profiles::resources::Availability::Offline);
                absent.then(|| (resource.id.clone(), assignment.source_id.clone()))
            }).collect()
    }
    fn unavailable_plan_diagnostics(plan: &ExecutionPlan) -> Vec<String> {
        let mut messages = plan.branches.iter()
            .filter(|branch| branch.readiness == PlanReadiness::Unavailable || !branch.required && branch.readiness == PlanReadiness::Blocked)
            .map(|branch| format!("{}:{} is unavailable; this branch is inactive.", branch.sink.node_id, branch.sink.port_id))
            .collect::<Vec<_>>();
        messages.extend(plan.sources.iter().filter(|source| source.readiness == PlanReadiness::Unavailable)
            .map(|source| format!("{}:{} is offline; its input is inactive.", source.endpoint.node_id, source.endpoint.port_id)));
        messages.extend(plan.visibility.iter().filter(|intent| intent.readiness == PlanReadiness::Unavailable)
            .map(|intent| format!("{} is offline; its visibility setting is inactive.", intent.resource_id)));
        dcs_hid::unique_diagnostics(messages)
    }
    fn waiting_for_vehicle(document:&osg_profiles::ProfileDocument,vehicle_id:Option<&str>)->bool {
        !document.vehicle_tabs.is_empty() && vehicle_id.is_none()
    }
    fn selected_vehicle_tab(games:&crate::games::GamesRuntime, document:&osg_profiles::ProfileDocument, game_id:Option<&str>) -> Option<String> {
        let game_id=game_id?;
        let vehicle_id=games.current_vehicle_id(game_id)?;
        document.vehicle_tabs.iter().find(|tab|tab.game_id==game_id&&tab.vehicle_id==vehicle_id).map(|tab|tab.id.clone())
    }
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
            recovery_publications: BTreeMap::new(),
            last_desktop_status: serde_json::Value::Null,
            progress: None,
            cleanup_epoch: None,
        }
    }
    /// Reports work from the exact captured admission, never from a timer or
    /// from whichever profile happens to be active when a callback is read.
    pub(crate) fn with_progress(mut self, callback: ProgressCallback) -> Self {
        self.progress = Some(callback);
        self
    }
    fn report_progress(&self, owner: &RunOwner, step: &str, message: &str) {
        if let Some(epoch) = self.progress_epoch(owner) {
            Self::emit_progress(self.progress.as_ref(), epoch, step, message);
        }
    }
    fn progress_epoch(&self, owner: &RunOwner) -> Option<u64> {
        // Stop/Recover has its own exact admission. Cold recovery deliberately
        // has no prepared executable run, but must still report its real work.
        self.cleanup_epoch.or_else(|| self.prepared.get(&key(owner))
            .filter(|state| state.owner == *owner).map(|state| state.admission_epoch))
    }
    fn emit_progress(callback: Option<&ProgressCallback>, epoch: u64, step: &str, message: &str) {
        if let Some(callback) = callback { callback(epoch, step, message); }
    }
    fn desktop_binding_kind(program: &osg_mapping::program::Program) -> &'static str {
        use osg_mapping::{desktop::Action, program::SinkKind};
        let (mut keyboard, mut mouse) = (false, false);
        for sink in &program.sinks {
            if let SinkKind::Desktop { action } = &sink.kind {
                match action {
                    Action::KeyHold { .. } | Action::KeyTap { .. } | Action::Chord { .. } | Action::Repeat { .. } => keyboard = true,
                    Action::MouseHold { .. } | Action::MouseClick { .. } | Action::Wheel { .. } | Action::Pointer { .. } => mouse = true,
                }
            }
        }
        match (keyboard, mouse) {
            (true, false) => "keyboard bindings",
            (false, true) => "mouse bindings",
            _ => "keyboard and mouse bindings",
        }
    }
    fn run(&mut self, owner: &RunOwner) -> Result<&mut NativeRun, String> {
        self.prepared
            .get_mut(&key(owner))
            .filter(|run| run.owner == *owner)
            .ok_or_else(|| "The native prepared run expired; capture a fresh profile plan.".into())
    }
    fn game_baseline(&mut self, owner: &RunOwner) -> Result<Vec<osg_mapping::engine::typed::Frame>, String> {
        if self.run(owner)?.game.requires_baseline() {
            self.report_progress(owner, "game-telemetry-baseline", "Waiting for fresh game telemetry");
        }
        let games = self.games.clone();
        let resources = self.resources.clone();
        let controllers = self.controllers.clone();
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        let mut first = true;
        loop {
            self.check_transition(owner)?;
            let state = self.run(owner)?;
            if !state.game.requires_baseline() { return Ok(vec![]); }
            if let Some(output) = state.output.as_mut() {
                output.keep_alive().map_err(|error| format!("Virtual output could not stay neutral while acquiring game telemetry. {error}"))?;
            }
            if let Some(desktop) = state.desktop.as_mut() { desktop.keep_alive()?; }
            let clock = || controllers.stream_page("", 0).captured_us;
            let sample = if first { state.game.baseline(&games, clock) } else { state.game.poll_baseline(&games, clock) };
            first = false;
            let batch = state.game.delivery_batch();
            self.check_transition(owner)?;
            let published = batch.is_some();
            if let Some(batch) = batch { resources.publish_profile_game_delivery(batch)?; }
            if sample.context_lost {
                return Err(format!("The verified game context changed during baseline acquisition. {}", sample.diagnostics.join(" ")));
            }
            if Instant::now() >= deadline {
                return Err("The game provider did not produce an advancing fresh baseline before activation.".into());
            }
            if sample.fresh && !published { return Ok(sample.frames); }
            // Publishing a catalogue can itself take longer than a lease.
            // Reacquire a complete live baseline after that work; never extend
            // the accepted counter's freshness or redate already decoded bytes.
            if published { first = true; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn panel_baseline(&mut self, owner: &RunOwner, delivery: &ExecutionPlan, live: bool) -> Result<Vec<osg_mapping::engine::typed::Frame>, String> {
        let resources = self.resources.clone();
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        if delivery.sources.iter().any(|source| source.route.as_ref().is_some_and(|route|
            matches!(route.transport, Transport::Usb | Transport::Can) && !route.id.starts_with("companion:"))) {
            self.report_progress(owner, "peripheral-input-baseline", "Waiting for fresh peripheral input states");
        }
        if live {self.check_transition(owner)?;self.run(owner)?.panels.begin_live_baseline()?;}
        loop {
            self.check_transition(owner)?;
            let state = self.run(owner)?;
            let baseline=if live {state.panels.live_baseline(delivery,|token|Self::validate_token(&resources,token))?}else{state.panels.baseline(delivery,|token|Self::validate_token(&resources,token))?};
            if let Some(frames) = baseline {
                return Ok(frames);
            }
            if Instant::now() >= deadline {
                return Err("The selected native panels did not supply fresh baselines before activation.".into());
            }
            if let Some(output) = state.output.as_mut() { output.keep_alive()?; }
            if let Some(desktop) = state.desktop.as_mut() { desktop.keep_alive()?; }
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
                    SinkAdapter::VirtualController | SinkAdapter::SimulatorCommand | SinkAdapter::CompanionFeedback | SinkAdapter::PhysicalAction | SinkAdapter::DesktopInput
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
                            && (resource.summary.attributes.get("catalogueRevision")
                                == Some(&game.proof.catalogue_revision)
                                || game.proof.game_id == "dcs"
                                    && resource.summary.attributes.get("aircraftModule").map(String::as_str) == Some("CommonData"))
                            && (game.proof.game_id != "dcs"
                                || resource.summary.attributes.get("aircraftModule").is_none_or(|module| module == "CommonData" || Some(module.as_str()) == game.aircraft_module.as_deref()))
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
    fn context_desktop(document:&osg_profiles::ProfileDocument)->bool {
        document.desktop_outputs.first().is_some_and(|output|matches!(output.scope.as_ref(),None|Some(osg_profiles::DesktopScope::SelectedGame)))
    }
    fn refresh_desktop_context(games:&crate::games::GamesRuntime,state:&mut NativeRun)->Result<Vec<String>,String>{
        if !Self::context_desktop(&state.document)||state.desktop.is_none(){return Ok(vec![]);}
        let id=state.child_game_owner.as_ref().and_then(|owner|owner.game_id.as_deref()).or(state.owner.game_id.as_deref());
        let Some(id)=id else{return Ok(vec![]);};
        let (scope,diagnostics)=match games.desktop_process(id) {
            Ok(process)=>(process.map(|process|osg_desktop_output::Scope::Application{identity:crate::profile_session_desktop::identity(&process)}),vec![]),
            Err(error)=>(None,vec![format!("Context scoped bindings are waiting: {error}")]),
        };
        state.desktop.as_mut().unwrap().refresh_context(scope.clone())?;
        state.desktop_scope=scope;
        Ok(diagnostics)
    }
    fn withdraw_game(resources: &crate::profile_resources::Runtime, state: &NativeRun) -> Result<(), String> {
        if let Some((game, reader)) = state.game.provider_identity() {
            resources.withdraw_profile_game_delivery(game, reader)?;
        }
        Ok(())
    }
    fn game_poll(
        resources: &Arc<crate::profile_resources::Runtime>,
        games: &Arc<crate::games::GamesRuntime>,
        state: &mut NativeRun,
        clock: impl Fn() -> u64,
    ) -> Result<crate::profile_session_games::GamePoll, String> {
        let now_us = clock();
        let mut resume_diagnostics=vec![];
        if let Some(receiver)=state.game_resume.take() {
            match receiver.try_recv() {
                Ok(result)=>match result {
                    Ok(Some(mut replacement))=>{
                        if state.game.accepts_replacement(&replacement.proof) {
                            replacement.game.inherit_source_counters(&state.game);
                            match replacement.game.admit(games) {
                                Ok(())=>{state.game=replacement.game;state.game_preparation=Some(replacement.proof);state.delivery=Some(replacement.plan);},
                                Err(error)=>resume_diagnostics.push(format!("Game integration is waiting: {error}")),
                            }
                        }
                    },
                    Ok(None)=>{},
                    Err(error)=>resume_diagnostics.push(format!("Game integration is waiting: {error}")),
                },
                Err(std::sync::mpsc::TryRecvError::Empty)=>state.game_resume=Some(receiver),
                Err(std::sync::mpsc::TryRecvError::Disconnected)=>resume_diagnostics.push("The game preparation worker ended; waiting to retry.".into()),
            }
        }
        if state.game.needs_context() && state.game_resume.is_none() && state.game_retry.is_none_or(|next|now_us>=next) {
            state.game_retry=Some(now_us.saturating_add(500_000));
            let id=state.child_game_owner.as_ref().and_then(|owner|owner.game_id.as_ref()).or(state.owner.game_id.as_ref()).cloned();
            if let Some(id)=id {
                let (sender,receiver)=std::sync::mpsc::channel();
                let games=games.clone();let resources=resources.clone();
                let document=state.document.clone();let assignments=state.assignments.clone();let backend=state.backend.clone();
                let program_hash=state.report.program_hash.clone();let previous=state.game.preparation_session().map(str::to_owned);
                // Catalogue and executable inspection can be slow. Keep it off
                // the active owner's health/lease renewal path. Late readers
                // have no command authority and are dropped after Stop.
                std::thread::spawn(move||{
                    let result=(||->Result<Option<NativeGameResume>,String>{
                        let Some(proof)=games.prepare_profile_game_context(&id)? else{return Ok(None);};
                        if previous.as_deref()==Some(proof.proof.session.as_str()){return Ok(None);}
                        let mut snapshot=resources.snapshot_for_compile(assignments)?;
                        snapshot.backend=backend;Self::game_capture(&mut snapshot,Some(&proof));
                        let report=osg_profiles::compiler::compile(&document,&snapshot);
                        if report.program_hash!=program_hash||matches!(report.readiness,PlanReadiness::Invalid|PlanReadiness::Blocked){return Err("The returning game's mappings changed or are unavailable; save the profile to update them.".into());}
                        let plan=report.plan.ok_or("The returning game has no native delivery plan.")?;
                        let mut game=crate::profile_session_games::GameSources::prepare(&plan,Some(proof.clone()))?;
                        game.acquire(&games)?;
                        Ok(Some(NativeGameResume{game,proof,plan}))
                    })();
                    let _=sender.send(result);
                });
                state.game_resume=Some(receiver);
            }
        }
        let mut sample = state.game.poll(games, &clock);
        sample.diagnostics.extend(resume_diagnostics);
        if let Some(batch) = state.game.delivery_batch() {
            if let Err(error) = resources.publish_profile_game_delivery(batch) {
                sample = state.game.fail(clock(), error);
            }
        }
        if !sample.context_lost && state.game.requires_baseline() && state.game.has_delivery_publication() {
            let validation = state.game.provider_identity()
                .ok_or_else(|| "The acquired game reader has no native inventory owner.".to_owned())
                .and_then(|(game, reader)| resources.validate_profile_game_delivery(game, reader));
            if let Err(error) = validation { sample = state.game.fail(clock(), error); }
        }
        if sample.context_lost {
            Self::withdraw_game(resources, state)?;
            if sample.diagnostics.is_empty() {
                sample.diagnostics.push("Game integration routes are waiting for the game to return with a fresh verified context.".into());
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
        panels: &crate::gateways::ProfilePanelIo,
        effects: Vec<osg_mapping::engine::typed::Effect>,
    ) -> Result<Vec<String>, String> {
        let mut diagnostics = std::collections::BTreeSet::new();
        for effect in effects {
            if !state.game.can_command() { continue; }
            let sink = state.delivery.as_ref().and_then(|plan| plan.sinks.iter().find(|sink|
                sink.endpoint.node_id == effect.endpoint.node_id && sink.endpoint.port_id == effect.endpoint.port_id)).cloned();
            let Some(sink) = sink.filter(|sink| sink.adapter == SinkAdapter::SimulatorCommand) else {
                // Virtual-controller effects are submitted by the existing
                // recording/isolated-feeder adapter. Other adapters are blocked
                // by native admission until their concrete owners are installed.
                continue;
            };
            if state.delivery.as_ref().is_some_and(|plan| plan.branches.iter().any(|branch|
                branch.sink == sink.endpoint && branch.readiness == PlanReadiness::Unavailable)) { continue; }
            if !matches!(effect.sample, osg_mapping::program::SignalSample::Known(_)) {
                continue;
            }
            dcs_hid::append_diagnostics(&mut diagnostics, dcs_hid::refresh(state, resources, panels));
            if state.dcs_hid.blocked(&effect.endpoint) { continue; }
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
                    // A returning game can make a previously dormant command
                    // ready. Validate its HID publication obligations after
                    // that transition, before the first resumed invocation.
                    dcs_hid::append_diagnostics(&mut diagnostics,dcs_hid::refresh(state,resources,panels));
                    if state.dcs_hid.blocked(&effect.endpoint){return Ok(());}
                }
                if state.delivery.as_ref().is_some_and(|plan| plan.branches.iter().any(|branch|
                    branch.sink.node_id == effect.endpoint.node_id && branch.sink.port_id == effect.endpoint.port_id
                        && branch.readiness == PlanReadiness::Unavailable)) { return Ok(()); }
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
                dcs_hid::append_diagnostics(&mut diagnostics, loss.diagnostics);
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
        Ok(diagnostics.into_iter().collect())
    }
    pub(crate) fn stage(
        resources: &crate::profile_resources::Runtime,
        games: &crate::games::GamesRuntime,
        companions: &crate::companion_host::Runtime,
        controllers: Arc<osg_controller_input::ControllerService>,
        native: bool,
        candidate: &Candidate,
    ) -> Result<Arc<crate::profile_session::Stage>, String> {
        Self::stage_with_handoff(resources,games,companions,controllers,native,candidate,None)
    }
    pub(crate) fn stage_with_handoff(
        resources:&crate::profile_resources::Runtime,
        games:&crate::games::GamesRuntime,
        companions:&crate::companion_host::Runtime,
        controllers:Arc<osg_controller_input::ControllerService>,
        native:bool,
        candidate:&Candidate,
        outgoing:Option<OwnedRun>,
    )->Result<Arc<crate::profile_session::Stage>,String> {
        Self::stage_internal(resources,games,companions,controllers,native,candidate,outgoing,None,
            &osg_virtual_controller::inspection::current,
            &|ids|osg_virtual_controller::inspection::sample_current(ids).map(|sample|sample.into_parts()))
    }
    pub(crate) fn stage_with_output_handoff(
        resources:&crate::profile_resources::Runtime,
        games:&crate::games::GamesRuntime,
        companions:&crate::companion_host::Runtime,
        controllers:Arc<osg_controller_input::ControllerService>,
        native:bool,
        candidate:&Candidate,
        source:Option<crate::profile_session::HandoffSource>,
    )->Result<Arc<crate::profile_session::Stage>,String> {
        if source.as_ref().is_some_and(|source|Some(source.admission_epoch)!=candidate.admission_epoch) {
            return Err("The outgoing virtual proof belongs to another admission epoch.".into());
        }
        let outgoing=source.as_ref().map(|source|source.run.clone());
        Self::stage_internal(resources,games,companions,controllers,native,candidate,outgoing,source,
            &osg_virtual_controller::inspection::current,
            &|ids|osg_virtual_controller::inspection::sample_current(ids).map(|sample|sample.into_parts()))
    }
    fn stage_internal(
        resources:&crate::profile_resources::Runtime,
        games:&crate::games::GamesRuntime,
        companions:&crate::companion_host::Runtime,
        controllers:Arc<osg_controller_input::ControllerService>,
        native:bool,
        candidate:&Candidate,
        outgoing:Option<OwnedRun>,
        source:Option<crate::profile_session::HandoffSource>,
        inspect:&dyn Fn()->Result<Vec<osg_virtual_controller::vjoy::Slot>,String>,
        sample:&dyn Fn(&[u8])->Result<(Vec<osg_virtual_controller::vjoy::Slot>,Instant),String>,
    )->Result<Arc<crate::profile_session::Stage>,String> {
        if candidate.launch_preparation&&candidate.game_id.is_some(){return Err("Hardware-first launch preparation cannot capture a running game scope.".into())}
        let source_candidate=candidate;
        if candidate.game_id.as_deref() == Some("dcs") {
            for tab in candidate.document.vehicle_tabs.iter().filter(|tab| tab.game_id == "dcs") {
                games.register_dcs_vehicle_catalogue(&tab.vehicle_id)?;
            }
        }
        let vehicle_selection=Self::selected_vehicle_tab(games,&candidate.document,candidate.game_id.as_deref());
        let vehicle_id=vehicle_selection.as_ref().and_then(|id|candidate.document.vehicle_tabs.iter().find(|tab|&tab.id==id)).map(|tab|tab.vehicle_id.as_str());
        let mut selected=candidate.clone();
        selected.document=candidate.document.for_vehicle(candidate.game_id.as_deref(),vehicle_id);
        selected.assignments.retain(|assignment|selected.document.resources.iter().any(|resource|resource.id==assignment.resource_id));
        let candidate=&selected;
        // Vehicle telemetry can disappear while the process remains verified.
        // Start Base immediately with dormant game branches rather than waiting
        // for the missing telemetry that caused this fallback. The normal game
        // reader retry resumes those branches on fresh native data, including
        // vehicles without a configured tab.
        let current_vehicle=candidate.game_id.as_deref().and_then(|id|games.current_vehicle_id(id));
        let waiting=Self::waiting_for_vehicle(&source_candidate.document,current_vehicle.as_deref());
        let game_preparation = match candidate.game_id.as_deref().filter(|_|!waiting) {
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
            inspect().ok()
        } else {
            None
        };
        snapshot.backend.name = osg_mapping_host::service::BACKEND.into();
        snapshot.backend.available = slots.is_some();
        snapshot.backend.qualified =
            native && osg_mapping_host::service::LIVE_OUTPUT_QUALIFIED;
        crate::profile_session_desktop::capabilities(&mut snapshot.backend, native);
        let report = osg_profiles::compiler::compile_for_preparation(
            &candidate.document,
            &snapshot,
            &preparations,
        );
        let plan = report
            .plan
            .clone()
            .ok_or("The profile compiler did not produce an executable plan.")?;
        let publication=crate::profile_session_overrides::Staged::prepare(&plan,&snapshot)?;
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
        let mut reuses_output = false;
        let bindings = if !plan.program.controllers.is_empty() && slots.is_some() {
            match crate::profile_session_virtual::select_pinned(
                &plan.program.controllers,
                &candidate.virtual_slots,
                slots.as_deref().unwrap(),
                source.as_ref().and_then(|source|source.output.as_ref()),
            ) {
                Ok(selection) => { reuses_output = selection.reused; selection.bindings },
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
        let unavailable_peripherals = Self::unavailable_peripherals(&snapshot, &candidate.document);
        admission.optional_unavailable = Self::unavailable_plan_diagnostics(&plan);
        admission.optional_unavailable.extend(unavailable_peripherals.keys().map(|id| {
            let name = candidate.document.resources.iter().find(|resource| &resource.id == id).map_or(id.as_str(), |resource| resource.name.as_str());
            format!("{name} is offline; its settings and peripheral controls are inactive.")
        }));
        admission.optional_unavailable = dcs_hid::unique_diagnostics(admission.optional_unavailable);
        let prepared = Prepared {
            plan,
            admission,
            program_hash: report
                .program_hash
                .clone()
                .ok_or("The compiled profile has no program identity.")?,
            capture_fingerprint: report.capture_fingerprint.clone(),
        };
        let desktop_scope = if prepared.plan.requirements.desktop_input {
            let preferences = crate::profile_desktop_settings::Settings::native();
            if let Some(error) = preferences.error { return Err(error); }
            let declared = candidate.document.desktop_outputs.first().and_then(|output|output.scope.as_ref());
            let uses_game = matches!(declared, Some(osg_profiles::DesktopScope::SelectedGame)) || declared.is_none();
            let game = if uses_game {
                candidate.game_id.as_deref().map(|game_id|if candidate.launch_preparation { Ok(None) } else { games.desktop_process(game_id) }).transpose()?
            } else { None };
            crate::profile_session_desktop::prepare_scope(declared,candidate.launch_preparation,game,&preferences.value)?
        } else { None };
        let scripts = crate::profile_session_scripts::Scripts::prepare(
            &prepared.plan.program,
            &std::env::current_exe().map_err(|error| error.to_string())?,
        )?;
        let game = crate::profile_session_games::GameSources::prepare(
            &prepared.plan,
            game_preparation.clone(),
        )?;
        let companions = crate::profile_session_companions::CompanionSources::prepare(&prepared.plan, companion_bindings)?;
        let visibility=crate::profile_session_visibility::Visibility::stage_with_handoff(&prepared.plan,&snapshot,controllers,outgoing.as_ref())?;
        let dcs_hid = dcs_hid::Guard::capture(&prepared.plan, &snapshot);
        // Final off-lock I/O follows every slow catalogue/script/visibility
        // preparation. The inspector supplies the parent timestamp from before
        // its nonce-bound sample enqueue, after separate driver readiness.
        // Never replace that timestamp with a post-I/O clock or block outgoing
        // health under the coordinator lock while the inspector starts.
        let output_handoff = if reuses_output {
            let source=source.ok_or("The retiring virtual output proof disappeared.")?;
            let selected=bindings.iter().map(|binding|binding.slot).collect::<Vec<_>>();
            let (slots,observed)=sample(&selected)?;
            let captured=crate::profile_session_virtual::PreparedHandoff { source, slots, observed };
            captured.validate(Some(&captured.source.run),captured.source.output.as_ref(),&bindings,
                candidate.admission_epoch.ok_or("Missing native admission epoch.")?,Some(&captured.source.run.owner))
                .map_err(|error|format!("{error} [phase=stage sampleAgeUs={}]",captured.observed.elapsed().as_micros()))?;
            Some(captured)
        } else { None };
        crate::profile_session::Stage::new(source_candidate, NativePreparation {
            document:candidate.document.clone(),assignments:candidate.assignments.clone(),vehicle_selection,
            prepared, report, backend: snapshot.backend, bindings, output_handoff, scripts, game, game_preparation,
            companions, companion_catalogues, visibility, publication, dcs_hid, desktop_scope, unavailable_peripherals,
        })
    }
}
impl Host for NativeProfileHost {
    fn set_cleanup_progress(&mut self, epoch: Option<u64>) {
        self.cleanup_epoch = epoch;
    }
    fn activation_progress(&self,epoch:Option<u64>,step:&str,message:&str) {
        if let Some(epoch)=epoch {Self::emit_progress(self.progress.as_ref(),epoch,step,message);}
    }
    fn desktop_status(&self) -> serde_json::Value {
        self.authority.as_ref().and_then(|owner| self.prepared.get(&key(owner)))
            .and_then(|state| state.desktop.as_ref()).map_or_else(||self.last_desktop_status.clone(), |desktop| desktop.status())
    }
    fn output_handoff(&mut self, run: &OwnedRun, admission_epoch: u64) -> Result<Option<osg_virtual_controller::session::RunningOutput>, String> {
        Self::check_epoch(&self.admission_epoch, admission_epoch)?;
        self.fence(&run.owner)?;
        let journal=crate::profile_session_virtual::journal_name(&run.owner)?;
        let state = self.run(&run.owner)?;
        if state.bindings.is_empty() { return Ok(None); }
        let proof = state.output.as_mut().ok_or("The outgoing virtual output has no live Session.")?
            .handoff_snapshot()?.ok_or("The outgoing virtual output has no proved native worker.")?;
        if proof.token().session != run.owner.session || proof.token().generation.0 != run.owner.generation
            || serde_json::to_vec(proof.bindings()).map_err(|e|e.to_string())? != serde_json::to_vec(&state.bindings).map_err(|e|e.to_string())?
            || !run.receipts.iter().any(|receipt|receipt.kind=="virtual-output" && receipt.resource_id=="virtual-output"
                && receipt.state==json!({"journal":journal}))
        {
            return Err("The outgoing virtual output does not match its exact native owner and receipt.".into());
        }
        Ok(Some(proof))
    }
    fn inspect_topology(&self, run: &OwnedRun, request: &crate::profile_session::topology::Request) -> Result<crate::profile_session::topology::Page, String> {
        self.fence(&run.owner)?;
        let state=self.prepared.get(&key(&run.owner)).filter(|state|state.owner==run.owner)
            .ok_or("The applied native profile is no longer available.")?;
        let reducer=state.reducer.as_ref().filter(|reducer|reducer.active())
            .ok_or("The applied native reducer is stopped.")?;
        crate::profile_session::topology::capture_variant(&state.document,&state.source_document,reducer,run,request)
    }
    fn inspect(&self, run: &OwnedRun, request: &crate::profile_session::inspection::Request) -> Result<crate::profile_session::inspection::Page, String> {
        self.fence(&run.owner)?;
        let state = self.prepared.get(&key(&run.owner)).filter(|state| state.owner == run.owner)
            .ok_or("The applied native profile is no longer available.")?;
        let reducer = state.reducer.as_ref().filter(|reducer| reducer.active())
            .ok_or("The applied native reducer is stopped.")?;
        crate::profile_session::inspection::capture_variant(&state.document,&state.source_document, reducer, run, request)
    }
    fn prepare(&mut self, candidate: &Candidate, owner: &RunOwner) -> Result<Prepared, String> {
        let admission_epoch = candidate.admission_epoch.ok_or("Native profile preparation is missing its captured cancellation fence.")?;
        Self::check_epoch(&self.admission_epoch, admission_epoch)?;
        Self::emit_progress(self.progress.as_ref(), admission_epoch, "preparing-resources", "Preparing profile resources");
        let NativePreparation {document, assignments, vehicle_selection, mut prepared, report, backend, bindings, output_handoff, scripts, game, game_preparation, companions, companion_catalogues, visibility, publication, dcs_hid, desktop_scope, unavailable_peripherals} = candidate.stage.as_ref()
            .ok_or("A fresh native profile preparation is required.")?
            .take::<NativePreparation>(candidate)?;
        let panels = crate::profile_session_panels_inputs::PanelInputs::prepare(&prepared.plan, self.panels.clone())?;
        let visibility=visibility.bind(owner)?;
        let actions=crate::profile_session_actions::PanelActions::prepare(&prepared.plan, owner, self.resources.clone(), self.panels.clone())?;
        let publication=crate::profile_session_overrides::Overrides::prepare(&publication,owner,self.panels.clone(),self.admission_epoch.clone(),admission_epoch)?;
        prepared.admission.optional_unavailable.extend(actions.optional_unavailable().iter().cloned());
        self.prepared.insert(key(owner), NativeRun {
            owner: owner.clone(), admission_epoch, report, assignments,
            document, source_document:candidate.document.clone(),source_assignments:candidate.assignments.clone(),vehicle_selection,vehicle_paused:false,vehicle_error:None,
            backend, delivery: None, inputs: None, panels,
            baselines: vec![], reducer: None, recording: RecordingSink::default(),
            started: Instant::now(), clock_origin: 0, bindings, output: None, desktop: None, desktop_scope, output_handoff, scripts, game, game_preparation,
            child_game_owner: None, game_retry: None, game_resume: None, launch_epoch_bound: false, launch_context_installed: false,
            companions, companion_catalogues, companion_retry: None, companion_initial_operations:BTreeMap::new(), visibility,
            actions,
            publication, dcs_hid, acquisition_plan: None,
            admission_degradation: prepared.admission.optional_unavailable.clone(), unavailable_peripherals,
        });
        self.validate_prepared(&prepared, owner)?;
        Ok(prepared)
    }
    fn validate_prepared(&mut self, prepared: &Prepared, owner: &RunOwner) -> Result<(), String> {
        self.check_transition(owner)?;
        let state=self.prepared.get(&key(owner)).ok_or("Missing native vehicle preparation.")?;
        if Self::selected_vehicle_tab(&self.games,&state.source_document,owner.game_id.as_deref())!=state.vehicle_selection {
            return Err("The loaded vehicle changed during profile preparation; capture the current vehicle again.".into());
        }
        let assignments = self.run(owner)?.assignments.clone();
        let proof = self
            .run(owner)?
            .game_preparation
            .as_ref()
            .map(|game| game.proof.clone());
        if let Some(proof) = proof {
            self.games.validate_game_preparation(&proof)?;
        }
        self.resources.validate_preparation_state(&assignments)?;
        let companions = self.companions.clone();
        let run = self.run(owner)?;
        run.companions.validate_preparation(&companions)?;
        run.publication.validate()?;
        if run.report.capture_fingerprint != prepared.capture_fingerprint
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
        if let Some(captured)=self.run(owner)?.output_handoff.clone() {
            let previous=previous.ok_or("The retiring virtual output no longer has an active run.")?;
            let current=self.output_handoff(previous,captured.source.admission_epoch)?;
            let bindings=self.run(owner)?.bindings.clone();
            captured.validate(Some(previous),current.as_ref(),&bindings,
                self.admission_epoch.load(Ordering::Acquire),self.authority.as_ref())
                .map_err(|error|format!("{error} [phase=capture sampleAgeUs={}]",captured.observed.elapsed().as_micros()))?;
        }
        let runtime = self.companions.clone();
        let state = self.run(owner)?;
        state.visibility.validate_handoff(previous)?;
        let previous = previous.map_or(&[][..], |run|run.receipts.as_slice());
        let mut receipts = if !state.companions.is_empty() || companion::has_receipts(previous) {
            companion::capture(&runtime.profile_metadata()?, &state.companions, previous)?
        } else { vec![] };
        receipts.extend(state.visibility.receipts()?);
        receipts.extend(state.actions.capture_receipts());
        receipts.extend(state.publication.receipts());
        if !state.bindings.is_empty() {
            receipts.push(Receipt { kind: "virtual-output".into(), resource_id:"virtual-output".into(),
                state: json!({"journal":crate::profile_session_virtual::journal_name(owner)?}) });
        }
        if prepared.plan.requirements.desktop_input {
            // Context scopes can follow a restarted process. The receipt pins
            // the exact run journal; that journal durably records each newly
            // verified process scope before any binding can be armed.
            let scope=if Self::context_desktop(&state.document){None}else{state.desktop_scope.clone()};
            receipts.push(Receipt { kind: "desktop-output".into(), resource_id: "desktop-output".into(),
                state: json!({"journal":crate::profile_session_desktop::journal_name(owner)?,"scope":scope,"gameId":owner.game_id}) });
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
    fn recapture_phase(&mut self,phase:ApplyPhase,prepared:&mut Prepared,run:&OwnedRun,previous:Option<&OwnedRun>)->Result<Option<Vec<Receipt>>,String>{
        self.recapture_publication_phase(phase,prepared,run,previous)
    }
    fn prepare_recovery(&mut self, run: &OwnedRun) -> Result<(), String> {
        self.report_progress(&run.owner, "cleanup-validating-recorded-outputs", "Validating recorded profile outputs");
        self.prepare_action_recovery(run)
    }
    fn neutralize(&mut self, run: &OwnedRun) -> Result<(), String> {
        if self.prepared.contains_key(&key(&run.owner)) {
            self.report_progress(&run.owner, "neutralizing-outputs", "Stopping profile outputs");
        }
        // Stop extending the volatile overlay at the first neutralization
        // fence. Release still follows physical action restoration below.
        if let Some(actor) = self.recovery_publications.get(&key(&run.owner)) {
            actor.revoke_renewal();
        }
        let resources = self.resources.clone();
        let companions = self.companions.clone();
        let mut adapter_result=Ok(());
        let mut desktop_result=Ok(());
        let mut desktop_status=None;
        let progress=self.progress.clone();
        let progress_epoch=self.progress_epoch(&run.owner);
        let report=|step:&str,message:&str| {
            if let Some(epoch)=progress_epoch {Self::emit_progress(progress.as_ref(),epoch,step,message);}
        };
        if let Some(state) = self.prepared.get_mut(&key(&run.owner)) {
            state.publication.revoke_renewal();
            if let Some(mut desktop) = state.desktop.take() {
                let program=state.delivery.as_ref().or(state.acquisition_plan.as_ref()).or(state.report.plan.as_ref())
                    .map(|plan|&plan.program);
                let kind=program.map(Self::desktop_binding_kind).unwrap_or("keyboard and mouse bindings");
                report("cleanup-releasing-desktop-bindings",&format!("Releasing {kind}"));
                desktop_result = desktop.stop(); desktop_status=Some(desktop.status());
            }
            if let Some(reducer) = state.reducer.as_mut() {
                if reducer.active() {report("cleanup-stopping-mappings", "Stopping profile mappings and routes");}
                reducer.stop();
                state.recording.record(reducer.drain_reports());
                reducer.drain();
            }
            if run.attempted.contains(&ApplyPhase::Acquire) && !state.actions.is_empty()
                && !state.actions.poll().cleanup_complete {
                report("cleanup-releasing-peripheral-outputs", "Releasing peripheral output controls");
            }
            let _=state.actions.stop();
            if state.report.plan.as_ref().is_some_and(|plan|plan.program.nodes.iter()
                .any(|node|matches!(node.operation,osg_mapping::program::Operation::Script {..}))) {
                report("cleanup-stopping-scripts", "Stopping profile scripts");
            }
            state.scripts.stop();
            if state.game.provider_identity().is_some() || state.game_preparation.is_some() {
                report("cleanup-disconnecting-game", "Releasing profile game context and telemetry");
            }
            state.game.stop();
            state.panels.stop();
            let withdrawal = Self::withdraw_game(&resources, state);
            if run.attempted.contains(&ApplyPhase::Acquire) && !state.companions.is_empty() {
                report("cleanup-releasing-companion-bindings", "Releasing companion controls and feedback");
            }
            let companion_stop = state.companions.stop_native(&companions);
            let mut output_result = Ok(());
            if let Some(output) = state.output.as_mut() {
                if output.active() {
                    report("cleanup-removing-virtual-device", "Neutralizing and removing virtual controller output");
                }
                output_result = crate::profile_session_virtual::neutralize_output(output, &state.recording);
            }
            adapter_result=withdrawal.and(output_result).and(companion_stop);
        }
        if let Some(status)=desktop_status {self.last_desktop_status=status;}
        let visibility_result=self.restore_visibility(run);
        let action_result=self.restore_actions(run,false);
        adapter_result.and(desktop_result).and(visibility_result).and(action_result)?;
        self.restore_publication(run)
    }
    fn enter(
        &mut self,
        phase: ApplyPhase,
        prepared: &Prepared,
        run: &OwnedRun,
    ) -> Result<(), String> {
        self.check_transition(&run.owner)?;
        if phase!=ApplyPhase::Overrides { publication::active(self.run(&run.owner)?)?; }
        match phase {
            ApplyPhase::Overrides => self.acquire_publication(&run.owner),
            ApplyPhase::Acquire => {
                let native = self.native;
                let origin = self.controllers.stream_page("", 0).captured_us;
                if !prepared.plan.sources.is_empty() {
                    self.report_progress(&run.owner, "acquiring-inputs", "Connecting profile inputs");
                }
                let progress = self.progress.clone();
                let state = self.run(&run.owner)?;
                state.clock_origin = origin;
                state.panels.acquire(origin)?;
                if prepared.plan.requirements.desktop_input {
                    Self::emit_progress(progress.as_ref(), state.admission_epoch, "preparing-desktop-bindings", &format!("Preparing {}", Self::desktop_binding_kind(&prepared.plan.program)));
                    state.desktop = Some(crate::profile_session_desktop::Desktop::acquire(&run.owner, &prepared.plan.program, state.desktop_scope.clone())?);
                }
                if !state.bindings.is_empty() {
                    if !native || !osg_mapping_host::service::LIVE_OUTPUT_QUALIFIED {
                        return Err(osg_mapping_host::service::QUALIFICATION.into());
                    }
                    let path = crate::config_paths::config_path()
                        .with_file_name(crate::profile_session_virtual::journal_name(&run.owner)?);
                    Self::emit_progress(progress.as_ref(), state.admission_epoch, "creating-virtual-device", "Creating virtual controller output");
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
                if self.run(&run.owner)?.game.requires_baseline() {
                    self.report_progress(&run.owner, "connecting-game-telemetry", "Connecting game telemetry");
                }
                self.run(&run.owner)?.game.acquire(&games)?;
                let receipts = companion::receipts(&run.receipts)?;
                let runtime = self.companions.clone();
                if receipts.iter().any(|(release,_)|*release) {
                    self.report_progress(&run.owner, "companion-restoration", "Restoring companion panels from the previous profile");
                    companion::restore_latest(&runtime, &receipts, true)?;
                }
                let selected = receipts.into_iter().filter_map(|(release,receipt)|(!release).then_some(receipt)).collect::<Vec<_>>();
                let initial_operations=self.run(&run.owner)?.companion_catalogues.iter().filter_map(|catalogue|
                    runtime.profile_panel_operation_history(&catalogue.instance_id)
                        .map(|operation|(catalogue.instance_id.clone(),operation))).collect();
                self.run(&run.owner)?.companion_initial_operations=initial_operations;
                if !selected.is_empty() {
                    self.report_progress(&run.owner, "companion-assignment", "Assigning companion panels");
                }
                self.run(&run.owner)?.companions.acquire(&runtime, &selected)?;
                for catalogue in &self.run(&run.owner)?.companion_catalogues {
                    runtime.profile_retry_panel(&catalogue.instance_id,&catalogue.publication_hash)?;
                }
                self.acquire_actions(&run.owner)
            }
            ApplyPhase::Baseline => {
                if !prepared.plan.sources.is_empty() {
                    self.report_progress(&run.owner, "capturing-neutral-inputs", "Checking neutral input states");
                }
                let game_frames = self.game_baseline(&run.owner)?;
                self.check_transition(&run.owner)?;
                let (delivery, companion_frames) = self.companion_baseline(&run.owner, &prepared.plan)?;
                let panel_frames = self.panel_baseline(&run.owner, &delivery, false)?;
                if delivery.sources.iter().any(|source| source.route.as_ref().is_some_and(|route| route.transport == Transport::Hid)) {
                    self.report_progress(&run.owner, "controller-input-baseline", "Reading physical controller input states");
                }
                let (inputs, baselines) = crate::profile_session_inputs::HidInputs::capture(
                    &delivery,
                    self.controllers.as_ref(),
                    |token| Self::validate_token(&self.resources, token),
                ).map_err(|error| format!("Initial physical input baseline failed. {error}"))?;
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
                let panels_io = self.panels.clone();
                let admission_epoch=self.admission_epoch.clone();
                let progress = self.progress.clone();
                let (assignments, document, backend, game_preparation) = {
                    let state = self.run(&run.owner)?;
                    (
                        state.assignments.clone(),
                        state.document.clone(),
                        state.backend.clone(),
                        state.game_preparation.clone(),
                    )
                };
                // Acquisition/visibility can outlast telemetry freshness. Wait
                // without starving Stop or the already acquired output lease.
                self.game_baseline(&run.owner)
                    .map_err(|error| format!("Profile activation could not refresh game telemetry before command admission. {error}"))?;
                {
                    let state = self.run(&run.owner)?;
                    state.game.admit(&games)?;
                    if let Some(batch) = state.game.delivery_batch() {
                        resources.publish_profile_game_delivery(batch)?;
                    }
                }
                // Visibility and panel setup may have invalidated the incoming
                // panel session. Reacquire actual baselines before final compile.
                self.companion_baseline(&run.owner, &prepared.plan)?;
                self.report_progress(&run.owner, "configuring-mappings", "Configuring profile mappings");
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
                let panel_frames = self.panel_baseline(&run.owner, &delivery, true)?;
                self.bind_actions(&run.owner, &delivery)?;
                let companion_hub = if self.run(&run.owner)?.companions.is_empty() { None } else { Some(self.companions.profile_metadata()?) };
                if delivery.sources.iter().any(|source| source.route.as_ref().is_some_and(|route| route.transport == Transport::Hid)) {
                    self.report_progress(&run.owner, "controller-input-baseline", "Reading physical controller input states");
                }
                let (inputs, baselines) = crate::profile_session_inputs::HidInputs::capture(
                    &delivery,
                    controllers.as_ref(),
                    |token| Self::validate_token(&resources, token),
                ).map_err(|error| format!("Final physical input baseline failed. {error}"))?;
                let game_frames = self.game_baseline(&run.owner)
                    .map_err(|error| format!("Profile activation could not refresh game telemetry before arming. {error}"))?;
                let state = self.run(&run.owner)?;
                state.companions.bind_delivery_plan(&delivery)?;
                let companion_frames = match companion_hub {
                    Some(ref hub) => state.companions.baseline(hub, |token|Self::validate_token(&resources,token), controllers.stream_page("",0).captured_us)
                        .map_err(|error| format!("Final companion baseline failed. {error}"))?
                        .ok_or("The companion baseline expired before arming the profile.")?,
                    None => vec![],
                };
                state.inputs = Some(inputs);
                state.baselines = baselines;
                state.baselines.extend(game_frames);
                state.baselines.extend(panel_frames);
                state.baselines.extend(companion_frames);
                state.delivery = Some(delivery);
                dcs_hid::validate(state, &resources, &panels_io)?;
                Self::check_epoch(&admission_epoch,state.admission_epoch)?;
                let started = Instant::now();
                let origin = controllers.stream_page("", 0).captured_us;
                let mut reducer = Phase::new_at(
                    prepared.plan.program.clone(),
                    Counter(run.owner.generation),
                    run.owner.session.clone(),
                    Counter(origin),
                )?;
                state.clock_origin = origin;
                for baseline in state.baselines.drain(..) {
                    state.clock_origin = state.clock_origin.max(baseline.captured_us.0);
                    reducer.accept(baseline)?;
                }
                visibility::active(&mut state.visibility)?;
                publication::active(state)?;
                state.companions.arm(reducer.run_token())?;
                state.scripts.poll(&mut reducer);
                state.recording.record(reducer.drain_reports());
                let effects = reducer.drain();
                state.reducer = Some(reducer);
                if let Some(desktop) = state.desktop.as_mut() {
                    Self::emit_progress(progress.as_ref(), state.admission_epoch, "activating-desktop-bindings", &format!("Activating {}", Self::desktop_binding_kind(&prepared.plan.program)));
                    desktop.start(state.reducer.as_ref().unwrap(), state.game_preparation.as_ref(), state.desktop_scope.clone())?;
                }
                Self::check_epoch(&admission_epoch,state.admission_epoch)?;
                Self::emit_progress(progress.as_ref(), state.admission_epoch, "starting-routes", "Activating profile routes");
                action::start(state)?;
                state.started = started;
                let _physical_diagnostics = action::dispatch(state,&effects);
                let diagnostics = Self::dispatch_game(&resources, &games, state, &panels_io, effects)?;
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
        if !run.receipts.is_empty() {
            self.report_progress(&run.owner, "cleanup-validating-receipts", "Checking recorded output ownership");
        }
        let captured = companion::receipts(&run.receipts)?;
        let _publication_receipts=crate::profile_session_overrides::Overrides::from_run(run,self.panels.clone())?;
        let visibility_receipt=crate::profile_session_visibility::Visibility::from_receipts(run,self.controllers.clone())?;
        for receipt in &run.receipts {
            if receipt.kind == "desktop-output" {
                let scope: Option<osg_desktop_output::Scope> = serde_json::from_value(receipt.state["scope"].clone()).map_err(|_|"Malformed desktop application scope receipt.")?;
                if receipt.resource_id != "desktop-output" || receipt.state != json!({"journal":crate::profile_session_desktop::journal_name(&run.owner)?,"scope":scope,"gameId":run.owner.game_id}) {
                    return Err("The desktop cleanup journal does not match the exact native run.".into());
                }
                continue;
            }
            if companion::has_receipts(std::slice::from_ref(receipt)) || action::has_receipts(std::slice::from_ref(receipt)) || receipt.kind==crate::profile_session_overrides::KIND || receipt.kind=="controller-visibility" && visibility_receipt.is_some() { continue; }
            if receipt.kind != "virtual-output" || receipt.resource_id != "virtual-output"
                || receipt.state != json!({"journal":crate::profile_session_virtual::journal_name(&run.owner)?}) {
                return Err("Unknown native recovery receipts require inspection; external state was preserved.".into());
            }
        }
        self.restore_visibility(run)?;
        self.restore_actions(run,recovery)?;
        self.restore_publication(run)?;
        if run.attempted.contains(&ApplyPhase::Acquire) {
            if run.receipts.iter().any(|r|r.kind=="desktop-output") {
                let path = crate::config_paths::config_path().with_file_name(crate::profile_session_desktop::journal_name(&run.owner)?);
                if osg_persistence::Evidence::inspect(&path,"recovery-desktop",osg_desktop_output::coordinator::MAX_JOURNAL_BYTES)?.is_some() {
                    let receipt=run.receipts.iter().find(|r|r.kind=="desktop-output").unwrap();
                    let scope:Option<osg_desktop_output::Scope>=serde_json::from_value(receipt.state["scope"].clone()).map_err(|_|"Malformed desktop scope receipt.")?;
                    let token=osg_mapping::engine::RunToken {session:run.owner.session.clone(),generation:Counter(run.owner.generation)};
                    let record = osg_desktop_output::Coordinator::inspect_owned_journal(&path,&token,scope.as_ref())?;
                    if record.uncertain || !record.cleanup_complete || !record.held.is_empty() || record.in_flight.is_some() {
                        if !recovery { return Err("Desktop cleanup is uncertain; use explicit profile recovery.".into()); }
                        self.report_progress(&run.owner, "cleanup-recovering-desktop-bindings", "Releasing recorded keyboard and mouse bindings");
                        if let Some(state) = self.prepared.get_mut(&key(&run.owner)) { state.desktop.take(); }
                        let preferences = crate::profile_desktop_settings::Settings::native();
                        osg_desktop_output::Coordinator::recover_owned_journal(preferences.value.enabled, crate::profile_desktop_settings::qualified(), path, &token, scope.as_ref())?;
                        self.last_desktop_status=json!({"state":"stopped","recovered":true,"nativeSubmission":"Explicit recovery releases recorded possible holds only; actions remain stopped."});
                    }
                } else if run.attempted.contains(&ApplyPhase::Start) {
                    return Err("The desktop output journal is missing after Start was attempted. Cleanup cannot be proved; preserve the session and inspect the missing record before recovery.".into());
                }
            }
            if run.receipts.iter().any(|r|r.kind=="virtual-output") {
                self.report_progress(&run.owner, "cleanup-restoring-virtual-device", "Restoring recorded virtual controller state");
                if let Some(output) = self.prepared.get_mut(&key(&run.owner)).and_then(|state|state.output.as_mut()) {
                    crate::profile_session_virtual::restore_retained_output(output, &run.owner, recovery)?;
                }
                crate::profile_session_virtual::recover(&run.owner)?;
            }
            if !captured.is_empty() {
                self.report_progress(&run.owner, "cleanup-restoring-companion-panels", "Restoring companion panels");
                companion::restore_latest(&self.companions, &captured, false)?;
            }
        }
        self.prepared.remove(&key(&run.owner));
        Ok(())
    }
    fn health(&mut self, run: &OwnedRun) -> Result<Health, String> {
        self.fence(&run.owner)?;
        if self.vehicle_change(&run.owner).is_some() || self.run(&run.owner)?.vehicle_paused {
            if !self.run(&run.owner)?.vehicle_paused {
                // Retire held keys, virtual state and physical ownership before
                // slow variant preparation. An old aircraft/car never emits into
                // the newly loaded vehicle while its replacement is being built.
                self.neutralize(run)?;
                let state=self.run(&run.owner)?;
                state.vehicle_paused=true;
                state.bindings.clear();
            }
            let message=self.run(&run.owner)?.vehicle_error.as_ref().map(|error|format!("Vehicle settings could not be applied: {error}")).unwrap_or_else(||"The loaded vehicle changed; preparing its profile settings.".into());
            return Ok(Health {degraded:vec![message],stop:None});
        }
        let controllers = self.controllers.clone();
        let resources = self.resources.clone();
        let games = self.games.clone();
        let companions = self.companions.clone();
        let panels_io = self.panels.clone();
        let state = self.run(&run.owner)?;
        publication::active(state)?;
        visibility::active(&mut state.visibility)?;
        let (frames, mut degraded) = state
            .inputs
            .as_mut()
            .ok_or("The native source reader was not acquired.")?
            .poll(controllers.as_ref(), |token| {
                Self::validate_token(&resources, token)
            })?;
        degraded.extend(state.admission_degradation.iter().cloned());
        // Native context loss reaches the reducer before timers/scripts can
        // emit effects, withdrawing only branches that depend on that game.
        let game = Self::game_poll(&resources, &games, state, || controllers.stream_page("", 0).captured_us)?;
        degraded.extend(Self::refresh_desktop_context(&games,state)?);
        degraded.extend(dcs_hid::refresh(state, &resources, &panels_io));
        degraded.extend(game.diagnostics);
        let panels = state.panels.poll(|token| Self::validate_token(&resources, token))?;
        degraded.extend(panels.diagnostics);
        let companion = Self::companion_poll(&companions,&resources,state,controllers.stream_page("",0).captured_us);
        degraded.extend(companion.diagnostics);
        let reducer = state
            .reducer
            .as_mut()
            .ok_or("The profile reducer is stopped.")?;
        let frames = frames.into_iter().chain(game.frames).chain(panels.frames).chain(companion.frames).collect::<Vec<_>>();
        degraded.extend(reducer.accept_batch(frames.iter().cloned())?);
        let elapsed = u64::try_from(state.started.elapsed().as_micros())
            .map_err(|_| "Profile runtime clock exhausted.")?;
        let tick = Counter(
            reducer.at_us().0.max(
                state
                    .clock_origin
                    .checked_add(elapsed)
                    .ok_or("Profile runtime clock exhausted.")?,
            ),
        );
        if let Some(inputs) = state.inputs.as_ref() { inputs.refresh_liveness(controllers.as_ref(), reducer, tick)?; }
        for (source,generation,ports) in panels.liveness {
            reducer.observe_port_liveness(&source,generation,tick,&ports)?;
        }
        for (source,generation) in companion.liveness {
            reducer.observe_liveness(&source,generation,tick)?;
        }
        reducer.advance(tick)?;
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
        if let Some(desktop) = state.desktop.as_mut() {
            degraded.extend(desktop.poll(&frames, state.reducer.as_ref().unwrap())?);
        }
        degraded.extend(Self::dispatch_game(&resources, &games, state, &panels_io, effects)?);
        if let Some(output) = state.output.as_mut() {
            output.poll(&state.recording)?;
        }
        Ok(Health {
            degraded: dcs_hid::unique_diagnostics(degraded),
            stop,
        })
    }
    fn supports_launch_hold(&self) -> bool { true }
    fn vehicle_change(&self, owner:&RunOwner) -> Option<String> {
        let state=self.prepared.get(&key(owner))?;
        if state.source_document.vehicle_tabs.is_empty(){return None;}
        let game_id=owner.game_id.as_deref().or_else(||state.child_game_owner.as_ref().and_then(|owner|owner.game_id.as_deref()));
        let selection=Self::selected_vehicle_tab(&self.games,&state.source_document,game_id);
        (selection!=state.vehicle_selection || state.vehicle_paused).then(||selection.unwrap_or_else(||"base".into()))
    }
    fn peripheral_change(&self, owner: &RunOwner) -> Option<String> {
        let state = self.prepared.get(&key(owner)).filter(|state| state.owner == *owner)?;
        if state.unavailable_peripherals.is_empty() || state.vehicle_paused { return None; }
        let assignments = state.assignments.iter().filter(|assignment| state.unavailable_peripherals.contains_key(&assignment.resource_id)).cloned().collect::<Vec<_>>();
        let (snapshot, preparations) = self.resources.snapshot_for_preparation(assignments).ok()?;
        if snapshot.read_only { return None; }
        let mut returned = vec![];
        for assignment in &snapshot.assignments {
            if state.unavailable_peripherals.get(&assignment.resource_id) != Some(&assignment.source_id) { continue; }
            let Some(resource) = snapshot.resources.iter().find(|resource| resource.summary.id == assignment.source_id
                && resource.summary.availability == osg_profiles::resources::Availability::Online
                && matches!(resource.summary.resource_type, osg_profiles::resources::ResourceType::HostController | osg_profiles::resources::ResourceType::OsgModule | osg_profiles::resources::ResourceType::Companion)) else { continue; };
            let companion = resource.summary.resource_type == osg_profiles::resources::ResourceType::Companion;
            if assignment.ports.iter().any(|expected| {
                !resource.ports.iter().any(|port| port.descriptor.id == expected.observed_port_id
                    && port.contract_hash == expected.expected_contract_hash && (port.current || companion))
                    || !companion && !preparations.iter().any(|pin| pin.resolved().resource_id == assignment.source_id
                        && pin.resolved().port.id == expected.observed_port_id
                        && pin.resolved().port.contract_hash().ok().as_ref() == Some(&expected.expected_contract_hash))
            }) { continue; }
            returned.push(json!([assignment.resource_id,assignment.source_id,resource.summary.generation,resource.summary.capability_revision,
                resource.summary.connections.iter().map(|connection|(&connection.route.id,&connection.route.provider_generation)).collect::<Vec<_>>(),assignment.ports]));
        }
        if returned.is_empty() { return None; }
        // Only stable identity/capability/route generations participate. Fresh
        // pin nonces and global discovery revisions never trigger a takeover.
        use sha2::{Digest, Sha256};
        Some(format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&returned).ok()?)))
    }
    fn vehicle_switch_failed(&mut self,owner:&RunOwner,message:&str) {
        if let Some(state)=self.prepared.get_mut(&key(owner)){state.vehicle_error=Some(message.into());}
    }
    fn hold_health(&mut self, run:&OwnedRun)->Result<Health,String> {self.held_health(run)}
    fn bind_launch_epoch(&mut self, run:&OwnedRun, epoch:u64)->Result<(),String> {self.bind_held_epoch(run,epoch)}
    fn install_launch_context(&mut self, run:&OwnedRun, candidate:&Candidate)->Result<Prepared,String> {self.install_held_context(run,candidate)}
    fn discard_preparation(&mut self, owner: &RunOwner) {
        if let Some(mut state) = self.prepared.remove(&key(owner)) {
            state.publication.revoke_renewal();
            let _=state.actions.stop();
            state.scripts.stop();
            state.game.stop();
            state.panels.stop();
            let _ = state.companions.stop_native(&self.companions);
            let _ = Self::withdraw_game(&self.resources, &state);
            if state.visibility.needs_cleanup() { self.recovery_visibility.insert(key(owner),state.visibility); }
            if state.publication.needs_cleanup(){self.recovery_publications.insert(key(owner),state.publication);}
            self.discard_actions(owner,state.actions);
        }
    }
}

#[cfg(test)]
#[path = "profile_session_output_handoff_tests.rs"]
mod output_handoff_tests;
#[cfg(test)]
#[path = "profile_session_progress_tests.rs"]
mod progress_tests;
#[cfg(test)]
#[path = "profile_session_offline_tests.rs"]
mod offline_tests;

#[cfg(test)]
mod vehicle_provider_tests {
    use super::*;
    #[test]
    fn stale_vehicle_telemetry_does_not_block_base_but_fresh_vehicle_retains_baseline_guards() {
        let mut document=osg_profiles::default_document();
        assert!(!NativeProfileHost::waiting_for_vehicle(&document,None));
        document.eligibility.game_ids=vec!["dcs".into()];
        document.vehicle_tabs.push(osg_profiles::VehicleTab {id:"hornet".into(),game_id:"dcs".into(),vehicle_id:"FA-18C_hornet".into(),vehicle_name:"Hornet".into(),node_ids:vec![]});
        assert!(NativeProfileHost::waiting_for_vehicle(&document,None));
        assert!(!NativeProfileHost::waiting_for_vehicle(&document,Some("FA-18C_hornet")));
        assert!(!NativeProfileHost::waiting_for_vehicle(&document,Some("unconfigured-aircraft")));
    }
}

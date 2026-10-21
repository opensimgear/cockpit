//! A journaled hardware hold has no reducer. Only a sealed fresh game context
//! may complete it; its native hardware owner and restoration receipts stay put.
use super::*;
use osg_profiles::{compiler_models::DiagnosticSeverity, resources::PortPreparation, ResourceKind};
use std::time::Duration;

struct NativeLaunchContext {
    prepared: Prepared,
    report: CompilationReport,
    game: crate::profile_session_games::GameSources,
    proof: Option<crate::games::PreparedGame>,
    started: crate::games::launch::StartedGame,
}
fn launched_desktop_scope(current:Option<&osg_desktop_output::Scope>,required:bool,process:&crate::game_discovery::ProcessIdentity)->Option<osg_desktop_output::Scope>{
    if required && current.is_none(){
        Some(osg_desktop_output::Scope::Application{identity:crate::profile_session_desktop::identity(process)})
    }else{current.cloned()}
}

fn game_resource(document: &osg_profiles::ProfileDocument, id: &str) -> bool {
    document
        .resources
        .iter()
        .any(|resource| resource.id == id && resource.kind == ResourceKind::Game)
}
fn prepared_report(report: &CompilationReport) -> Result<Prepared, String> {
    let plan = report
        .plan
        .clone()
        .ok_or("The launch context has no compiled profile program.")?;
    let mut admission = Admission::default();
    if matches!(
        report.readiness,
        PlanReadiness::Invalid | PlanReadiness::Blocked
    ) {
        admission.blockers.extend(
            report
                .diagnostics
                .iter()
                .filter(|d| d.severity == DiagnosticSeverity::Error)
                .map(|d| AdmissionBlocker {
                    kind: if report.readiness == PlanReadiness::Invalid {
                        BlockerKind::Structural
                    } else {
                        BlockerKind::RequiredResource
                    },
                    message: d.message.clone(),
                }),
        );
        if admission.blockers.is_empty() {
            return Err(
                "The launched game requires an available compatible native resource.".into(),
            );
        }
    }
    admission
        .blockers
        .extend(NativeProfileHost::blockers(&plan));
    admission.waiting_baselines = plan
        .sources
        .iter()
        .filter(|s| {
            s.preparation
                .as_ref()
                .is_some_and(PortPreparation::requires_baseline)
        })
        .map(|s| format!("{}:{}", s.endpoint.node_id, s.endpoint.port_id))
        .collect();
    admission.dormant_branches = plan
        .branches
        .iter()
        .filter(|b| b.readiness == PlanReadiness::Dormant)
        .map(|b| format!("{}:{}", b.sink.node_id, b.sink.port_id))
        .collect();
    admission.optional_unavailable = plan
        .branches
        .iter()
        .filter(|b| !b.required && b.readiness == PlanReadiness::Blocked)
        .map(|b| format!("{}:{} is unavailable.", b.sink.node_id, b.sink.port_id))
        .collect();
    Ok(Prepared {
        plan,
        admission,
        program_hash: report
            .program_hash
            .clone()
            .ok_or("The launch context has no program identity.")?,
        capture_fingerprint: report.capture_fingerprint.clone(),
    })
}
fn hardware_pin(
    before_token: Option<&ResourceToken>,
    before_route: Option<&osg_profiles::resources::Route>,
    after_pin: Option<&PortPreparation>,
    after_route: Option<&osg_profiles::resources::Route>,
    validate: impl Fn(&ResourceToken) -> Result<(), String>,
) -> Result<(), String> {
    if before_route != after_route {
        return Err("An acquired hardware route changed while the game was starting.".into());
    }
    let Some(token) = before_token else {
        if before_route.is_some() {
            return Err("The held hardware has no actual delivery capability.".into());
        }
        return Ok(());
    };
    validate(token)?;
    let pin = after_pin.ok_or("The launched profile lost an acquired hardware preparation.")?;
    let route = pin.resolved();
    if token.inventory_session != pin.inventory_session()
        || token.capability_revision != pin.capability_revision()
        || token.provider_session != pin.provider_session()
        || token.access != pin.access()
        || token.resource_id != route.resource_id
        || token.route_id != route.route.id
        || token.provider_generation != route.route.provider_generation
        || token.port_id != route.port.id
        || token.contract_hash != route.port.contract_hash()?
    {
        return Err(
            "The hardware identity, capabilities or provider changed during game startup.".into(),
        );
    }
    Ok(())
}
fn unchanged_hardware(
    document: &osg_profiles::ProfileDocument,
    before: &ExecutionPlan,
    after: &ExecutionPlan,
    validate: impl Fn(&ResourceToken) -> Result<(), String>,
) -> Result<(), String> {
    if serde_json::to_vec(&before.program).map_err(|e| e.to_string())?
        != serde_json::to_vec(&after.program).map_err(|e| e.to_string())?
    {
        return Err("A game context cannot change the held hardware program.".into());
    }
    for source in before
        .sources
        .iter()
        .filter(|s| !game_resource(document, &s.endpoint.node_id))
    {
        let next = after
            .sources
            .iter()
            .find(|s| s.endpoint == source.endpoint)
            .ok_or("A held hardware source disappeared.")?;
        if source.inventory_resource_id != next.inventory_resource_id
            || source.observed_port_id != next.observed_port_id
            || source.contract != next.contract
            || source.readiness != next.readiness
        {
            return Err("A held hardware source binding changed during game startup.".into());
        }
        hardware_pin(
            source.token.as_ref(),
            source.route.as_ref(),
            next.preparation.as_ref(),
            next.route.as_ref(),
            &validate,
        )?;
    }
    for sink in before
        .sinks
        .iter()
        .filter(|s| !game_resource(document, &s.endpoint.node_id))
    {
        let next = after
            .sinks
            .iter()
            .find(|s| s.endpoint == sink.endpoint)
            .ok_or("A held hardware destination disappeared.")?;
        if sink.inventory_resource_id != next.inventory_resource_id
            || sink.observed_port_id != next.observed_port_id
            || sink.contract != next.contract
            || sink.readiness != next.readiness
            || sink.adapter != next.adapter
        {
            return Err("A held hardware destination binding changed during game startup.".into());
        }
        hardware_pin(
            sink.token.as_ref(),
            sink.route.as_ref(),
            next.preparation.as_ref(),
            next.route.as_ref(),
            &validate,
        )?;
    }
    Ok(())
}
impl NativeProfileHost {
    pub(super) fn effective_context_owner(
        &self,
        owner: Option<&RunOwner>,
    ) -> Result<Option<crate::game_context::ProfileOwner>, String> {
        let Some(owner) = owner else {
            return Ok(None);
        };
        if let Some(child) = self
            .prepared
            .get(&key(owner))
            .filter(|run| run.owner == *owner)
            .and_then(|run| run.child_game_owner.as_ref())
        {
            if child.session != owner.session
                || child.profile_id != owner.profile_id
                || child.applied_revision != owner.profile_revision
                || child.run_generation != owner.generation
                || child.game_id.is_none()
            {
                return Err("The game child does not belong to this exact hardware owner.".into());
            }
            return Ok(Some(child.clone()));
        }
        Ok(owner
            .game_id
            .as_ref()
            .map(|_| crate::game_context::ProfileOwner {
                session: owner.session.clone(),
                profile_id: owner.profile_id.clone(),
                applied_revision: owner.profile_revision,
                run_generation: owner.generation,
                game_id: owner.game_id.clone(),
            }))
    }
    /// Native-only, off-lock read acquisition. It does not acquire hardware,
    /// alter companion publications, compile scripts or preflight hiding again.
    pub(crate) fn stage_launch_context(
        resources: &crate::profile_resources::Runtime,
        games: &crate::games::GamesRuntime,
        controllers: &osg_controller_input::ControllerService,
        native: bool,
        candidate: &Candidate,
        started: crate::games::launch::StartedGame,
    ) -> Result<Arc<crate::profile_session::Stage>, String> {
        if candidate.launch_preparation || candidate.read_only || candidate.stage.is_some() || candidate.admission_epoch.is_none() {
            return Err("A launch context requires a fresh writable native candidate.".into());
        }
        games.validate_started_profile_game(&started)?;
        if started.owner.profile_id != candidate.document.id
            || started.owner.profile_revision != candidate.document.revision
            || started.owner.game_id.is_some()
            || !candidate.document.eligibility.game_ids.is_empty()
                && candidate
                    .game_id
                    .as_ref()
                    .is_some_and(|id| !candidate.document.eligibility.game_ids.contains(id))
        {
            return Err(
                "The native launched process does not match this profile and selected context."
                    .into(),
            );
        }
        // Provider readiness is optional. The independently verified process may
        // launch hardware-only, leaving unavailable integration branches dormant.
        let proof = match candidate.game_id.as_deref() {
            Some(id) => games.prepare_profile_game_context(id)?,
            None => None,
        };
        // Hardware-first launch starts with Base. Once native vehicle telemetry
        // arrives, the ordinary vehicle switch stages the matching full variant.
        let document=candidate.document.for_vehicle(None,None);
        let assignments=candidate.assignments.iter().filter(|assignment|document.resources.iter().any(|resource|resource.id==assignment.resource_id)).cloned().collect();
        let (mut snapshot, pins) = resources.snapshot_for_preparation(assignments)?;
        Self::game_capture(&mut snapshot, proof.as_ref());
        snapshot.backend.name = osg_mapping_host::service::BACKEND.into();
        snapshot.backend.available = if native && !candidate.document.virtual_outputs.is_empty() {
            osg_virtual_controller::inspection::current().is_ok()
        } else {
            false
        };
        snapshot.backend.qualified = native && osg_mapping_host::service::LIVE_OUTPUT_QUALIFIED;
        crate::profile_session_desktop::capabilities(&mut snapshot.backend, native);
        if candidate
            .document
            .visibility
            .iter()
            .any(|v| v.mode == osg_profiles::VisibilityMode::HiddenWhileActive)
        {
            let status = controllers.request(&json!({"command":"controller_visibility_status"}))?;
            snapshot.backend.visibility_available = native
                && cfg!(windows)
                && status["visibility"]["modes"].as_array().is_some_and(|m| {
                    m.iter()
                        .any(|m| m["mode"] == "hide-all" && m["supported"] == true)
                });
            snapshot.backend.visibility_qualified =
                native && status["visibility"]["qualified"] == true;
        }
        let report =
            osg_profiles::compiler::compile_for_preparation(&document, &snapshot, &pins);
        let prepared = prepared_report(&report)?;
        if !prepared.admission.blockers.is_empty() {
            return Err(prepared
                .admission
                .blockers
                .iter()
                .map(|b| b.message.as_str())
                .collect::<Vec<_>>()
                .join("; "));
        }
        let mut game =
            crate::profile_session_games::GameSources::prepare(&prepared.plan, proof.clone())?;
        game.acquire(games)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while game.requires_baseline() {
            let sample = game.poll(games, || controllers.stream_page("", 0).captured_us);
            if sample.context_lost {
                return Err(sample.diagnostics.join("; "));
            }
            if sample.fresh {
                break;
            }
            if Instant::now() >= deadline {
                return Err(
                    "The launched provider has not produced an advancing fresh native baseline."
                        .into(),
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(proof) = &proof {
            games.validate_game_preparation(&proof.proof)?;
        }
        games.validate_started_profile_game(&started)?;
        crate::profile_session::Stage::new(
            candidate,
            NativeLaunchContext {
                prepared,
                report,
                game,
                proof,
                started,
            },
        )
    }
    pub(super) fn bind_held_epoch(&mut self, run: &OwnedRun, epoch: u64) -> Result<(), String> {
        Self::check_epoch(&self.admission_epoch, epoch)?;
        if self.authority.is_some() {
            return Err("A pending launch cannot have delivery authority.".into());
        }
        let resources = self.resources.clone();
        let state = self.run(&run.owner)?;
        if state.reducer.is_some()
            || state.launch_epoch_bound
            || state.owner.game_id.is_some()
            || state.launch_context_installed
        {
            return Err("This exact hardware run cannot bind another launch hold.".into());
        }
        let mut snapshot = resources.snapshot_for_compile(state.assignments.clone())?;
        snapshot.backend = state.backend.clone();
        let report = osg_profiles::compiler::compile(&state.document, &snapshot);
        if matches!(
            report.readiness,
            PlanReadiness::Invalid | PlanReadiness::Blocked
        ) || report.program_hash != state.report.program_hash
        {
            return Err(
                "The held hardware lacks its current post-acquisition delivery capabilities."
                    .into(),
            );
        }
        let delivery = report
            .plan
            .ok_or("The held hardware delivery capture is missing.")?;
        let state = self.run(&run.owner)?;
        state.delivery = Some(delivery);
        state.admission_epoch = epoch;
        state.launch_epoch_bound = true;
        Ok(())
    }
    pub(super) fn held_health(&mut self, run: &OwnedRun) -> Result<Health, String> {
        self.check_transition(&run.owner)?;
        if self.authority.is_some() {
            return Err("A waiting launch unexpectedly gained delivery authority.".into());
        }
        let controllers = self.controllers.clone();
        let resources = self.resources.clone();
        let companions = self.companions.clone();
        let state = self.run(&run.owner)?;
        if state.reducer.is_some() || state.launch_context_installed {
            return Err("A waiting launch cannot run scripts, timers or a game context.".into());
        }
        visibility::active(&mut state.visibility)?;
        publication::active(state)?;
        let mut degraded = action::held(state)?;
        if let Some(output) = state.output.as_mut() {
            output.keep_alive()?;
        }
        if let Some(desktop) = state.desktop.as_mut() { desktop.keep_alive()?; }
        let (_, diagnostics) = state
            .inputs
            .as_mut()
            .ok_or("Held native hardware has no acquired reader.")?
            .poll(controllers.as_ref(), |token| {
                Self::validate_token(&resources, token)
            })?;
        degraded.extend(diagnostics);
        degraded.extend(
            state
                .panels
                .poll(|token| Self::validate_token(&resources, token))?
                .diagnostics,
        );
        if !state.companions.is_empty() {
            let hub = companions.profile_metadata()?;
            state
                .companions
                .baseline(
                    &hub,
                    |token| Self::validate_token(&resources, token),
                    controllers.stream_page("", 0).captured_us,
                )?
                .ok_or("The held companion lost its owned fresh baseline.")?;
        }
        // No frames, feedback, scripts, timers or action effects survive hold.
        state.baselines.clear();
        Ok(Health {
            degraded,
            stop: None,
        })
    }
    pub(super) fn install_held_context(
        &mut self,
        run: &OwnedRun,
        candidate: &Candidate,
    ) -> Result<Prepared, String> {
        self.check_transition(&run.owner)?;
        if self.authority.is_some() {
            return Err("Install game context only while delivery is disarmed.".into());
        }
        let payload = candidate
            .stage
            .as_ref()
            .ok_or("The launched context requires a one-use native capture.")?
            .take::<NativeLaunchContext>(candidate)?;
        let games = self.games.clone();
        let resources = self.resources.clone();
        let state = self.run(&run.owner)?;
        if state.reducer.is_some()
            || !state.launch_epoch_bound
            || state.launch_context_installed
            || candidate.admission_epoch != Some(state.admission_epoch)
            || candidate.read_only
            || serde_json::to_vec(&(&state.source_document, &state.source_assignments))
                .map_err(|e| e.to_string())?
                != serde_json::to_vec(&(&candidate.document, &candidate.assignments))
                    .map_err(|e| e.to_string())?
            || payload.started.owner != run.owner
            || payload.proof.as_ref().is_some_and(|proof| {
                candidate.game_id.as_deref() != Some(proof.proof.game_id.as_str())
            })
            || state.report.program_hash != payload.report.program_hash
        {
            return Err("The held native profile or exact launch context changed.".into());
        }
        games.validate_started_profile_game(&payload.started)?;
        if let Some(proof) = &payload.proof {
            games.validate_game_preparation(&proof.proof)?;
        }
        unchanged_hardware(
            &state.document,
            state
                .delivery
                .as_ref()
                .ok_or("The held hardware delivery pins were not captured.")?,
            &payload.prepared.plan,
            |token| Self::validate_token(&resources, token),
        )?;
        for pin in payload
            .prepared
            .plan
            .sources
            .iter()
            .filter_map(|s| s.preparation.as_ref())
            .chain(
                payload
                    .prepared
                    .plan
                    .sinks
                    .iter()
                    .filter_map(|s| s.preparation.as_ref()),
            )
        {
            resources.validate_preparation(pin)?;
        }
        state.child_game_owner =
            candidate
                .game_id
                .as_ref()
                .map(|_| crate::game_context::ProfileOwner {
                    session: run.owner.session.clone(),
                    profile_id: run.owner.profile_id.clone(),
                    applied_revision: run.owner.profile_revision,
                    run_generation: run.owner.generation,
                    game_id: candidate.game_id.clone(),
                });
        state.launch_context_installed = true;
        state.desktop_scope=launched_desktop_scope(state.desktop_scope.as_ref(),payload.prepared.plan.requirements.desktop_input,&payload.started.process);
        state.report = payload.report;
        state.game = payload.game;
        state.game_preparation = payload.proof;
        state.baselines.clear();
        Ok(payload.prepared)
    }
}
#[cfg(test)]
#[path = "profile_session_launch_host_tests.rs"]
mod tests;

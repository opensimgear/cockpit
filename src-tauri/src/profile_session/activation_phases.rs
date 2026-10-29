//! Pre-arm phase work; the caller owns a bounded neutral heartbeat around it.
use super::*;

impl NativeProfileHost {
    pub(super) fn enter_prepared(
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
                self.with_activation_heartbeat(&run.owner, |host| host.acquire_game_and_companions(run))
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
                let (debug,debug_frames)=crate::profile_debug_controller::DebugInputs::capture(&state.document,&delivery,run.owner.generation,state.clock_origin)?;
                state.debug_inputs=debug;
                state.baselines.extend(debug_frames);
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
                let (assignments, document, backend, game_preparation, companion_catalogues) = {
                    let state = self.run(&run.owner)?;
                    (
                        state.assignments.clone(),
                        state.document.clone(),
                        state.backend.clone(),
                        state.game_preparation.clone(),
                        state.companion_catalogues.clone(),
                    )
                };
                // Acquisition/visibility can outlast telemetry freshness. Wait
                // without starving Stop or the already acquired output lease.
                self.game_baseline(&run.owner)
                    .map_err(|error| format!("Profile activation could not refresh game telemetry before command admission. {error}"))?;
                self.with_activation_heartbeat(&run.owner, |host| {
                    let state = host.run(&run.owner)?;
                    state.game.admit(&games)?;
                    if let Some(batch) = state.game.delivery_batch() {
                        resources.publish_profile_game_delivery(batch)?;
                    }
                    Ok(())
                })?;
                // Visibility and panel setup may have invalidated the incoming
                // panel session. Reacquire actual baselines before final compile.
                self.companion_baseline(&run.owner, &prepared.plan)?;
                self.report_progress(&run.owner, "configuring-mappings", "Configuring profile mappings");
                let delivery = self.with_activation_heartbeat(&run.owner, |_| {
                    let mut snapshot = resources.snapshot_for_compile_with_companions(assignments, &companion_catalogues)?;
                    snapshot.backend = backend;
                    Self::game_capture(&mut snapshot, game_preparation.as_ref());
                    Ok(osg_profiles::compiler::compile(&document, &snapshot))
                })?;
                if matches!(delivery.readiness, PlanReadiness::Invalid | PlanReadiness::Blocked) {
                    let blockers=admission_diagnostics::blockers(&delivery,&document);
                    return Err(format!("Final native delivery is unavailable: {}",blockers.iter().map(|blocker|blocker.message.trim_end_matches('.')).collect::<Vec<_>>().join("; ")));
                }
                if delivery.program_hash.as_deref() != Some(&prepared.program_hash) {
                    return Err("The final native delivery program differs from its prepared program".into());
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
                let (started, effects) = self.with_activation_heartbeat(&run.owner, |host| {
                    let state = host.run(&run.owner)?;
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
                    let (debug,debug_frames)=crate::profile_debug_controller::DebugInputs::capture(&state.document,&delivery,run.owner.generation,state.clock_origin.max(controllers.stream_page("",0).captured_us))?;
                    state.debug_inputs=debug;
                    state.baselines.extend(debug_frames);
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
                    Ok((started, effects))
                })?;
                let state = self.run(&run.owner)?;
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
}

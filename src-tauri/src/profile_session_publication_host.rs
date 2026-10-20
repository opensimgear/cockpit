//! Composite publication phase admission. Serial work lives on bounded native
//! actors; the Coordinator persists every refreshed receipt before its claim.
use super::*;
use crate::profile_session_overrides::{Overrides, VerifiedPublication, KIND};
use std::time::Duration;
pub(super) fn active(state: &NativeRun) -> Result<(), String> {
    if !state.publication.is_empty() {
        let status = state.publication.poll();
        if !status.active || status.error.is_some() {
            return Err(status.error.unwrap_or(
                "The owned controller publication was revoked; a fresh Apply is required.".into(),
            ));
        }
        state.publication.verified(false)?.validate()?;
    }
    Ok(())
}

fn wait(
    actor: &mut Overrides,
    restoring: bool,
    mut guard: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        guard()?;
        let status = if restoring {
            actor.stop()?
        } else {
            actor.poll()
        };
        if restoring && status.stopped && status.cleanup_complete {
            return status.error.map_or(Ok(()), Err);
        }
        if !restoring && status.active && status.error.is_none() {
            return Ok(());
        }
        if !restoring && status.error.is_some() {
            let _ = actor.stop();
            return Err(status.error.unwrap());
        }
        if Instant::now() >= deadline {
            return Err(if restoring{"Publication restoration remains pending; its sealed recovery receipt was retained."}else{"Publication did not establish exact settled ownership and a fresh native input baseline before activation."}.into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
impl NativeProfileHost {
    pub(super) fn acquire_publication(&mut self, owner: &RunOwner) -> Result<(), String> {
        let epoch = self.admission_epoch.clone();
        if !self.run(owner)?.publication.is_empty() {
            self.report_progress(owner, "publishing-hardware-config", "Applying peripheral hardware configuration");
        }
        let state = self.run(owner)?;
        let expected = state.admission_epoch;
        if state.publication.is_empty() {
            return Ok(());
        }
        Self::check_epoch(&epoch, expected)?;
        state.publication.acquire()?;
        let result = wait(&mut state.publication, false, || {
            Self::check_epoch(&epoch, expected)
        });
        if result.is_err() {
            let _ = state.publication.stop();
        }
        result
    }
    pub(super) fn restore_publication(&mut self, run: &OwnedRun) -> Result<(), String> {
        if !run.attempted.contains(&ApplyPhase::Overrides) {
            return Ok(());
        }
        let owner_key = key(&run.owner);
        if self.prepared.get(&owner_key).is_some_and(|state| state.publication.needs_cleanup()) {
            self.report_progress(&run.owner, "cleanup-restoring-peripheral-config", "Restoring peripheral hardware configuration");
        }
        if let Some(state) = self.prepared.get_mut(&owner_key) {
            return wait(&mut state.publication, true, || Ok(()));
        }
        if !self.recovery_publications.contains_key(&owner_key) {
            let actor = Overrides::from_run(run, self.panels.clone())?;
            if actor.is_empty() {
                return Ok(());
            }
            self.recovery_publications.insert(owner_key.clone(), actor);
        }
        if self.recovery_publications.get(&owner_key).is_some_and(|actor| !actor.is_empty()) {
            self.report_progress(&run.owner, "cleanup-restoring-peripheral-config", "Restoring peripheral hardware configuration");
        }
        wait(
            self.recovery_publications.get_mut(&owner_key).unwrap(),
            true,
            || Ok(()),
        )?;
        self.recovery_publications.remove(&owner_key);
        Ok(())
    }
    fn publication_plan(
        &mut self,
        owner: &RunOwner,
        prepared: &Prepared,
        proof: &VerifiedPublication,
    ) -> Result<CompilationReport, String> {
        self.check_transition(owner)?;
        proof.validate()?;
        self.resources.refresh_native()?;
        self.check_transition(owner)?;
        let (assignments, document, backend, game, catalogues, before) = {
            let state = self.run(owner)?;
            (
                state.assignments.clone(),
                state.document.clone(),
                state.backend.clone(),
                state.game_preparation.clone(),
                state.companion_catalogues.clone(),
                state
                    .acquisition_plan
                    .clone()
                    .unwrap_or_else(|| prepared.plan.clone()),
            )
        };
        let (mut snapshot, pins) = self
            .resources
            .snapshot_for_preparation_with_companions(assignments, &catalogues)?;
        snapshot.backend = backend;
        Self::game_capture(&mut snapshot, game.as_ref());
        let report = osg_profiles::compiler::compile_for_preparation(&document, &snapshot, &pins);
        if report.program_hash.as_deref() != Some(&prepared.program_hash) {
            return Err(
                "The authored program changed during native publication acquisition.".into(),
            );
        }
        let after = report
            .plan
            .as_ref()
            .ok_or("The exact publication delivery plan is unavailable.")?;
        if matches!(
            report.readiness,
            PlanReadiness::Invalid | PlanReadiness::Blocked
        ) {
            return Err(
                "Waiting for the exact post-publication native capability and input baseline."
                    .into(),
            );
        }
        self.resources
            .validate_publication_rebind(&before, after, proof)?;
        self.check_transition(owner)?;
        Ok(report)
    }
    /// Refresh only already sealed physical targets. This never changes the
    /// logical graph, resource assignments, private owner or requested masks.
    pub(super) fn recapture_publication_phase(
        &mut self,
        phase: ApplyPhase,
        prepared: &mut Prepared,
        run: &OwnedRun,
        previous: Option<&OwnedRun>,
    ) -> Result<Option<Vec<Receipt>>, String> {
        if phase != ApplyPhase::Overrides && phase != ApplyPhase::Acquire {
            return Ok(None);
        }
        self.check_transition(&run.owner)?;
        let own = !self.run(&run.owner)?.publication.is_empty();
        let outgoing = previous.is_some_and(|run| run.receipts.iter().any(|r| r.kind == KIND));
        if !own && (!outgoing || phase == ApplyPhase::Acquire) {
            return Ok(None);
        }
        let proof = if phase == ApplyPhase::Overrides {
            if let Some(previous) = previous {
                if previous.receipts.iter().any(|r| r.kind == KIND) {
                    Some(self.prepared.get(&key(&previous.owner)).ok_or("The outgoing publication owner was not retained for exact restoration.")?.publication.verified(true)?)
                } else {
                    None
                }
            } else {
                None
            }
        } else if !self.run(&run.owner)?.publication.is_empty() {
            Some(self.run(&run.owner)?.publication.verified(false)?)
        } else {
            None
        };
        if let Some(proof) = proof {
            let deadline = Instant::now() + Duration::from_secs(3);
            let report = loop {
                match self.publication_plan(&run.owner,prepared,&proof){
                Ok(report)=>break report,
                Err(error) if error=="Waiting for the exact post-publication native capability and input baseline."&&Instant::now()<deadline=>{self.check_transition(&run.owner)?;std::thread::sleep(Duration::from_millis(5));},
                Err(error)=>return Err(error),
            }
            };
            prepared.plan = report
                .plan
                .clone()
                .ok_or("Publication lost its exact compiled plan.")?;
            prepared.capture_fingerprint = report.capture_fingerprint.clone();
            let panels = crate::profile_session_panels_inputs::PanelInputs::prepare(
                &prepared.plan,
                self.panels.clone(),
            )?;
            let actions = crate::profile_session_actions::PanelActions::prepare(
                &prepared.plan,
                &run.owner,
                self.resources.clone(),
                self.panels.clone(),
            )?;
            let state = self.run(&run.owner)?;
            state.acquisition_plan = Some(prepared.plan.clone());
            state.report = report;
            state.panels = panels;
            state.actions = actions;
        }
        let state = self.run(&run.owner)?;
        let mut receipts = run
            .receipts
            .iter()
            .filter(|r| r.kind != KIND && r.kind != "physical-action-lease")
            .cloned()
            .collect::<Vec<_>>();
        if phase == ApplyPhase::Overrides {
            receipts.extend(state.publication.recapture()?);
        } else {
            receipts.extend(state.publication.receipts());
        }
        receipts.extend(state.actions.capture_receipts());
        self.check_transition(&run.owner)?;
        Ok(Some(receipts))
    }
}

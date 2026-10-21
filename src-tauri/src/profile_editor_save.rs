//! A durable editor save can replace its own active run. Persistence succeeds
//! independently of activation; slow native preparation never delays Stop.
use super::*;
use crate::profile_session::{Candidate, RunOwner, State};

fn saved_game_context(document:&osg_profiles::ProfileDocument)->Option<String> {
    document.eligibility.game_ids.first().filter(|id|matches!(id.as_str(),"dcs"|"assetto-corsa-rally"|"x-plane-11"|"x-plane-12" | "assetto-corsa" | "assetto-corsa-competizione" | "assetto-corsa-evo")).cloned()
}

impl GatewayManager {
    /// Save an AI draft through the editor's normalization and atomic store
    /// transaction. Saving a draft never replaces or prepares a running owner.
    pub(super) fn save_profile_draft(&self, request: &Value) -> Result<Value, String> {
        let normalized = self.normalize_editor_panels(request)?;
        let mut response = self.profiles.lock().unwrap_or_else(|p| p.into_inner()).request(&normalized)?;
        self.decorate_profile_session(&mut response);
        response["saveApplication"] = json!({"state":"saved","error":null});
        Ok(response)
    }

    pub(super) fn save_profile_editor(&self, request: &Value, arrival: Option<u64>) -> Result<Value, String> {
        // Capture the run and commit storage under the same admission guard. A
        // later Stop or replacement owner must never be rearmed by Save.
        let (mut response, owner, candidate) = {
            let _admission = self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
            let normalized=self.normalize_editor_panels(request)?;
            let request=&normalized;
            let status = self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).status().clone();
            let owner = status.owner.filter(|owner| {
                matches!(status.state, State::Running | State::Degraded)
                    && request["document"]["id"] == owner.profile_id
            });
            let mut profiles = self.profiles.lock().unwrap_or_else(|p|p.into_inner());
            let response = profiles.request(request)?;
            let candidate = owner.as_ref().map(|owner| {
                let (document, assignments, read_only) = profiles.compile_source(&owner.profile_id)?;
                let game_id=saved_game_context(&document);
                Ok::<_, String>(Candidate { document, assignments, read_only, game_id,
                    launch_preparation: false, admission_epoch: arrival, stage: None })
            }).transpose();
            (response, owner, candidate)
        };
        let application = match owner {
            None => json!({"state":"inactive","error":null}),
            Some(owner) => {
                let result = candidate.and_then(|candidate| candidate.ok_or_else(|| "The saved profile is missing its active candidate.".to_owned()))
                    .and_then(|candidate| self.apply_saved_editor(candidate, &owner, arrival));
                match result {
                    Ok(()) => json!({"state":"applied","error":null}),
                    Err(error) => json!({"state":"failed","error":format!("Profile saved, but its changes could not be applied to the active profile: {error}")}),
                }
            }
        };
        self.decorate_profile_session(&mut response);
        response["saveApplication"] = application;
        Ok(response)
    }

    pub(super) fn apply_saved_editor(&self, mut candidate: Candidate, previous: &RunOwner, arrival: Option<u64>) -> Result<(), String> {
        let epoch = arrival.filter(|epoch| *epoch == self.profile_admission_epoch.load(Ordering::Acquire))
            .ok_or("Applying the saved changes was cancelled by a newer profile control.")?;
        if candidate.game_id.as_ref().is_some_and(|game| !candidate.document.eligibility.game_ids.is_empty()
            && !candidate.document.eligibility.game_ids.contains(game)) {
            return Err("The saved profile is not eligible for its active game context.".into());
        }
        if candidate.read_only { return Err("Repair profile storage before applying its saved changes.".into()); }
        self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).check_apply()?;
        let activation_id = self.start_native_profile_activation(
            &candidate.document.id, epoch, "saved-changes", candidate.game_id.as_deref(),
        )?;
        self.capture_profile_activation_id(&activation_id, &candidate.document, &candidate.assignments);
        let result = (|| {
            // The outgoing native run keeps receiving health renewals throughout
            // slow read-only preparation. Emergency controls hold neither lock.
            candidate.stage = Some(self.prepare_profile_at(&candidate, epoch)?);
            let _admission = self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
            let _game = self.game_admission.read().unwrap_or_else(|p|p.into_inner());
            if self.closed.load(Ordering::Acquire) || self.profile_admission_epoch.load(Ordering::Acquire) != epoch {
                return Err("Applying the saved changes was cancelled by deactivation, recovery or a newer profile control.".into());
            }
            self.guard_retired_recovery()?;
            let (document, assignments, read_only) = self.profiles.lock().unwrap_or_else(|p|p.into_inner()).compile_source(&previous.profile_id)?;
            let game_id=saved_game_context(&document);
            let saved = Candidate { document, assignments, read_only, game_id,
                launch_preparation: false, admission_epoch: Some(epoch), stage: None };
            candidate.stage.as_ref().ok_or("Fresh native preparation is missing.")?.validate(&saved)?;
            // Health can retire a run without an operator epoch change. Check its
            // exact owner under the same coordinator lock that performs takeover.
            let mut coordinator = self.profile_session.lock().unwrap_or_else(|p|p.into_inner());
            if coordinator.status().owner.as_ref() != Some(previous) {
                return Err("The active profile changed while these saved changes were preparing.".into());
            }
            // Keep the selected profile, Manual/Automatic mode and rule reconciler
            // unchanged. Only this exact active run is replaced.
            coordinator.apply(candidate)?;
            self.profile_admission_epoch.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })();
        self.finish_profile_activation(&activation_id, &match &result {
            Ok(()) => json!({"ok":true}),
            Err(error) => json!({"ok":false,"error":error}),
        });
        result
    }
}

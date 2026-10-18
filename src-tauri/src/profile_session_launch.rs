//! A launch holds the existing journaled takeover before Start. No reducer,
//! script or timer exists while the game process/context is being acquired.
use super::*;
use std::time::{Duration, Instant};

pub(super) struct Held {
    pub candidate: Candidate,
    pub prepared: Prepared,
    pub owner: RunOwner,
    pub deadline: Instant,
    pub epoch: Option<u64>,
}

impl Held {
    pub(super) fn new(candidate: Candidate, prepared: Prepared, owner: RunOwner) -> Self {
        Self {
            candidate,
            prepared,
            owner,
            deadline: Instant::now() + Duration::from_secs(90),
            epoch: None,
        }
    }
}
impl<H: Host> Coordinator<H> {
    pub(crate) fn begin_launch(&mut self, candidate: Candidate) -> Result<RunOwner, String> {
        if !candidate.launch_preparation || candidate.game_id.is_some() || !self.host.supports_launch_hold() {
            return Err("This native adapter cannot prepare a hardware-only launch hold.".into());
        }
        self.apply_mode(candidate, true)?;
        Ok(self
            .launch
            .as_ref()
            .ok_or("The native launch hold was not installed.")?
            .owner
            .clone())
    }
    fn held_run(&self, owner: &RunOwner) -> Result<&OwnedRun, String> {
        let held = self
            .launch
            .as_ref()
            .filter(|h| &h.owner == owner && h.deadline > Instant::now())
            .ok_or("The exact native launch hold expired or was cancelled.")?;
        if self.status.paused
            || self.status.state != State::AwaitingGame
            || self.status.owner.as_ref() != Some(&held.owner)
        {
            return Err("The native launch hold no longer owns its prepared hardware.".into());
        }
        self.manifest
            .pending
            .as_ref()
            .and_then(|p| p.incoming.as_ref())
            .filter(|run| &run.owner == owner)
            .ok_or_else(|| "The pending launch has no intact journaled owner.".into())
    }
    /// Manager supplies the exact epoch returned by its atomic commit. It may
    /// not adopt a later epoch loaded after Stop or another operator choice.
    pub(crate) fn bind_launch_epoch(&mut self, owner: &RunOwner, epoch: u64) -> Result<(), String> {
        let run = self.held_run(owner)?.clone();
        if self.launch.as_ref().unwrap().epoch.is_some() {
            return Err("The native launch epoch was already bound.".into());
        }
        self.host.bind_launch_epoch(&run, epoch)?;
        self.launch.as_mut().unwrap().epoch = Some(epoch);
        Ok(())
    }
    pub(crate) fn launch_candidate(
        &self,
        owner: &RunOwner,
        game_id: Option<&str>,
        epoch: u64,
    ) -> Result<Candidate, String> {
        self.held_run(owner)?;
        let held = self.launch.as_ref().unwrap();
        if held.epoch != Some(epoch)
            || game_id.is_some_and(|id| {
                !matches!(
                    id,
                    "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally"
                )
            })
        {
            return Err("The game or committed launch epoch is invalid.".into());
        }
        let mut candidate = held.candidate.clone();
        candidate.game_id = game_id.map(str::to_owned);
        candidate.launch_preparation=false;
        candidate.admission_epoch = Some(epoch);
        candidate.stage = None;
        Ok(candidate)
    }
    pub(crate) fn check_launch_hold(
        &mut self,
        owner: &RunOwner,
        epoch: u64,
    ) -> Result<Status, String> {
        let run = self.held_run(owner)?.clone();
        if self.launch.as_ref().unwrap().epoch != Some(epoch) {
            return Err("The exact committed launch epoch changed.".into());
        }
        let health = self.host.hold_health(&run)?;
        if let Some(reason) = health.stop {
            return Err(reason);
        }
        let mut status = self.status.clone();
        status.degradation = health.degraded;
        Ok(status)
    }
    /// Admission remains native and short; the caller stages/acquires the game
    /// reader off-lock before passing its one-use context payload here.
    pub(crate) fn finish_launch(
        &mut self,
        owner: &RunOwner,
        candidate: Candidate,
    ) -> Result<(), String> {
        let mut run = self.held_run(owner)?.clone();
        let held = self.launch.as_ref().unwrap();
        let expected = self.launch_candidate(
            owner,
            candidate.game_id.as_deref(),
            held.epoch.ok_or("Bind the committed launch epoch first.")?,
        )?;
        if candidate.stage.is_none()
            || serde_json::to_vec(&(
                &candidate.document,
                &candidate.assignments,
                candidate.read_only,
                &candidate.game_id,
                candidate.launch_preparation,
                candidate.admission_epoch,
            ))
            .map_err(|e| e.to_string())?
                != serde_json::to_vec(&(
                    &expected.document,
                    &expected.assignments,
                    expected.read_only,
                    &expected.game_id,
                    expected.launch_preparation,
                    expected.admission_epoch,
                ))
                .map_err(|e| e.to_string())?
        {
            return Err(
                "The saved profile or local assignment changed during launch preparation.".into(),
            );
        }
        let original_hash = held.prepared.program_hash.clone();
        let original_program =
            serde_json::to_vec(&held.prepared.plan.program).map_err(|e| e.to_string())?;
        let result = (|| {
            self.store.verify()?;
            let prepared = self.host.install_launch_context(&run, &candidate)?;
            if prepared.program_hash != original_hash
                || serde_json::to_vec(&prepared.plan.program).map_err(|e| e.to_string())?
                    != original_program
                || !prepared.admission.blockers.is_empty()
            {
                return Err("The game launch changed the captured hardware program or introduced a blocker.".into());
            }
            self.host.validate_prepared(&prepared, owner)?;
            run.capture_fingerprint = prepared.capture_fingerprint.clone();
            let mut next = self.manifest.clone();
            next.pending.as_mut().unwrap().incoming = Some(run.clone());
            self.persist(next)?;
            // Baseline was already journaled before Visibility. Refresh it after
            // startup; no duplicate phase or replayable journal command is added.
            self.host.enter(ApplyPhase::Baseline, &prepared, &run)?;
            run.attempted.push(ApplyPhase::Start);
            let mut next = self.manifest.clone();
            next.pending.as_mut().unwrap().incoming = Some(run.clone());
            self.persist(next)?;
            self.host.set_authority(Some(owner))?;
            self.host.enter(ApplyPhase::Start, &prepared, &run)?;
            self.complete_transition(candidate, owner.clone(), prepared)
        })();
        if let Err(error) = result {
            if self.status.state == State::RecoveryNeeded {
                return Err(error);
            }
            return self.fail_transition(error);
        }
        self.launch = None;
        Ok(())
    }
    /// Only the exact held owner can be compensated after a late launch error.
    /// A newer successful Apply is never stopped by an old launch response.
    pub(crate) fn cancel_launch(&mut self, owner: &RunOwner) -> Result<bool, String> {
        if self.launch.as_ref().is_none_or(|held| &held.owner != owner) {
            return Ok(false);
        }
        self.stop()?;
        Ok(true)
    }
    pub(super) fn poll_launch(&mut self) -> Result<(), String> {
        let owner = self
            .launch
            .as_ref()
            .ok_or("No launch is pending.")?
            .owner
            .clone();
        let result = self
            .held_run(&owner)
            .cloned()
            .and_then(|run| self.host.hold_health(&run));
        match result {
            Ok(health) if health.stop.is_none() => {
                self.status.degradation = health.degraded;
                Ok(())
            }
            other => {
                let reason = other.map_or_else(|e| e, |h| h.stop.unwrap());
                self.stop()?;
                self.status.state = State::Faulted;
                self.status.fault = Some(reason);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
#[path = "profile_session_launch_tests.rs"]
mod tests;

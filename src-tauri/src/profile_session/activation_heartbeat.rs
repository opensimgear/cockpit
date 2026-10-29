//! Renew only prepared outputs while synchronous activation work owns the caller.
//! The scoped pump cannot outlive that work or renew beyond its epoch/deadline.
use super::*;
use std::{sync::mpsc, time::Duration};

fn permitted(epoch: &AtomicU64, expected: u64, deadline: Instant) -> Result<(), String> {
    NativeProfileHost::check_epoch(epoch, expected)?;
    if Instant::now() >= deadline {
        return Err("Profile activation preparation exceeded its deadline.".into());
    }
    Ok(())
}

pub(super) fn during<T>(
    epoch: &AtomicU64,
    expected: u64,
    deadline: Instant,
    mut heartbeat: impl FnMut() -> Result<(), String> + Send,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    permitted(epoch, expected, deadline)?;
    heartbeat()?;
    permitted(epoch, expected, deadline)?;
    std::thread::scope(|scope| {
        let (finished, stop) = mpsc::channel();
        let pump = scope.spawn(move || loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match stop.recv_timeout(remaining.min(Duration::from_millis(10))) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok::<(), String>(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    permitted(epoch, expected, deadline)?;
                    heartbeat()?;
                }
            }
        });
        // Keep catalogue reads/publication on this caller. Joining the pump
        // before returning also prevents a cancelled publisher from running
        // after compensation or replacing a newer run's catalogue.
        let result = work();
        drop(finished);
        pump.join().map_err(|_| "Profile activation heartbeat worker panicked.".to_string())??;
        permitted(epoch, expected, deadline)?;
        result
    })
}

impl NativeProfileHost {
    pub(super) fn install_authority(&mut self, owner: Option<&RunOwner>) -> Result<(), String> {
        let context = self.effective_context_owner(owner)?;
        let previous_context = self.effective_context_owner(self.authority.as_ref())?;
        let retained_child = owner.is_none() && self.prepared.values().any(|run| run.child_game_owner.is_some());
        if self.authority.as_ref() == owner && !retained_child { return Ok(()); }
        if previous_context.is_none() && context.is_none() && !retained_child {
            self.authority = owner.cloned();
            return Ok(());
        }
        self.authority = None;
        self.games.set_profile_context_owner(context)?;
        self.authority = owner.cloned();
        Ok(())
    }

    pub(super) fn acquire_game_and_companions(&mut self, run: &OwnedRun) -> Result<(), String> {
        self.capture_game_predecessor(&run.owner)?;
        let games = self.games.clone();
        if self.run(&run.owner)?.game.requires_baseline() {
            self.report_progress(&run.owner, "connecting-game-telemetry", "Connecting game telemetry");
        }
        self.run(&run.owner)?.game.acquire(&games)?;
        self.check_transition(&run.owner)?;
        let receipts = companion::receipts(&run.receipts)?;
        let runtime = self.companions.clone();
        if receipts.iter().any(|(release, _)| *release) {
            self.report_progress(&run.owner, "companion-restoration", "Restoring companion panels from the previous profile");
            companion::restore_latest(&runtime, &receipts, true)?;
        }
        let selected = receipts.into_iter().filter_map(|(release, receipt)| (!release).then_some(receipt)).collect::<Vec<_>>();
        let initial_operations = self.run(&run.owner)?.companion_catalogues.iter().filter_map(|catalogue|
            runtime.profile_panel_operation_history(&catalogue.instance_id)
                .map(|operation| (catalogue.instance_id.clone(), operation))).collect();
        self.run(&run.owner)?.companion_initial_operations = initial_operations;
        if !selected.is_empty() {
            self.report_progress(&run.owner, "companion-assignment", "Assigning companion panels");
        }
        self.check_transition(&run.owner)?;
        self.run(&run.owner)?.companions.acquire(&runtime, &selected)?;
        for catalogue in self.prepared[&key(&run.owner)].companion_catalogues.iter()
            .filter(|catalogue| self.prepared[&key(&run.owner)].companions.has_resource(&catalogue.logical_resource_id)) {
            runtime.profile_retry_panel(&catalogue.instance_id, &catalogue.publication_hash)?;
        }
        self.check_transition(&run.owner)?;
        self.acquire_actions(&run.owner)
    }

    pub(super) fn capture_game_predecessor(&mut self, owner: &RunOwner) -> Result<(), String> {
        self.check_transition(owner)?;
        let state = self.run(owner)?;
        let game_id = state.child_game_owner.as_ref().and_then(|child| child.game_id.clone())
            .or_else(|| state.owner.game_id.clone());
        let predecessor = game_id.map(|game| self.resources.profile_game_reader(&game)
            .map(|reader| (game, reader))).transpose()?;
        self.check_transition(owner)?;
        self.run(owner)?.game_predecessor = predecessor;
        Ok(())
    }

    pub(super) fn with_activation_heartbeat<T>(
        &mut self,
        owner: &RunOwner,
        work: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with_prepared_heartbeat(owner, Duration::from_secs(3), true, work)
    }

    pub(super) fn with_prepared_heartbeat<T>(
        &mut self,
        owner: &RunOwner,
        timeout: Duration,
        include_desktop: bool,
        work: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        self.check_transition(owner)?;
        let epoch = self.admission_epoch.clone();
        let state = self.run(owner)?;
        if state.reducer.is_some() {
            return Err("Only prepared neutral outputs may use the activation heartbeat.".into());
        }
        let expected = state.admission_epoch;
        let mut output = state.output.take();
        let mut desktop = if include_desktop { state.desktop.take() } else { None };
        let result = during(&epoch, expected, Instant::now() + timeout, || {
            if let Some(output) = output.as_mut() {
                output.keep_alive().map_err(|error| format!("Virtual output could not stay neutral during profile activation. {error}"))?;
            }
            if let Some(desktop) = desktop.as_mut() { desktop.keep_alive()?; }
            Ok(())
        }, || work(self));
        let state = self.run(owner)?;
        state.output = output;
        if include_desktop { state.desktop = desktop; }
        result
    }

    pub(super) fn game_baseline(&mut self, owner: &RunOwner) -> Result<Vec<osg_mapping::engine::typed::Frame>, String> {
        if !self.run(owner)?.game.requires_baseline() { return Ok(vec![]); }
        self.report_progress(owner, "game-telemetry-baseline", "Waiting for fresh game telemetry");
        self.with_activation_heartbeat(owner, |host| {
            let games = host.games.clone();
            let resources = host.resources.clone();
            let controllers = host.controllers.clone();
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut first = true;
            loop {
                host.check_transition(owner)?;
                let state = host.run(owner)?;
                let clock = || controllers.stream_page("", 0).captured_us;
                let sample = if first { state.game.baseline(&games, clock) } else { state.game.poll_baseline(&games, clock) };
                first = false;
                let batch = state.game.delivery_batch();
                host.check_transition(owner)?;
                let published = batch.is_some();
                if let Some(batch) = batch { resources.publish_profile_game_delivery(batch)?; }
                if sample.context_lost {
                    return Err(format!("The verified game context changed during baseline acquisition. {}", sample.diagnostics.join(" ")));
                }
                if Instant::now() >= deadline {
                    return Err("The game provider did not produce an advancing fresh baseline before activation.".into());
                }
                if sample.fresh && !published { return Ok(sample.frames); }
                // Publication may outlast telemetry freshness. Reacquire the
                // complete baseline without redating previously decoded bytes.
                if published { first = true; }
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    }
}

#[cfg(test)]
#[path = "activation_heartbeat_tests.rs"]
mod tests;

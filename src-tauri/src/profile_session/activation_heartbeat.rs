//! Renew only prepared outputs while synchronous activation work owns the caller.
//! The scoped pump cannot outlive that work or renew beyond its epoch/deadline.
use super::*;
use osg_streams::NativeWake;
use std::{sync::atomic::AtomicBool, time::Duration};

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
    wake: &NativeWake,
    mut heartbeat: impl FnMut() -> Result<Instant, String> + Send,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    permitted(epoch, expected, deadline)?;
    let mut observed = wake.observed();
    let mut next_renewal = heartbeat()?;
    permitted(epoch, expected, deadline)?;
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let pump = scope.spawn(|| {
            wake.bind_thread(std::thread::current()).map_err(|_| "Cannot bind activation heartbeat.")?;
            loop {
                if finished.load(Ordering::Acquire) { return Ok::<(), String>(()); }
                permitted(epoch, expected, deadline)?;
                let current = wake.observed();
                let now = Instant::now();
                if observed != current || now >= next_renewal {
                    observed = current;
                    next_renewal = heartbeat()?;
                } else {
                    wake.wait(observed, deadline.min(next_renewal).saturating_duration_since(now));
                }
            }
        });
        // Keep catalogue reads/publication on this caller. Joining the pump
        // before returning also prevents a cancelled publisher from running
        // after compensation or replacing a newer run's catalogue.
        let stop = StopHeartbeat { finished: &finished, wake };
        let result = work();
        drop(stop);
        pump.join().map_err(|_| "Profile activation heartbeat worker panicked.".to_string())??;
        permitted(epoch, expected, deadline)?;
        result
    })
}

struct StopHeartbeat<'a> {
    finished: &'a AtomicBool,
    wake: &'a NativeWake,
}
impl Drop for StopHeartbeat<'_> {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Release);
        self.wake.notify();
    }
}

impl NativeProfileHost {
    pub(super) fn install_authority(&mut self, owner: Option<&RunOwner>) -> Result<(), String> {
        let context = self.effective_context_owner(owner)?;
        let previous_context = self.effective_context_owner(self.authority.as_ref())?;
        let retained_child = owner.is_none() && self.prepared.values().any(|run| run.child_game_owner.is_some());
        if self.authority.as_ref() == owner && !retained_child { return Ok(()); }
        if let Some(previous) = &self.authority {
            if let Some(state) = self.prepared.get_mut(&key(previous)) { state.game.stop_commands(); }
        }
        if let (Some(signal), Some(previous)) = (&self.sample_signal, &self.authority) { signal.retire(previous); }
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
        let admission = self.games.read_model_signal();
        let host_wake = self.native_wake.as_ref().map(Arc::downgrade);
        let state = self.run(owner)?;
        if state.reducer.is_some() {
            return Err("Only prepared neutral outputs may use the activation heartbeat.".into());
        }
        if include_desktop && state.desktop.as_ref().is_some_and(|desktop| desktop.started()) {
            return Err("A started desktop output cannot use the activation heartbeat.".into());
        }
        let expected = state.admission_epoch;
        let deadline = Instant::now() + timeout;
        let mut output = state.output.take();
        let desktop = if include_desktop { state.desktop.take() } else { None };
        let result = if let Some(output) = output.as_mut() {
            let wake = Arc::new(NativeWake::default());
            admission.watch_activation_heartbeat(&wake);
            let fault_wake = Arc::downgrade(&wake);
            output.watch_fault(Arc::new(move || {
                if let Some(wake) = fault_wake.upgrade() { wake.notify(); }
                if let Some(wake) = host_wake.as_ref().and_then(|wake| wake.upgrade()) { wake.notify(); }
            }));
            during(&epoch, expected, deadline, &wake, || {
                output.keep_alive().map_err(|error| format!("Virtual output could not stay neutral during profile activation. {error}"))?;
                output.renewal_deadline().ok_or_else(|| "The prepared virtual output lease is unavailable.".into())
            }, || work(self))
        } else {
            // Prepared desktop scopes have no registration to renew before Start.
            (|| {
                permitted(&epoch, expected, deadline)?;
                let result = work(self);
                permitted(&epoch, expected, deadline)?;
                result
            })()
        };
        let state = self.run(owner)?;
        state.output = output;
        if include_desktop { state.desktop = desktop; }
        result
    }

    pub(super) fn game_baseline(&mut self, owner: &RunOwner) -> Result<Vec<osg_mapping::engine::typed::native::Admission>, String> {
        if !self.run(owner)?.game.requires_baseline() { return Ok(vec![]); }
        self.report_progress(owner, "game-telemetry-baseline", "Waiting for fresh game telemetry");
        self.with_activation_heartbeat(owner, |host| {
            let games = host.games.clone();
            let resources = host.resources.clone();
            let controllers = host.controllers.clone();
            let deadline = Instant::now() + Duration::from_secs(3);
            let wake = Arc::new(osg_streams::NativeWake::default());
            wake.bind_thread(std::thread::current())
                .map_err(|_| "Cannot bind the native game baseline waiter.")?;
            games.read_model_signal().watch_profile_admission(&wake);
            let game = &mut host.run(owner)?.game;
            let previous_wake = game.notification_wake();
            game.set_wake(Some(wake.clone()));
            let result = (|| {
                let mut first = true;
                loop {
                    host.check_transition(owner)?;
                    let revision = wake.observed();
                    let state = host.run(owner)?;
                    let clock = || controllers.stream_page("", 0).captured_us;
                    let sample = if first { state.game.baseline(&games, clock) } else { state.game.poll_baseline(&games, clock) };
                    first = false;
                    let batch = state.game.delivery_batch();
                    let wait_until = state.game.baseline_wait_until(deadline);
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
                    if published {
                        first = true;
                        continue;
                    }
                    while wake.observed() == revision {
                        host.check_transition(owner)?;
                        let remaining = wait_until.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        wake.wait(revision, remaining);
                    }
                }
            })();
            host.run(owner)?.game.set_wake(previous_wake);
            result
        })
    }
}

#[cfg(test)]
#[path = "activation_heartbeat_tests.rs"]
mod tests;

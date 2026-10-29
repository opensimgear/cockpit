//! One owned inventory writer. Live polling only queues/coalesces immutable
//! reader batches; inventory adoption and withdrawal run off the heartbeat.
use osg_profiles::resources::ProviderBatch;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc};

type Identity = (String, String);
enum Change {
    Publish(ProviderBatch),
    Replace { batch: ProviderBatch, previous: Option<String> },
    Withdraw(Identity),
}
impl Change {
    fn identity(&self) -> Identity {
        match self {
            Self::Publish(batch) | Self::Replace { batch, .. } => (batch.scope.trim_start_matches("profile-game:").into(), batch.session.clone()),
            Self::Withdraw(identity) => identity.clone(),
        }
    }
    fn publishing(&self) -> bool { !matches!(self, Self::Withdraw(_)) }
    fn replacing(&self) -> bool { matches!(self, Self::Replace { .. }) }
}
struct Pending {
    identity: Identity,
    publishing: bool,
    replacing: bool,
    cancelled: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<bool, String>>,
}
#[derive(Default)]
pub(super) struct Publication {
    pending: Option<Pending>,
    queued: Option<Change>,
    pub(super) deferred: bool,
    withdrawn: Option<Identity>,
}
impl Drop for Publication {
    fn drop(&mut self) { self.cancel(); }
}
impl Publication {
    pub(super) fn cancel(&mut self) {
        if let Some(pending) = &self.pending { pending.cancelled.store(true, Ordering::Release); }
        self.queued = None;
        self.deferred = false;
        self.withdrawn = None;
        // Retain a cancelled pending slot until that exact worker exits.
    }
    pub(super) fn publish(&mut self, batch: ProviderBatch) { self.queued = Some(Change::Publish(batch)); }
    pub(super) fn replace(&mut self, batch: ProviderBatch, previous: Option<String>) {
        self.cancel();
        self.queued = Some(Change::Replace { batch, previous });
    }
    pub(super) fn claiming(&self, identity: &Identity) -> bool {
        self.queued.as_ref().is_some_and(|change| change.replacing() && change.identity() == *identity)
            || self.pending.as_ref().is_some_and(|pending| pending.replacing && pending.identity == *identity
                && !pending.cancelled.load(Ordering::Acquire))
    }
    pub(super) fn withdraw(&mut self, identity: Identity) {
        if self.withdrawn.as_ref() == Some(&identity)
            || self.queued.as_ref().is_some_and(|change| !change.publishing() && change.identity() == identity)
            || self.pending.as_ref().is_some_and(|pending| !pending.publishing && pending.identity == identity) { return; }
        self.cancel();
        self.queued = Some(Change::Withdraw(identity));
    }
    pub(super) fn waiting(&self, identity: &Identity) -> bool {
        self.queued.as_ref().is_some_and(|change| change.publishing() && change.identity() == *identity)
            || self.pending.as_ref().is_some_and(|pending| pending.publishing && pending.identity == *identity
                && !pending.cancelled.load(Ordering::Acquire))
    }
    pub(super) fn poll(&mut self, current: Option<&Identity>) -> Option<Result<bool, String>> {
        let pending = self.pending.as_ref()?;
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("The native game inventory worker ended before reporting its result.".into()),
        };
        let pending = self.pending.take().unwrap();
        if !pending.publishing && !pending.cancelled.load(Ordering::Acquire) && matches!(result, Ok(true)) {
            self.withdrawn = Some(pending.identity.clone());
        }
        (!pending.cancelled.load(Ordering::Acquire) && current == Some(&pending.identity)).then_some(result)
    }
    pub(super) fn drive(&mut self, resources: Arc<crate::profile_resources::Runtime>) -> Result<(), String> {
        self.start(move |change, cancelled| match change {
            Change::Publish(batch) => resources.publish_current_profile_game_delivery(batch,
                || !cancelled.load(Ordering::Acquire)),
            Change::Replace { batch, previous } => resources.replace_profile_game_delivery(batch, previous.as_deref(),
                || !cancelled.load(Ordering::Acquire)),
            Change::Withdraw((game, reader)) => {
                if cancelled.load(Ordering::Acquire) { return Ok(false); }
                resources.withdraw_profile_game_delivery(&game, &reader).map(|_| true)
            }
        })
    }
    fn start(&mut self, work: impl FnOnce(Change, Arc<AtomicBool>) -> Result<bool, String> + Send + 'static) -> Result<(), String> {
        if self.pending.is_some() { return Ok(()); }
        let Some(change) = self.queued.take() else { return Ok(()); };
        let identity = change.identity();
        let publishing = change.publishing();
        let replacing = change.replacing();
        let cancelled = Arc::new(AtomicBool::new(false));
        let captured = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().name("profile-game-inventory".into()).spawn(move || {
            let _ = sender.send(work(change, captured));
        }).map_err(|error| format!("Cannot start the native game inventory worker: {error}"))?;
        self.pending = Some(Pending { identity, publishing, replacing, cancelled, receiver });
        Ok(())
    }
}

pub(super) fn resume_batch(game: &mut crate::profile_session_games::GameSources,
    games: &crate::games::GamesRuntime, now_us: u64) -> Result<Option<ProviderBatch>, String> {
    if !game.requires_baseline() { return Ok(None); }
    let started = std::time::Instant::now();
    let clock = || now_us.saturating_add(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
    let mut first = true;
    loop {
        let sample = if first { game.baseline(games, clock) } else { game.poll_baseline(games, clock) };
        first = false;
        if sample.context_lost { return Err(sample.diagnostics.join("; ")); }
        if sample.fresh {
            let batch = game.delivery_batch().ok_or("The returning game has no complete reader publication.")?;
            // Handoff consumes fresh telemetry again on the owner. The worker's
            // read can install metadata, never replay frames or grant commands.
            game.pending(clock(), "Waiting for returning reader publication.".into());
            return Ok(Some(batch));
        }
        if started.elapsed() >= std::time::Duration::from_secs(3) {
            return Err("The returning game did not supply a fresh baseline before reader admission.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(test)]
#[path = "game_publication_tests.rs"]
mod tests;

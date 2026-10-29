//! One coalescing feedback worker. Slow metadata validation never owns the
//! mapping heartbeat; cancelled work cannot publish through a replacement peer.
use super::*;
use std::{
    sync::{atomic::{AtomicBool, Ordering}, mpsc},
    time::{Duration, Instant},
};

const MAX_AGE: Duration = Duration::from_millis(250);
pub(super) fn revocation() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }

pub(super) struct Worker<T> {
    pending: Option<mpsc::Receiver<T>>,
    cancelled: Arc<AtomicBool>,
}
impl<T> Default for Worker<T> {
    fn default() -> Self { Self { pending: None, cancelled: revocation() } }
}
impl<T> Drop for Worker<T> {
    fn drop(&mut self) { self.cancelled.store(true, Ordering::Release); }
}
impl<T: Send + 'static> Worker<T> {
    pub(super) fn busy(&self) -> bool { self.pending.is_some() }
    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancelled = revocation();
        // Retain the pending slot until the blocked worker actually exits.
        // Repeated cancellation cannot create an unbounded set of threads.
    }
    fn poll(&mut self) -> Option<Result<T, String>> {
        match self.pending.as_ref()?.try_recv() {
            Ok(result) => { self.pending = None; Some(Ok(result)) }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Err("The companion feedback worker ended before reporting its result.".into()))
            }
        }
    }
    fn start(&mut self, work: impl FnOnce(Arc<AtomicBool>) -> T + Send + 'static) -> Result<bool, String> {
        if self.pending.is_some() { return Ok(false); }
        let cancelled = self.cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().name("profile-feedback".into()).spawn(move || {
            let _ = sender.send(work(cancelled));
        }).map_err(|error| format!("Cannot start the companion feedback worker: {error}"))?;
        self.pending = Some(receiver);
        Ok(true)
    }
}

struct Job {
    logical: String,
    generation: u64,
    run: RunToken,
    receipt: AssignmentReceipt,
    tokens: Vec<ResourceToken>,
    samples: Vec<FeedbackSample>,
    captured: Instant,
    cancelled: Arc<AtomicBool>,
}
pub(super) struct Completion {
    logical: String,
    generation: u64,
    run: RunToken,
    cancelled: Arc<AtomicBool>,
    result: Result<bool, String>,
}
impl Job {
    fn execute(self, hub: &Hub, validate: &impl Fn(&ResourceToken) -> Result<(), String>, worker: &AtomicBool) -> Completion {
        let current = || !worker.load(Ordering::Acquire) && !self.cancelled.load(Ordering::Acquire)
            && self.captured.elapsed() < MAX_AGE;
        let result = (|| {
            if !current() { return Ok(false); }
            // Capture the session/binding token before potentially slow native
            // capability validation. A later clear/reconnect invalidates it.
            let token = hub.profile_feedback_token(&self.receipt)?;
            if !current() { return Ok(false); }
            // NativeFeed generations count accepted mapping baselines; the
            // Hub token counts this wire session's protocol generation. They
            // advance independently across reconnects. Fence each in its own
            // domain instead of comparing their numeric values.
            let native_generation = self.generation.to_string();
            for resource in &self.tokens {
                if resource.provider_generation != native_generation {
                    return Err("The companion feedback native generation changed.".into());
                }
                validate(resource)?;
                if !current() { return Ok(false); }
            }
            Ok(hub.try_publish_profile_feedback(&self.receipt, &token, &self.samples,
                FeedbackMode::Snapshot, self.captured, MAX_AGE, current)?.is_some())
        })();
        Completion { logical: self.logical, generation: self.generation, run: self.run,
            cancelled: self.cancelled, result }
    }
}

impl Instance {
    pub(super) fn revoke_feedback(&mut self) {
        self.feedback_cancel.store(true, Ordering::Release);
        self.feedback_cancel = revocation();
    }
}

impl CompanionSources {
    pub(super) fn cancel_feedback(&mut self) {
        self.feedback_worker.cancel();
        for instance in &mut self.instances { instance.revoke_feedback(); }
    }

    pub(crate) fn publish_feedback_async(&mut self, hub: &Hub, phase: &Phase,
        resources: Arc<crate::profile_resources::Runtime>, now_us: u64) -> Vec<String> {
        self.publish_feedback_background(hub, phase, move |token| resources.validate_delivery(token).map(|_| ()), now_us)
    }

    pub(super) fn publish_feedback_background(&mut self, hub: &Hub, phase: &Phase,
        validate: impl Fn(&ResourceToken) -> Result<(), String> + Send + 'static, now_us: u64) -> Vec<String> {
        let mut diagnostics = vec![];
        if let Some(completed) = self.feedback_worker.poll() {
            match completed {
                Err(error) => diagnostics.push(error),
                Ok(completed) => for completion in completed {
                    if completion.cancelled.load(Ordering::Acquire) || self.run.as_ref() != Some(&completion.run) { continue; }
                    if let Some(instance) = self.instances.iter_mut().find(|instance|
                        instance.logical == completion.logical && instance.generation == completion.generation && instance.ready) {
                        match completion.result {
                            Ok(published) => { if published { instance.feedback_live = true; } }
                            Err(error) => {
                                // Never clear here: a failed worker may belong
                                // to an obsolete peer or feedback generation.
                                instance.feedback_live = false;
                                diagnostics.push(format!("{} feedback: {error}", instance.logical));
                            }
                        }
                    }
                },
            }
        }
        if self.stopped || !self.acquired { return diagnostics; }
        if !phase.active() || self.run.as_ref() != Some(&phase.run_token()) || phase.at_us().0 < now_us {
            self.cancel_feedback();
            diagnostics.push("The companion feedback reducer run is not armed by this native owner.".into());
            return diagnostics;
        }
        if self.feedback_worker.busy() { return diagnostics; }
        let mut jobs = vec![];
        for instance in &mut self.instances {
            if !instance.ready || instance.feedback_clear_pending || instance.outputs.is_empty()
                || instance.feedback_at.is_some_and(|last| now_us.saturating_sub(last) < 34_000) { continue; }
            let result = (|| {
                let receipt = instance.receipt.clone().ok_or("The companion feedback owner is missing.")?;
                let tokens = instance.outputs.iter().map(|output| output.token.clone()
                    .ok_or_else(|| "Bind current companion feedback delivery tokens first.".to_string()))
                    .collect::<Result<Vec<_>, _>>()?;
                let captured = Instant::now();
                let samples = instance.outputs.iter().map(|output| {
                    let observed = phase.inspected_sample(&output.from);
                    let sample = feedback_sample(output, observed.map(|(sample, _)| sample))?;
                    Ok(sample.aged(observed.and_then(|(_, at)| at)
                        .map_or(0, |at| now_us.saturating_sub(at.0) / 1000)))
                }).collect::<Result<Vec<_>, String>>()?;
                Ok::<_, String>(Job { logical: instance.logical.clone(), generation: instance.generation,
                    run: phase.run_token(), receipt, tokens, samples, captured, cancelled: instance.feedback_cancel.clone() })
            })();
            instance.feedback_at = Some(now_us);
            match result {
                Ok(job) => jobs.push(job),
                Err(error) => diagnostics.push(format!("{} feedback: {error}", instance.logical)),
            }
        }
        if !jobs.is_empty() {
            let hub = hub.clone();
            if let Err(error) = self.feedback_worker.start(move |cancelled| jobs.into_iter()
                .map(|job| job.execute(&hub, &validate, &cancelled)).collect()) { diagnostics.push(error); }
        }
        diagnostics
    }
}

#[cfg(test)]
#[path = "companion_feedback_tests.rs"]
mod tests;

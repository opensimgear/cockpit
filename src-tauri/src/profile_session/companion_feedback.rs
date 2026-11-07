//! One coalescing feedback worker. Slow metadata validation never owns the
//! mapping heartbeat; cancelled work cannot publish through a replacement peer.
use super::*;
use std::{
    sync::{atomic::{AtomicBool, Ordering}, mpsc},
    time::{Duration, Instant},
};
#[path = "companion_feedback/delivery.rs"]
mod delivery;
pub(super) use delivery::Delivery;

const MAX_AGE: Duration = Duration::from_millis(250);
const CONTENTION_BACKOFF: Duration = Duration::from_millis(2);
pub(super) const MIN_PUBLISH_INTERVAL: Duration = Duration::from_millis(34);
pub(super) fn revocation() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }

pub(super) struct Worker<T> {
    pending: Option<mpsc::Receiver<T>>,
    cancelled: Arc<AtomicBool>,
    wake: Option<Arc<osg_streams::NativeWake>>,
}
impl<T> Default for Worker<T> {
    fn default() -> Self { Self { pending: None, cancelled: revocation(), wake: None } }
}
struct CompletionWake(Option<Arc<osg_streams::NativeWake>>);
impl Drop for CompletionWake {
    fn drop(&mut self) { if let Some(wake) = &self.0 { wake.notify(); } }
}
impl<T> Drop for Worker<T> {
    fn drop(&mut self) { self.cancelled.store(true, Ordering::Release); }
}
impl<T: Send + 'static> Worker<T> {
    pub(super) fn set_wake(&mut self, wake: Option<Arc<osg_streams::NativeWake>>) { self.wake = wake; }
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
        let wake = self.wake.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().name("profile-feedback".into()).spawn(move || {
            // Declare the sender after the guard so unwinding disconnects the
            // channel before notifying a parked consumer.
            let _completion = CompletionWake(wake);
            let sender = sender;
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
    publication: delivery::Publication,
    cancelled: Arc<AtomicBool>,
}
pub(super) struct Completion {
    logical: String,
    generation: u64,
    run: RunToken,
    cancelled: Arc<AtomicBool>,
    result: Result<bool, String>,
    receipt: delivery::Receipt,
    published_at: Instant,
}
impl Job {
    fn execute(self, hub: &Hub, validate: &impl Fn(&ResourceToken) -> Result<(), String>, worker: &AtomicBool) -> Completion {
        let mut published_at = Instant::now();
        let current = || !worker.load(Ordering::Acquire) && !self.cancelled.load(Ordering::Acquire)
            && self.publication.captured.elapsed() < MAX_AGE;
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
            published_at = Instant::now();
            publish_with_contention_retry(current, || {
                hub.try_publish_profile_feedback(&self.receipt, &token, &self.publication.samples,
                    self.publication.mode, self.publication.captured, MAX_AGE, current)
            })
        })();
        Completion { logical: self.logical, generation: self.generation, run: self.run,
            cancelled: self.cancelled, result, receipt: self.publication.receipt, published_at }
    }
}

fn publish_with_contention_retry(current: impl Fn() -> bool,
    mut try_publish: impl FnMut() -> Result<Option<u64>, String>) -> Result<bool, String> {
    if try_publish()?.is_some() { return Ok(true); }
    if !current() { return Ok(false); }
    // A panel reader woken by the previous job can briefly own the Hub lock.
    // Retry once on this worker before the normal publication cooldown.
    std::thread::sleep(CONTENTION_BACKOFF);
    if !current() { return Ok(false); }
    Ok(try_publish()?.is_some())
}

impl Instance {
    pub(super) fn revoke_feedback(&mut self) {
        self.feedback_cancel.store(true, Ordering::Release);
        self.feedback_cancel = revocation();
        self.feedback_delivery.reset();
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
                        // Back off failed and expired attempts too; immediately
                        // retrying them would churn workers on every heartbeat.
                        instance.feedback_at = Some(Instant::now());
                        match completion.result {
                            Ok(published) => { if published {
                                instance.feedback_live = true;
                                instance.feedback_delivery.confirm(completion.receipt, completion.published_at);
                            } }
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
        // Replace one bounded pending projection even while the independent
        // worker is busy. A completion never clears newer captured state.
        for instance in &mut self.instances {
            if !instance.ready || instance.feedback_clear_pending || instance.outputs.is_empty() { continue; }
            match projection(instance, phase, now_us) {
                Ok(projection) => if let Err(error) = instance.feedback_delivery.update(projection) { diagnostics.push(error); },
                Err(error) => diagnostics.push(format!("{} feedback: {error}", instance.logical)),
            }
        }
        if self.feedback_worker.busy() { return diagnostics; }
        let mut jobs = vec![];
        for instance in &mut self.instances {
            if !instance.ready || instance.feedback_clear_pending || instance.outputs.is_empty()
                { continue; }
            let Some(publication) = instance.feedback_delivery.prepare(Instant::now(), instance.feedback_at) else { continue; };
            let result = (|| {
                let receipt = instance.receipt.clone().ok_or("The companion feedback owner is missing.")?;
                let tokens = instance.outputs.iter().filter(|output| publication.samples.iter().any(|sample|
                    sample.channel_id == output.binding.channel_id && sample.output_id == output.binding.output_id))
                    .map(|output| output.token.clone()
                    .ok_or_else(|| "Bind current companion feedback delivery tokens first.".to_string()))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok::<_, String>(Job { logical: instance.logical.clone(), generation: instance.generation,
                    run: phase.run_token(), receipt, tokens, publication, cancelled: instance.feedback_cancel.clone() })
            })();
            instance.feedback_at = Some(Instant::now());
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

fn projection(instance: &Instance, phase: &Phase, now_us: u64) -> Result<delivery::Projection, String> {
    let captured = Instant::now();
    let entries = instance.outputs.iter().map(|output| {
        let observed = phase.inspected_observation(&output.from);
        let timeless = observed.as_ref().is_some_and(|value| value.timeless);
        let age_us = if timeless { 0 } else { observed.as_ref().and_then(|value| value.observed_at)
            .map_or(0, |at| now_us.saturating_sub(at.0)) };
        let sample = feedback_sample(output, observed.as_ref().map(|value| value.sample))?.aged(age_us / 1000);
        let mut signature = sample.clone();
        signature.source_age_ms = 0;
        let stamp = delivery::Stamp { sample: signature,
            captured_us: (!timeless).then(|| observed.as_ref().and_then(|value| value.captured_at).map(|at| at.0)).flatten(),
            captured_sequence: if timeless { 0 } else { observed.as_ref().map_or(0, |value| value.captured_sequence.0) } };
        let age_due = (sample.status == FeedbackStatus::Fresh && !timeless).then(||
            captured + Duration::from_micros((osg_companion_service::FEEDBACK_STALE_MS * 1000).saturating_sub(age_us)));
        Ok(delivery::Entry { stamp, sample, age_due })
    }).collect::<Result<Vec<_>, String>>()?;
    Ok(delivery::Projection { entries, captured })
}

#[cfg(test)]
#[path = "companion_feedback_tests.rs"]
mod tests;

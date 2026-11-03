use super::*;
use super::usb_source::Clock;
use crate::module_usb::stream::{Batch, Capture, Snapshot};
use osg_streams::{Delivery, LossReason, NativeSubscription};
use std::panic::{catch_unwind, AssertUnwindSafe};

struct Worker {
    source: Source,
    capture: Arc<Capture>,
    reader: NativeSubscription<Snapshot, Batch>,
    running: bool,
    epoch: u64,
    failed: bool,
    clock: Clock,
}
impl Worker {
    fn reset(&mut self) -> Result<(), String> {
        self.source.generation = self.source.generation.checked_add(1).ok_or("USB panel generation exhausted.")?;
        self.source.cursor = None;
        self.reader = self.capture.subscribe()?;
        Ok(())
    }
    fn failure(&mut self, state: &mut Mailbox, message: &str, running: bool, admitted: bool, _now: Instant) {
        state.baselines.remove(&self.source.spec.logical);
        state.liveness.remove(&self.source.spec.logical);
        state.diagnostics.insert(self.source.spec.logical.clone(), message.chars().filter(|c| !c.is_control()).take(320).collect());
        if !self.failed {
            if running && admitted {
                // Loss is admitted now, but cannot move the source capture
                // clock beyond an unchanged fresh recovery baseline.
                if let Ok(gap) = self.source.frame(self.source.at, Reason::Gap, vec![]) {
                    if state.frames.len() >= MAX_QUEUE { let _ = state.overflow(&self.source.spec.logical); }
                    else { state.frames.push_back(gap); }
                }
            }
            self.source.cursor = None;
            match self.source.generation.checked_add(1) {
                Some(generation) => self.source.generation = generation,
                None => { state.blocked.insert(self.source.spec.logical.clone()); }
            }
            self.failed = true;
        }
        state.notify();
    }
    fn commit(&mut self, state: &mut Mailbox, frames: Vec<Frame>, baseline: bool,
        running: bool, admitted: bool, witnessed: Instant) -> Result<(), String>
    {
        state.diagnostics.remove(&self.source.spec.logical);
        state.liveness.insert(self.source.spec.logical.clone(),
            (self.source.generation, self.source.sequence, self.clock.at(witnessed)?));
        if baseline {
            if let Some(frame) = frames.first() { state.baselines.insert(self.source.spec.logical.clone(), frame.clone()); }
        }
        if running && admitted {
            if state.frames.len() + frames.len() > MAX_QUEUE {
                state.overflow(&self.source.spec.logical)?;
                return Ok(());
            }
            state.frames.extend(frames);
        }
        self.failed = false;
        state.notify();
        Ok(())
    }
    fn delivery(&mut self, delivery: Delivery<Snapshot, Batch>, state: &mut Mailbox,
        running: bool, admitted: bool, now: Instant) -> Result<bool, String>
    {
        match delivery {
            Delivery::Loss(loss) => {
                self.failure(state, "USB input continuity was lost; a fresh baseline is required.", running, admitted, now);
                if loss.reason == LossReason::Closed { return Ok(false); }
                if loss.reason == LossReason::Overflow { self.reader = self.capture.subscribe()?; }
            }
            Delivery::Snapshot(snapshot) => {
                let result = (|| {
                    let at = snapshot.value.received_at.ok_or_else(|| snapshot.value.error.clone()
                        .unwrap_or_else(|| "USB input baseline is pending.".into()))?;
                    let frame = self.source.usb_baseline(&snapshot.value, self.clock, now)?;
                    self.commit(state, vec![frame], true, running, admitted, at)
                })();
                if let Err(error) = result { self.failure(state, &error, running, admitted, now); }
            }
            Delivery::Event(event) => {
                let at = event.value.witnessed_at;
                if !running || !admitted || self.source.cursor.is_none() {
                    let Target::Usb { route, .. } = &self.source.spec.target else { unreachable!() };
                    let snapshot = Snapshot { route: route.clone(), stream: event.value.latest.stream,
                        state: Some(event.value.latest.state.clone()), received_at: Some(at), ready: true, error: None };
                    let frame = self.source.usb_baseline(&snapshot, self.clock, now)?;
                    self.commit(state, vec![frame], true, running, admitted, at)?;
                } else {
                    let frames = self.source.usb_batch(&event.value, self.clock, now)?;
                    self.commit(state, frames, false, running, admitted, at)?;
                }
            }
        }
        Ok(true)
    }
}

pub(super) fn run(spec: Spec, io: Arc<dyn Io>, mailbox: Arc<Mutex<Mailbox>>,
    revoked: Arc<AtomicBool>, active: Arc<AtomicBool>, admitted: Arc<AtomicBool>,
    started: Instant, clock_origin_us: u64, wake: Arc<NativeWake>)
{
    let logical = spec.logical.clone();
    let mut checkpoint = None;
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        let _ = wake.bind_thread(thread::current());
        let Target::Usb { key, route } = &spec.target else { unreachable!() };
        let capture = io.usb_capture(key)?;
        if capture.snapshot().route != *route { return Err("USB capture belongs to another native route.".into()); }
        // Registration precedes the atomic baseline subscription: no lost wake.
        let _watch = capture.watch(wake.clone())?;
        let reader = capture.subscribe()?;
        let source = Source { spec, cursor: None, held: vec![], sequence: 0, at: 0,
            retry: Instant::now(), generation: 1, sampled_at: None };
        let mut worker = Worker { source, capture, reader, running: false, epoch: 0,
            failed: false, clock: Clock { started, origin: clock_origin_us } };
        loop {
            let observed = wake.observed();
            let mut state = mailbox.lock().unwrap_or_else(|p| p.into_inner());
            if revoked.load(Ordering::Acquire) || state.blocked.contains(&logical) { return Ok(()); }
            checkpoint = Some((worker.source.generation, worker.source.sequence, worker.source.at));
            let running = active.load(Ordering::Acquire);
            let delivering = admitted.load(Ordering::Acquire);
            if running != worker.running || state.loss_epoch != worker.epoch {
                worker.running = running;
                worker.epoch = state.loss_epoch;
                worker.reset()?;
            }
            // Consume + publish under the common Host admission lock. No I/O or
            // global service lock is taken, so the accepted startup checkpoint
            // and subscription cursor cannot diverge during admission.
            match worker.reader.try_recv() {
                Ok(Some(delivery)) => {
                    match worker.delivery(delivery, &mut state, running, delivering, Instant::now()) {
                        Ok(true) => {}
                        Ok(false) => return Ok(()),
                        Err(error) => {
                            worker.failure(&mut state, &error, running, delivering, Instant::now());
                            // An atomic current baseline skips discarded stale
                            // edges; it never requests or resets the producer.
                            worker.reader = worker.capture.subscribe()?;
                        }
                    }
                    drop(state);
                    continue;
                }
                Err(error) => return Err(format!("USB input subscription closed: {error:?}")),
                Ok(None) => {}
            }
            drop(state);
            wake.wait_scheduled(observed, None);
        }
    }));
    let error = match outcome {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "USB profile input worker panicked.".into(),
    };
    if !revoked.load(Ordering::Acquire) {
        terminal_failure(&mut mailbox.lock().unwrap_or_else(|p| p.into_inner()), &logical, &error, checkpoint);
    }
}

#[cfg(test)]
#[path = "usb_clock_tests.rs"]
mod clock_tests;

pub(super) fn terminal_failure(state: &mut Mailbox, logical: &str, error: &str,
    checkpoint: Option<(u64,u64,u64)>)
{
    state.frames.retain(|frame| frame.source != logical);
    state.baselines.remove(logical);
    state.liveness.remove(logical);
    state.diagnostics.insert(logical.into(), error.chars().filter(|c| !c.is_control()).take(320).collect());
    if let Some((generation, sequence, captured_us)) = checkpoint.filter(|(_, sequence, _)| *sequence > 0) {
        if let Some(sequence) = sequence.checked_add(1) {
            if state.frames.len() >= MAX_QUEUE {
                // Only genuine shared queue exhaustion needs the existing
                // global loss fence; a known source failure stays isolated.
                let _ = state.overflow(logical);
            } else {
                state.frames.push_back(Frame { source: logical.into(), generation: Counter(generation),
                    sequence: Counter(sequence), captured_us: Counter(captured_us),
                    reason: Reason::Gap, changes: vec![] });
            }
        } else {
            state.blocked.insert(logical.into());
        }
    }
    state.notify();
}

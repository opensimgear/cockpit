//! Inventory contention suspends input authority without resetting the panel's
//! independently owned presentation stream. Real revocation keeps its clear.
use super::*;

impl Instance {
    fn live_tokens(&self, validate: &impl Fn(&ResourceToken) -> Result<Option<()>, String>) -> Result<bool, String> {
        let mut current = true;
        for input in &self.inputs {
            let token = input.token.as_ref().ok_or("Bind an actual fresh companion read token first.")?;
            current &= validate(token)?.is_some();
        }
        for output in &self.outputs {
            if let Some(token) = &output.token { current &= validate(token)?.is_some(); }
        }
        Ok(current)
    }
    fn pause_inputs(&mut self, now: u64) -> Result<Option<Frame>, String> {
        if self.inputs.is_empty() || self.metadata_paused { return Ok(None); }
        self.metadata_paused = true;
        self.at = self.at.max(now);
        Ok(Some(Frame { source: self.logical.clone(), generation: Counter(self.generation),
            sequence: Counter(self.next()?), captured_us: Counter(self.at), reason: Reason::Gap, changes: vec![] }))
    }
    fn pending_wire(&mut self) -> Result<(), String> {
        let frames = self.feed.as_mut().ok_or("The companion subscription is unavailable.")?.poll_audited()?;
        for incoming in frames {
            if incoming.frame.source.id != self.proof.instance_id()
                || incoming.frame.generation.0 != self.generation
                || matches!(incoming.frame.reason, Reason::Disconnect | Reason::Gap) {
                return Err("The companion writer or native generation changed while inventory validation was pending.".into());
            }
            // Discard unchecked transitions. Recovery captures the actual
            // current neutral state instead of replaying queued presses.
        }
        Ok(())
    }
    fn flush_feedback_clear(&mut self, hub: &Hub) {
        if !self.feedback_clear_pending { return; }
        self.feedback_clear_pending = self.receipt.as_ref().is_some_and(|receipt|
            matches!(hub.try_clear_profile_feedback(receipt), Ok(false)));
    }
    fn fail_live(&mut self, hub: &Hub, now: u64, was_ready: bool, error: String, poll: &mut CompanionPoll) {
        let clear = was_ready || self.ready || self.feedback_live;
        self.revoke_feedback();
        self.metadata_paused = false;
        if was_ready || self.ready { if let Ok(frame) = self.loss(now) { poll.frames.extend(frame); } }
        poll.diagnostics.push(format!("{}: {error}", self.logical));
        self.feedback_clear_pending |= clear;
        self.flush_feedback_clear(hub);
        self.feedback_live = false;
    }
}

impl CompanionSources {
    pub(crate) fn needs_rebind_live(&self,
        validate: impl Fn(&ResourceToken) -> Result<Option<()>, String>) -> bool {
        self.instances.iter().any(|instance| !instance.ready
            || instance.inputs.iter().any(|input| input.token.is_none())
            || instance.outputs.iter().any(|output| output.token.is_none())
            || instance.live_tokens(&validate).is_err())
    }

    pub(crate) fn poll_live(&mut self, hub: &Hub,
        validate: impl Fn(&ResourceToken) -> Result<Option<()>, String>, now: u64) -> CompanionPoll {
        let mut poll = CompanionPoll::default();
        if !self.acquired || self.stopped { return poll; }
        for instance in &mut self.instances {
            instance.flush_feedback_clear(hub);
            let was_ready = instance.ready;
            let outcome = (|| {
                instance.validate_binding_identity()?;
                let owner = hub.try_validate_profile_assignment(instance.receipt.as_ref().ok_or("The companion assignment is not acquired.")?)?;
                if !instance.live_tokens(&validate)? || owner.is_none() {
                    instance.pending_wire()?;
                    poll.frames.extend(instance.pause_inputs(now)?);
                    poll.diagnostics.push(format!("{}: Waiting for current companion inventory validation.", instance.logical));
                    return Ok(());
                }
                let frames = if instance.metadata_paused {
                    instance.pending_wire()?;
                    match instance.feed.as_mut().unwrap().try_neutral_baseline_audited()? {
                        Some(frames) => frames,
                        None => {
                            poll.diagnostics.push(format!("{}: Release companion controls to resume input after inventory validation.", instance.logical));
                            return Ok(());
                        }
                    }
                } else {
                    instance.feed.as_mut().ok_or("The companion subscription is unavailable.")?.poll_audited()?
                };
                let mut converted = vec![];
                for frame in frames {
                    if matches!(frame.frame.reason, Reason::Disconnect | Reason::Gap) {
                        return Err("The actual companion writer lost its native input lease.".into());
                    }
                    if let Some(frame) = instance.convert(frame, now)? { converted.push(frame); }
                }
                poll.frames.extend(converted);
                instance.metadata_paused = false;
                if instance.ready && !instance.inputs.is_empty() {
                    poll.liveness.push((instance.logical.clone(), Counter(instance.generation)));
                }
                Ok::<_, String>(())
            })();
            if let Err(error) = outcome {
                instance.fail_live(hub, now, was_ready, error, &mut poll);
            }
        }
        poll
    }

    pub(crate) fn rebind_live(&mut self, hub: &Hub, delivery: &ExecutionPlan,
        allowed: impl Fn(&str) -> bool, validate: impl Fn(&ResourceToken) -> Result<Option<()>, String>, now: u64) -> CompanionPoll {
        let mut poll = CompanionPoll::default();
        if !self.acquired || self.stopped { return poll; }
        for instance in &mut self.instances {
            if !allowed(&instance.logical) { continue; }
            instance.flush_feedback_clear(hub);
            let was_ready = instance.ready;
            let result = (|| {
                let receipt = instance.receipt.as_ref().ok_or("The companion assignment is not acquired.")?;
                if hub.try_validate_profile_assignment(receipt)?.is_none() { return Ok(None); }
                let (inputs, outputs) = instance.delivery_bindings(delivery)?;
                if outputs.iter().any(Option::is_none) { return Err("The companion feedback has no current write capability.".into()); }
                let mut current = true;
                for token in inputs.iter().chain(outputs.iter().flatten()) { current &= validate(token)?.is_some(); }
                if !current { return Ok(None); }
                let changed = instance.inputs.iter().map(|input| input.token.as_ref()).ne(inputs.iter().map(Some))
                    || instance.outputs.iter().map(|output| output.token.as_ref()).ne(outputs.iter().map(Option::as_ref));
                if instance.ready && !changed { return Ok(Some(vec![])); }
                instance.revoke_feedback();
                if instance.ready {
                    poll.frames.extend(instance.loss(now)?);
                    instance.feedback_clear_pending = true;
                    instance.flush_feedback_clear(hub);
                    instance.feedback_live = false;
                }
                instance.install_bindings(inputs, outputs);
                let frames = instance.feed.as_mut().ok_or("The companion subscription is unavailable.")?.baseline_audited()?;
                let mut converted = vec![];
                for frame in frames { if let Some(frame) = instance.convert(frame, now)? { converted.push(frame); } }
                instance.metadata_paused = false;
                Ok::<_, String>(Some(converted))
            })();
            match result {
                Ok(Some(frames)) => poll.frames.extend(frames),
                Ok(None) => {},
                Err(error) => instance.fail_live(hub, now, was_ready, error, &mut poll),
            }
        }
        poll
    }
}

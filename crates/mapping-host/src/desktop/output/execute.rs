use super::{Completion, Job, Shared};
use osg_desktop_output::{protocol::{Command, Request, ScopeAdmission, FRAME_LIMIT, MAX_BATCH}, Coordinator};
use osg_mapping::{desktop::{Delivery, Intent}, engine::{Counter, typed::Token}};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn cleanup(delivery: &Delivery) -> bool {
    matches!(&delivery.intent, Intent::Cleanup)
        || matches!(&delivery.intent, Intent::Held { controls } if controls.is_empty())
}
pub(super) fn guarded_duration(delivery: &Delivery, tap_durations: &BTreeMap<String, Counter>) -> Option<Counter> {
    matches!(&delivery.intent, Intent::Held { controls } if !controls.is_empty())
        .then(|| tap_durations.get(&delivery.action_id).copied())
        .flatten()
}
pub(in crate::desktop) fn same_pointer_batch(first: &Delivery, next: &Delivery) -> bool {
    matches!(first.intent, Intent::Pointer { .. })
        && matches!(next.intent, Intent::Pointer { .. })
        && first.at_us == next.at_us && first.producer.run == next.producer.run
}
fn envelope_size() -> Result<usize, String> {
    serde_json::to_vec(&Request { sequence: Counter(u64::MAX), deadline_us: Some(Counter(u64::MAX)),
        admission: Some(ScopeAdmission { registration: Counter(u64::MAX), epoch: Counter(u64::MAX) }),
        command: Command::DeliverPointerBatch { deliveries: vec![] } })
        .map(|bytes| bytes.len()).map_err(|error| error.to_string())
}
fn reason(shared: &Shared, job: &Job, armed: &BTreeMap<String, Token>, cancelled: &BTreeSet<String>, delivery: &Delivery) -> Option<u64> {
    if cleanup(delivery) && armed.get(&delivery.action_id) != Some(&delivery.producer) { return Some(1); }
    if !cleanup(delivery) && cancelled.contains(&delivery.action_id) { return Some(2); }
    let state = shared.lock();
    if state.closed || state.shutdown || state.pause.is_some() { return Some(4); }
    if !cleanup(delivery) && (state.epoch != job.epoch || state.fault.is_some()
        || !super::child::permitted(&state, job.admission)
        || !super::focus::permitted(&state) || !Shared::current(&state, delivery)) {
        return Some(4);
    }
    None
}
fn arm(output: &mut Coordinator, armed: &mut BTreeMap<String, Token>, delivery: &Delivery) -> Result<bool, String> {
    if !cleanup(delivery) && armed.get(&delivery.action_id) != Some(&delivery.producer) {
        if !output.arm_checked(delivery.producer.clone(), delivery.action_id.clone())? { return Ok(false); }
        armed.insert(delivery.action_id.clone(), delivery.producer.clone());
    }
    Ok(true)
}
fn source_now(job: &Job) -> u64 {
    effective_source_now(job.source_now, job.admitted)
}
pub(in crate::desktop) fn effective_source_now(source_now: u64, admitted: std::time::Instant) -> u64 {
    source_now.saturating_add(admitted.elapsed().as_micros().min(u64::MAX as u128) as u64)
}
fn record(completion: &mut Completion, cancelled: &mut BTreeSet<String>, delivery: Delivery, ack: osg_desktop_output::protocol::Ack) {
    if ack.skipped && matches!(&delivery.intent, Intent::Held { controls } if !controls.is_empty()) {
        cancelled.insert(delivery.action_id.clone());
    }
    completion.outcomes.push((delivery, ack));
}
pub(super) fn execute(output: &mut Coordinator, shared: &Shared, armed: &mut BTreeMap<String, Token>, tap_durations: &BTreeMap<String, Counter>, mut job: Job) -> Completion {
    output.set_admission(job.admission);
    let mut completion = Completion { audit_shadow: job.audit_shadow, outcomes: vec![], dropped: vec![], error: None };
    let mut cancelled = BTreeSet::new();
    let mut rejected = false;
    let mut pending: VecDeque<_> = std::mem::take(&mut job.deliveries).into();
    let result = (|| {
        while let Some(delivery) = pending.pop_front() {
            if rejected && !cleanup(&delivery) {
                completion.dropped.push((delivery, 4));
                continue;
            }
            if let Some(reason) = reason(shared, &job, armed, &cancelled, &delivery) {
                completion.dropped.push((delivery, reason));
                continue;
            }
            let mut group = vec![delivery];
            if matches!(group[0].intent, Intent::Pointer { .. }) {
                let mut size = envelope_size()? + serde_json::to_vec(&group[0]).map_err(|error| error.to_string())?.len();
                while group.len() < MAX_BATCH {
                    let Some(next) = pending.front() else { break };
                    if !same_pointer_batch(&group[0], next) || reason(shared, &job, armed, &cancelled, next).is_some() { break; }
                    let next_size = size.saturating_add(1).saturating_add(serde_json::to_vec(next).map_err(|error| error.to_string())?.len());
                    if next_size > FRAME_LIMIT { break; }
                    size = next_size;
                    group.push(pending.pop_front().expect("front was present"));
                }
            }
            // Arming can block on IPC/journal. The coordinator gate rechecks the
            // complete group afterwards, including producers already armed here.
            for delivery in &group {
                if reason(shared, &job, armed, &cancelled, delivery).is_some() { break; }
                if !arm(output, armed, delivery)? { rejected = true; break; }
            }
            if rejected || group.iter().any(|delivery| reason(shared, &job, armed, &cancelled, delivery).is_some()) {
                // A motion vector is one physical operation. Revoking any member
                // rejects the whole vector, including still-current members.
                completion.dropped.extend(group.into_iter().map(|delivery| (delivery, 4)));
                continue;
            }
            if group.len() > 1 {
                let acknowledgements = output.submit_pointer_batch(group.clone(), source_now(&job))?;
                if acknowledgements.len() != group.len() { return Err("Desktop pointer batch returned an incomplete acknowledgement list.".into()); }
                for (delivery, ack) in group.into_iter().zip(acknowledgements) {
                    rejected |= ack.scope_rejected;
                    record(&mut completion, &mut cancelled, delivery, ack);
                }
            } else {
                let delivery = group.pop().expect("one delivery");
                let ack = if let Some(release_after_us) = guarded_duration(&delivery, tap_durations) {
                    output.submit_guarded(delivery.clone(), source_now(&job), release_after_us)?
                } else {
                    output.submit(delivery.clone(), source_now(&job))?
                };
                rejected |= ack.scope_rejected;
                record(&mut completion, &mut cancelled, delivery, ack);
            }
        }
        Ok(())
    })();
    output.set_admission(None);
    completion.error = result.err();
    completion
}

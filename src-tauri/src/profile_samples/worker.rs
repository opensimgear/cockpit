use super::*;

struct Task { id: u64, plan: Arc<NodePlan>, publisher: Option<Publisher>, last: Option<Arc<CapturedSamples>> }

fn tasks(inner: &Inner) -> Vec<Task> {
    let (tasks, retired) = {
        let mut state = inner.lock();
        let idle = state.slots.iter().filter(|(_, slot)| slot.pins == 0
            && slot.publisher.as_ref().is_none_or(|publisher| !publisher.has_readers()))
            .map(|(topic, _)| topic.clone()).collect::<Vec<_>>();
        let retired = idle.into_iter().filter_map(|topic| state.slots.remove(&topic)
            .and_then(|slot| slot.publisher)).collect();
        inner.interested.store(state.slots.len(), Ordering::Release);
        let tasks = state.slots.values().filter(|slot| !matches!(slot.ready, Some(Err(_))))
            .map(|slot| Task { id: slot.id, plan: slot.plan.clone(), publisher: slot.publisher.clone(), last: slot.last.clone() }).collect();
        (tasks, retired)
    };
    inner.retire(retired);
    tasks
}
fn fail(inner: &Inner, task: &Task, error: String) {
    let publisher = {
        let mut state = inner.lock();
        state.slots.get_mut(&task.plan.topic).filter(|slot| slot.id == task.id).and_then(|slot| {
            slot.ready = Some(Err(error));
            slot.publisher.take()
        })
    };
    inner.changed.notify_all();
    inner.retire(publisher.into_iter().collect());
}
fn publish(inner: &Inner, broker: &Broker, task: &Task, value: CapturedSamples, clock: &str) {
    if !inner.current(&task.plan.topic, task.id) { return; }
    if task.last.as_ref().is_some_and(|old| old.samples == value.samples) { return; }
    let payload = match task.plan.encode(&value) { Ok(payload) => payload, Err(error) => { fail(inner, task, error); return; } };
    if !inner.current(&task.plan.topic, task.id) { return; }
    let result = if let Some(publisher) = &task.publisher {
        publisher.publish_latest_state(publisher.generation(), payload, value.captured_us, clock)
            .map(|_| publisher.clone())
    } else {
        broker.register(osg_streams::TopicSpec { topic: task.plan.topic.clone(), source: "profile-samples".into(),
            audiences: vec!["ui".into(), "browser".into(), "mcp".into()] }, payload, value.captured_us, clock.to_owned())
    };
    match result {
        Err(error) => fail(inner, task, format!("Profile sample publication failed: {error}")),
        Ok(publisher) => {
            let accepted = {
                let mut state = inner.lock();
                if let Some(slot) = state.slots.get_mut(&task.plan.topic)
                    .filter(|slot| slot.id == task.id && !matches!(slot.ready, Some(Err(_)))) {
                    slot.publisher = Some(publisher.clone());
                    slot.last = Some(Arc::new(value));
                    slot.ready = Some(Ok(()));
                    true
                } else { false }
            };
            if !accepted { broker.retire(&publisher); }
            inner.changed.notify_all();
        }
    }
}
pub(super) fn run(inner: Arc<Inner>, broker: Broker, capture: Arc<Capture>, clock: String) {
    loop {
        let observed = inner.wake.observed();
        if inner.closed.load(Ordering::Acquire) { break; }
        let tasks = tasks(&inner);
        if !tasks.is_empty() {
            // No sample-worker lock is held while acquiring the coordinator.
            let plans = tasks.iter().map(|task| task.plan.clone()).collect::<Vec<_>>();
            let values = capture(&plans);
            if values.len() != tasks.len() {
                for task in &tasks { fail(&inner, task, "Profile sample capture returned an invalid query batch.".into()); }
            } else {
                for (task, value) in tasks.iter().zip(values) {
                    match value { Ok(value) => publish(&inner, &broker, task, value, &clock), Err(error) => fail(&inner, task, error) }
                }
            }
        }
        if !inner.closed.load(Ordering::Acquire) { inner.wake.wait_scheduled(observed, None); }
    }
}

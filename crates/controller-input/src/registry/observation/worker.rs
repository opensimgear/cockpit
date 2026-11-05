use super::{
    queue::{Captured, Shared, Slot, Update, Work},
    *,
};
use osg_streams::{Publisher, RegistryError, TopicSpec};

struct Binding {
    slot: Arc<Slot>,
    publisher: Publisher,
    generation: u64,
    dirty: bool,
}
impl Drop for Binding {
    fn drop(&mut self) {
        self.slot.broker.retire(&self.publisher);
    }
}
pub(super) fn run(shared: &Shared) {
    let mut bindings = BTreeMap::<u64, Binding>::new();
    while let Some(work) = shared.next() {
        let result = process(shared, &mut bindings, &work);
        work.slot.complete(work.update.epoch, result.clone());
        if result.is_err() {
            bindings.remove(&work.slot.token);
            shared.remove(&work.slot);
        }
    }
    // Binding Drop withdraws exact registrations, including active readers.
}
fn process(
    shared: &Shared,
    bindings: &mut BTreeMap<u64, Binding>,
    work: &Work,
) -> Result<(), String> {
    if let Some(error) = &work.update.failure {
        return Err(error.clone());
    }
    let Some(source) = &work.update.source else {
        return Err("Controller was forgotten.".into());
    };
    let generation = source
        .generation
        .checked_add(1)
        .ok_or("Controller generation exhausted.")?;
    let clock = format!("controllers:{}", work.update.session);
    if !bindings.contains_key(&work.slot.token) {
        if !work.slot.pinned() {
            return Err("Controller observation preparation cancelled.".into());
        }
        if bindings.len() >= MAX_BINDINGS {
            prune(shared, bindings);
        }
        if bindings.len() >= MAX_BINDINGS {
            return Err("Controller observation limit reached.".into());
        }
        let snapshot = snapshot(&work.update, source);
        let spec = TopicSpec {
            topic: format!("controllers.input.{}", work.update.id),
            source: "controller-input".into(),
            audiences: osg_streams::KNOWN_AUDIENCES
                .iter()
                .map(|s| (*s).into())
                .collect(),
        };
        let publisher = match work.slot.broker.register(
            spec.clone(),
            snapshot.clone(),
            source.captured_us,
            clock.clone(),
        ) {
            Ok(publisher) => publisher,
            Err(RegistryError::TopicLimit) => {
                prune(shared, bindings);
                work.slot
                    .broker
                    .register(spec, snapshot.clone(), source.captured_us, clock.clone())
                    .map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
        let binding = Binding {
            slot: work.slot.clone(),
            publisher,
            generation: source.generation,
            dirty: false,
        };
        if generation > 1 {
            binding
                .publisher
                .replace_generation(generation, snapshot, source.captured_us, &clock)
                .map_err(|e| e.to_string())?;
        }
        bindings.insert(work.slot.token, binding);
        return Ok(());
    }
    let binding = bindings.get_mut(&work.slot.token).unwrap();
    if !work.slot.pinned() && !binding.publisher.has_readers() {
        // Typed capture is authoritative. The next pinned preparation carries
        // its own complete state, so an idle topic need not encode any reports.
        binding.dirty = true;
        return Ok(());
    }
    // Preparation fences wait behind every preceding metadata/report update.
    // A current active topic already has the correct atomic full baseline.
    if work.update.fence && !binding.dirty && !work.resync {
        return Ok(());
    }
    let snapshot = snapshot(&work.update, source);
    if binding.generation != source.generation {
        binding
            .publisher
            .replace_generation(generation, snapshot, source.captured_us, &clock)
            .map_err(|e| e.to_string())?;
        binding.generation = source.generation;
    } else if work.resync {
        binding
            .publisher
            .resynchronize(generation, snapshot, source.captured_us, &clock)
            .map_err(|e| e.to_string())?;
    } else if work.update.reason == "change" && source.ready() {
        // This UI preview topic carries the complete absolute controller state.
        // Coalesce unread preview samples so a slow renderer catches up to the
        // current level; ordered capture reports remain in the routing journal.
        binding
            .publisher
            .publish_latest_state(generation, snapshot, source.captured_us, &clock)
            .map_err(|e| e.to_string())?;
    } else {
        let values = if source.ready() {
            source.values.clone()
        } else {
            BTreeMap::new()
        };
        let change = json!({"session":work.update.session,"source":source.summary(),"reason":work.update.reason,
            "values":values,"cursor":source.cursor.to_string(),"capturedUs":source.captured_us.to_string()});
        binding
            .publisher
            .publish(generation, snapshot, change, source.captured_us, &clock)
            .map_err(|e| e.to_string())?;
    }
    binding.dirty = false;
    Ok(())
}
fn prune(shared: &Shared, bindings: &mut BTreeMap<u64, Binding>) {
    bindings.retain(|_, binding| {
        // Pinning and retirement compete by CAS. No broker operation holds the
        // capture queue mutex, and the actual idle test is atomic with subscribe.
        if !binding.slot.begin_retire() {
            return true;
        }
        if binding.slot.broker.retire_if_idle(&binding.publisher) {
            shared.remove(&binding.slot);
            binding
                .slot
                .complete(0, Err("Controller observation retired.".into()));
            false
        } else {
            binding.slot.cancel_retire();
            true
        }
    });
}
fn snapshot(update: &Update, source: &Captured) -> Value {
    let ready = source.ready();
    let snapshot = json!({"session":update.session,"source":source.summary(),"identity":source.saved.identity,
        "diagnostics":source.diagnostics,"controls":source.controls.iter().map(|control| {
            json!({"capability":control,"value":if ready {source.values.get(&control.id)} else {None}})
        }).collect::<Vec<_>>(),"offset":0,"total":source.controls.len(),"limit":MAX_CONTROLS,
        "cursor":source.cursor.to_string(),"capturedUs":source.captured_us.to_string()});
    #[cfg(test)]
    super::test_support::before_publication(&update.id);
    snapshot
}

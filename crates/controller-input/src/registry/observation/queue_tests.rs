use super::*;
use crate::tests::fixture;

fn shared() -> Shared {
    Shared {
        state: Mutex::new(State {
            slots: BTreeMap::new(),
            pending: VecDeque::new(),
            recovery: BTreeMap::new(),
            recovery_order: VecDeque::new(),
            recovery_turn: true,
            pending_bytes: 0,
            next: 0,
            stop: false,
        }),
        wake: Condvar::new(),
    }
}
fn source() -> Source {
    let mut registry = Registry::default();
    let id = registry.connect(fixture("typed-queue", None)).unwrap();
    registry.sources.remove(&id).unwrap()
}

#[test]
fn native_reports_reuse_immutable_descriptor_and_replace_it_at_generation_boundary() {
    let shared = shared();
    let mut source = source();
    let broker = Broker::new("descriptor-sharing".into()).unwrap();
    let slot = shared
        .prepare(&broker, &source.saved.id, "session", &source, 1)
        .unwrap();
    let initial = shared.next().unwrap().update.source.unwrap();
    source.sequence = 1;
    shared.capture(&source.saved.id, "change", |slot| {
        Update::new(
            &source.saved.id,
            "session",
            Some(&source),
            "change",
            2,
            false,
            slot,
        )
    });
    let report = shared.next().unwrap().update.source.unwrap();
    assert!(Arc::ptr_eq(&initial.controls, &report.controls));
    source.generation += 1;
    source.controls[1].max = 100;
    shared.capture(&source.saved.id, "metadata", |slot| {
        Update::new(
            &source.saved.id,
            "session",
            Some(&source),
            "metadata",
            3,
            false,
            slot,
        )
    });
    let replacement = shared.next().unwrap().update.source.unwrap();
    assert!(!Arc::ptr_eq(&report.controls, &replacement.controls));
    assert_eq!(report.controls[1].max, 32767);
    assert_eq!(replacement.controls[1].max, 100);
    slot.unpin();
}

#[test]
fn recovery_and_fifo_sources_get_fair_service_under_repeated_overflow() {
    let shared = shared();
    let mut source = source();
    let broker = Broker::new("fair-recovery".into()).unwrap();
    let mut slots = Vec::new();
    for index in 0..3 {
        source.saved.id = format!("source{index}");
        slots.push(
            shared
                .prepare(&broker, &source.saved.id, "session", &source, 1)
                .unwrap(),
        );
        shared.next().unwrap();
    }
    let work = |slot: &Arc<Slot>, epoch| {
        Update::new(
            &slot.id,
            "session",
            Some(&source),
            "change",
            epoch,
            false,
            slot,
        )
    };
    {
        let mut state = shared.state.lock().unwrap();
        state.overflow(slots[0].clone(), work(&slots[0], 2));
        state.overflow(slots[1].clone(), work(&slots[1], 2));
        state.enqueue(slots[2].clone(), work(&slots[2], 2));
    }
    assert_eq!(shared.next().unwrap().slot.token, slots[0].token);
    shared
        .state
        .lock()
        .unwrap()
        .overflow(slots[0].clone(), work(&slots[0], 3));
    assert_eq!(
        shared.next().unwrap().slot.token,
        slots[2].token,
        "ordered FIFO must not starve"
    );
    assert_eq!(
        shared.next().unwrap().slot.token,
        slots[1].token,
        "older recovery must not starve"
    );
    assert_eq!(shared.next().unwrap().slot.token, slots[0].token);
    for slot in slots {
        slot.unpin();
    }
}

#[test]
fn overflow_keeps_one_bounded_typed_recovery_and_no_covered_ordered_updates() {
    let shared = shared();
    let source = source();
    let broker = Broker::new("queue-bound".into()).unwrap();
    let slot = shared
        .prepare(&broker, &source.saved.id, "session", &source, 1)
        .unwrap();
    shared.next().unwrap();
    for epoch in 2..1000 {
        shared.capture(&source.saved.id, "change", |slot| {
            Update::new(
                &source.saved.id,
                "session",
                Some(&source),
                "change",
                epoch,
                false,
                slot,
            )
        });
    }
    let state = shared.state.lock().unwrap();
    assert!(state.pending.len() <= MAX_PENDING && state.pending_bytes <= MAX_PENDING_BYTES);
    assert_eq!(state.recovery.len(), 1);
    assert_eq!(state.recovery_order.len(), 1);
    assert!(state.pending.is_empty());
    assert_eq!(state.recovery[&slot.token].update.epoch, 999);
    assert!(state.recovery[&slot.token].resync);
    slot.unpin();
}

#[test]
fn unresolved_retirement_never_replaces_an_active_slot() {
    let shared = shared();
    let source = source();
    let broker = Broker::new("retirement-race".into()).unwrap();
    let slot = shared
        .prepare(&broker, &source.saved.id, "session", &source, 1)
        .unwrap();
    shared.next().unwrap();
    slot.unpin();
    assert!(slot.begin_retire());
    assert!(shared
        .prepare(&broker, &source.saved.id, "session", &source, 2)
        .is_err());
    assert!(Arc::ptr_eq(
        &shared.state.lock().unwrap().slots[&source.saved.id],
        &slot
    ));
    slot.cancel_retire();
    let following = shared
        .prepare(&broker, &source.saved.id, "session", &source, 3)
        .unwrap();
    assert!(Arc::ptr_eq(&slot, &following));
    following.unpin();
}

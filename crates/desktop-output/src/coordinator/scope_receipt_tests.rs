use super::*;
use super::test_support::*;
use std::sync::atomic::Ordering;

#[test]
fn malformed_vectors_are_rejected_atomically_without_observer_publication() {
    for fault in [Forgery::Missing, Forgery::Duplicate, Forgery::Unknown, Forgery::Regress,
        Forgery::EligibleWithoutEpoch, Forgery::ExpiryWithoutEpoch, Forgery::ExpiryEligible, Forgery::Budget] {
        let mut fixture = Fixture::two_runs();
        if matches!(fault, Forgery::Regress) {
            fixture.source.lose(fixture.registration(1));
            fixture.coordinator.renew(run(2)).unwrap();
        }
        fixture.updates.lock().unwrap().clear();
        let before = [fixture.coordinator.scope_receipt(&run(1)), fixture.coordinator.scope_receipt(&run(2))];
        *fixture.fault.lock().unwrap() = fault;
        assert!(fixture.coordinator.renew(run(2)).is_err(), "{fault:?}");
        assert!(fixture.coordinator.snapshot().uncertain, "{fault:?}");
        assert!(fixture.updates.lock().unwrap().is_empty(), "{fault:?}");
        assert_eq!([fixture.coordinator.scope_receipt(&run(1)), fixture.coordinator.scope_receipt(&run(2))], before, "{fault:?}");
    }
}

#[test]
fn initial_registration_requires_epoch_one_and_publishes_nothing_on_failure() {
    let mut fixture = Fixture::new();
    *fixture.fault.lock().unwrap() = Forgery::InitialEpoch;
    assert!(fixture.coordinator.activate(run(1), Scope::DesktopWide).is_err());
    assert!(fixture.coordinator.scope_receipt(&run(1)).is_none());
    assert!(fixture.updates.lock().unwrap().is_empty());
    assert!(fixture.coordinator.snapshot().uncertain);
}

#[test]
fn eligibility_loss_without_raw_source_epoch_delta_still_requires_a_worker_epoch() {
    let mut fixture = Fixture::two_runs();
    let registration = fixture.registration(1);
    fixture.source.evidence.lock().unwrap().get_mut(&registration).unwrap().eligible = false;
    fixture.coordinator.renew(run(2)).unwrap();
    let receipt = fixture.coordinator.scope_receipt(&run(1)).unwrap();
    assert_eq!(receipt.admission.epoch, Counter(2));
    assert!(!receipt.eligible && receipt.live);
    assert_eq!(fixture.updates.lock().unwrap().len(), 1);
}

#[test]
fn expired_tombstones_are_exact_members_once_then_retired_from_both_sides() {
    let mut fixture = Fixture::two_runs();
    fixture.coordinator.submit(down(1, 1), 0).unwrap();
    fixture.coordinator.submit(down(2, 1), 0).unwrap();
    fixture.now.store(LEASE_US, Ordering::Release);
    assert!(fixture.coordinator.renew(run(2)).is_err());
    let ack = fixture.acks.lock().unwrap().last().unwrap().clone();
    assert_eq!(ack.scopes.len(), 2);
    assert!(ack.scopes.iter().all(|scope| !scope.live && !scope.eligible && scope.admission.epoch == Counter(2)));
    assert!(fixture.coordinator.scope_receipt(&run(1)).is_none());
    assert!(fixture.coordinator.scope_receipt(&run(2)).is_none());
    assert!(fixture.coordinator.snapshot().held.is_empty());
    assert!(!fixture.coordinator.snapshot().uncertain);
    assert_eq!(fixture.source.retired.lock().unwrap().len(), 2);
    assert_eq!(fixture.updates.lock().unwrap().len(), 2);
    fixture.coordinator.pause(run(2)).unwrap();
    assert!(fixture.acks.lock().unwrap().last().unwrap().scopes.is_empty());
    assert_eq!(fixture.updates.lock().unwrap().len(), 2);
}

#[test]
fn unrelated_scope_loss_rebases_every_owner_projection() {
    for operation in ["held", "arm", "pause", "pointer", "renew"] {
        let mut fixture = Fixture::two_runs();
        fixture.coordinator.submit(down(1, 1), 0).unwrap();
        fixture.coordinator.submit(down(2, 1), 0).unwrap();
        fixture.source.lose(fixture.registration(1));
        match operation {
            "held" => { fixture.coordinator.submit(down(2, 2), 0).unwrap(); },
            "arm" => fixture.coordinator.arm(token(2), "action".into()).unwrap(),
            "pause" => fixture.coordinator.pause(run(2)).unwrap(),
            "pointer" => { fixture.coordinator.submit(delivery(2, 2, Intent::Pointer { dx: 1, dy: 0 }), 0).unwrap(); },
            "renew" => fixture.coordinator.renew(run(2)).unwrap(),
            _ => unreachable!(),
        }
        let expected = if matches!(operation, "arm" | "pause") { vec![] } else { vec![key(2)] };
        let snapshot = fixture.coordinator.snapshot();
        assert_eq!(snapshot.held, expected, "{operation}");
        assert_eq!(snapshot.possible_held, expected, "{operation}");
        assert!(snapshot.owners.iter().all(|owner| owner.producer.run != run(1)), "{operation}");
        assert_eq!(fixture.coordinator.reducer.held(), expected, "{operation}");
        assert!(!snapshot.uncertain, "{operation}");
    }
}

#[test]
fn native_guard_receipt_is_reconciled_before_same_ack_scope_pause() {
    let mut fixture = Fixture::two_runs();
    let ack = fixture.coordinator.submit_guarded(down(1, 1), 0, Counter(1_000)).unwrap();
    fixture.coordinator.submit(down(2, 1), 0).unwrap();
    fixture.source.lose(fixture.registration(1));
    fixture.now.store(1_000, Ordering::Release);
    fixture.coordinator.renew(run(2)).unwrap();
    let latest = fixture.acks.lock().unwrap().last().unwrap().clone();
    assert_eq!(latest.released_guards, [ack.sequence]);
    assert_eq!(fixture.coordinator.snapshot().held, [key(2)]);
    assert!(fixture.coordinator.guard_deliveries.is_empty());
    assert!(fixture.coordinator.snapshot().owners.iter().all(|owner| owner.producer.run == run(2)));
    assert_eq!(fixture.updates.lock().unwrap().len(), 1);
}

#[test]
fn immutable_override_proof_cannot_upgrade_after_a_new_receipt_or_mutate_new_owners() {
    let mut fixture = Fixture::two_runs();
    let old = fixture.coordinator.scope_receipt(&run(1)).unwrap().admission;
    fixture.coordinator.submit(down(1, 1), 0).unwrap();
    fixture.source.lose(fixture.registration(1));
    fixture.coordinator.renew(run(2)).unwrap();
    let current = fixture.coordinator.scope_receipt(&run(1)).unwrap().admission;
    assert_ne!(old, current);
    fixture.coordinator.arm(token(1), "action".into()).unwrap();
    fixture.coordinator.submit(down(1, 1), 0).unwrap();
    let before = fixture.source.events.lock().unwrap().len();
    fixture.coordinator.set_admission(Some(old));
    assert!(!fixture.coordinator.arm_checked(token(1), "action".into()).unwrap());
    for intent in [down(1, 2).intent, Intent::Pointer { dx: 1, dy: 0 }] {
        let ack = fixture.coordinator.submit(delivery(1, 2, intent), 0).unwrap();
        assert!(ack.ok && ack.skipped && ack.scope_rejected, "{ack:?}");
        assert_eq!(ack.held, [key(1)]);
        assert_eq!(fixture.coordinator.admission, Some(old));
    }
    assert_eq!(fixture.source.events.lock().unwrap().len(), before);
    assert_eq!(fixture.coordinator.reducer.held(), [key(1)]);
    fixture.coordinator.set_admission(None);
    // Reuse operation two: none of the stale packets consumed its fence.
    fixture.coordinator.submit(delivery(1, 2, Intent::Held { controls: vec![] }), 0).unwrap();
    assert!(fixture.coordinator.snapshot().held.is_empty());
}

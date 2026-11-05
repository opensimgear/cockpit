use super::*;
use osg_companion_service::{FeedbackProvenance, FeedbackSource, FeedbackValue};

fn entry(channel: &str, sequence: u64, status: FeedbackStatus) -> Entry {
    let sample = FeedbackSample {
        channel_id: channel.into(), output_id: format!("{channel}-out"),
        value: Some(FeedbackValue::Boolean(true)), status,
        provenance: FeedbackProvenance::GameConfirmed, source_kind: FeedbackSource::Simulator,
        source_age_ms: 0, unit: "boolean".into(), range: None,
    };
    Entry { stamp: Stamp { sample: sample.clone(), captured_us: Some(sequence * 1000),
        captured_sequence: sequence }, sample, age_due: None }
}

fn project(delivery: &mut Delivery, captured: Instant, entries: Vec<Entry>) {
    delivery.update(Projection { entries, captured }).unwrap();
}

fn accept(delivery: &mut Delivery, at: Instant) -> Publication {
    let publication = delivery.prepare(at, None).unwrap();
    delivery.confirm(publication.receipt.clone(), at);
    publication
}

#[test]
fn cooldown_is_anchored_to_attempt_and_queries_do_not_move_it() {
    let at = Instant::now();
    let mut delivery = Delivery::default();
    project(&mut delivery, at, vec![entry("a", 1, FeedbackStatus::Fresh)]);
    let due = at + COOLDOWN;
    assert_eq!(delivery.deadline(Some(at)), Some(due));
    assert_eq!(delivery.deadline(Some(at)), Some(due));
    assert!(delivery.prepare(due - Duration::from_nanos(1), Some(at)).is_none());
    assert_eq!(delivery.prepare(due, Some(at)).unwrap().mode, FeedbackMode::Snapshot);
}

#[test]
fn unchanged_unknown_or_stale_has_no_idle_publication_or_deadline() {
    let at = Instant::now();
    for status in [FeedbackStatus::Unknown, FeedbackStatus::Stale, FeedbackStatus::Disconnected] {
        let mut delivery = Delivery::default();
        project(&mut delivery, at, vec![entry("a", 1, status)]);
        accept(&mut delivery, at);
        project(&mut delivery, at + RENEWAL, vec![entry("a", 1, status)]);
        assert!(delivery.deadline(None).is_none());
        assert!(delivery.prepare(at + RENEWAL, None).is_none());
    }
}

#[test]
fn same_value_new_capture_is_dirty_but_observation_age_is_not() {
    let at = Instant::now();
    let mut delivery = Delivery::default();
    project(&mut delivery, at, vec![entry("a", 1, FeedbackStatus::Fresh)]);
    accept(&mut delivery, at);
    let mut observed = entry("a", 1, FeedbackStatus::Fresh);
    observed.sample.source_age_ms = 100;
    project(&mut delivery, at + COOLDOWN, vec![observed]);
    assert!(delivery.prepare(at + COOLDOWN, None).is_none());
    assert_eq!(delivery.deadline(None), Some(at + RENEWAL));
    let mut captured = entry("a", 1, FeedbackStatus::Fresh);
    captured.stamp.captured_us = Some(2000);
    project(&mut delivery, at + COOLDOWN, vec![captured]);
    assert_eq!(accept(&mut delivery, at + COOLDOWN).mode, FeedbackMode::Delta);
    project(&mut delivery, at + COOLDOWN, vec![entry("a", 2, FeedbackStatus::Fresh)]);
    assert!(delivery.prepare(at + COOLDOWN, None).is_some());
}

#[test]
fn pending_latest_replaces_and_old_receipt_does_not_acknowledge_new_capture() {
    let at = Instant::now();
    let mut delivery = Delivery::default();
    project(&mut delivery, at, vec![entry("a", 1, FeedbackStatus::Fresh)]);
    let old = delivery.prepare(at, None).unwrap();
    for sequence in 2..100 {
        project(&mut delivery, at, vec![entry("a", sequence, FeedbackStatus::Fresh)]);
    }
    delivery.confirm(old.receipt, at);
    let new = delivery.prepare(at, None).unwrap();
    assert_eq!(new.mode, FeedbackMode::Delta);
    assert_eq!(new.receipt.stamps[0].captured_sequence, 99);
    assert_eq!(delivery.latest.as_ref().unwrap().entries.len(), 1);
}

#[test]
fn channel_renewals_are_independent_of_other_channel_changes() {
    let at = Instant::now();
    let mut delivery = Delivery::default();
    project(&mut delivery, at, vec![entry("a", 1, FeedbackStatus::Fresh), entry("b", 1, FeedbackStatus::Fresh)]);
    accept(&mut delivery, at);
    let later = at + Duration::from_millis(700);
    project(&mut delivery, later, vec![entry("a", 2, FeedbackStatus::Fresh), entry("b", 1, FeedbackStatus::Fresh)]);
    let changed = accept(&mut delivery, later);
    assert_eq!(changed.samples.len(), 1);
    assert_eq!(changed.samples[0].channel_id, "a");
    assert_eq!(delivery.deadline(None), Some(at + RENEWAL));
    let renewed = accept(&mut delivery, at + RENEWAL);
    assert_eq!(renewed.samples.len(), 1);
    assert_eq!(renewed.samples[0].channel_id, "b");
    assert_eq!(delivery.deadline(None), Some(later + RENEWAL));
}

#[test]
fn age_deadline_wakes_for_recapture_without_republishing_unchanged_sample() {
    let at = Instant::now();
    let due = at + Duration::from_millis(200);
    let mut delivery = Delivery::default();
    let mut fresh = entry("a", 1, FeedbackStatus::Fresh);
    fresh.age_due = Some(due);
    project(&mut delivery, at, vec![fresh]);
    accept(&mut delivery, at);
    assert_eq!(delivery.deadline(None), Some(due));
    assert!(delivery.prepare(due, None).is_none());
    project(&mut delivery, due, vec![entry("a", 1, FeedbackStatus::Stale)]);
    let stale = accept(&mut delivery, due);
    assert_eq!(stale.mode, FeedbackMode::Delta);
    assert_eq!(stale.samples[0].status, FeedbackStatus::Stale);
    assert!(delivery.deadline(None).is_none());
}

#[test]
fn snapshot_quality_transition_and_reset_restore_initial_snapshot() {
    let at = Instant::now();
    let mut delivery = Delivery::default();
    project(&mut delivery, at, vec![entry("a", 1, FeedbackStatus::Unknown)]);
    let initial = accept(&mut delivery, at);
    assert_eq!(initial.mode, FeedbackMode::Snapshot);
    assert_eq!(initial.captured, at);
    project(&mut delivery, at, vec![entry("a", 1, FeedbackStatus::Fresh)]);
    assert_eq!(accept(&mut delivery, at).mode, FeedbackMode::Delta);
    delivery.reset();
    assert!(delivery.deadline(None).is_none());
    delivery.confirm(initial.receipt, at);
    project(&mut delivery, at, vec![entry("a", 1, FeedbackStatus::Fresh)]);
    assert_eq!(accept(&mut delivery, at).mode, FeedbackMode::Snapshot);
}

#[test]
fn channel_budget_and_replaced_channels_keep_state_bounded() {
    let at = Instant::now();
    let mut delivery = Delivery::default();
    project(&mut delivery, at, vec![entry("old", 1, FeedbackStatus::Unknown)]);
    let old = delivery.prepare(at, None).unwrap();
    let oversized = Projection { captured: at, entries: (0..65)
        .map(|i| entry(&i.to_string(), 1, FeedbackStatus::Unknown)).collect() };
    assert!(delivery.update(oversized).is_err());
    assert_eq!(delivery.latest.as_ref().unwrap().entries[0].sample.channel_id, "old");
    project(&mut delivery, at, vec![entry("new", 1, FeedbackStatus::Unknown)]);
    delivery.confirm(old.receipt, at);
    assert!(delivery.confirmed.is_empty());
    accept(&mut delivery, at);
    assert_eq!(delivery.confirmed.len(), 1);
    let duplicate = Projection { captured: at, entries: vec![entry("x", 1, FeedbackStatus::Unknown),
        entry("x", 2, FeedbackStatus::Unknown)] };
    assert!(delivery.update(duplicate).is_err());
}

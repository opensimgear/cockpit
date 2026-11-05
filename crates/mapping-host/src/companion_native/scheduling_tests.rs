use super::*;
use crate::companions::{
    CompanionProvider, ControlDefinition, Definition, InputToken, Kind, NativeSelection, Value,
    LEASE_US,
};
use osg_mapping::engine::Reason;
use std::sync::atomic::{AtomicU64, Ordering};

fn definition(id: &str, publication: char) -> Definition {
    Definition {
        source_id: id.into(),
        name: "Native companion".into(),
        publication_hash: format!("sha256:{}", publication.to_string().repeat(64)),
        controls: vec![ControlDefinition {
            id: "button".into(),
            kind: Kind::Button,
            fallback: Value::Button(false),
        }],
    }
}

fn token(id: &str, publication: char, generation: u64) -> InputToken {
    InputToken {
        source_id: id.into(),
        session_id: format!("session-{id}"),
        generation,
        publication_hash: definition(id, publication).publication_hash,
    }
}

fn ready_feed(clock: Arc<AtomicU64>, id: &str) -> (CompanionProvider, NativeFeed, InputToken) {
    let read_clock = clock.clone();
    let provider = CompanionProvider::new(Arc::new(move || read_clock.load(Ordering::Acquire)));
    provider.register_definition(definition(id, 'a')).unwrap();
    let token = token(id, 'a', 1);
    provider
        .baseline_native(
            &token,
            0,
            &[osg_mapping::engine::Change {
                control: "button".into(),
                value: Value::Button(false),
            }],
        )
        .unwrap();
    let mut feed = provider
        .subscribe_native(&[NativeSelection {
            definition: definition(id, 'a'),
            controls: vec!["button".into()],
        }])
        .unwrap();
    feed.baseline().unwrap();
    (provider, feed, token)
}

#[test]
fn notification_driven_reflects_only_provider_wake_capability() {
    let clock = Arc::new(AtomicU64::new(100));
    let (_provider, feed, _) = ready_feed(clock.clone(), "left");
    assert!(!feed.notification_driven());
    drop(feed);

    let read_clock = clock;
    let provider = CompanionProvider::with_wake(Arc::new(move || read_clock.load(Ordering::Acquire)), Arc::new(|| {}));
    provider.register_definition(definition("left", 'a')).unwrap();
    let token = token("left", 'a', 1);
    provider
        .baseline_native(
            &token,
            0,
            &[osg_mapping::engine::Change {
                control: "button".into(),
                value: Value::Button(false),
            }],
        )
        .unwrap();
    let feed = provider
        .subscribe_native(&[NativeSelection {
            definition: definition("left", 'a'),
            controls: vec!["button".into()],
        }])
        .unwrap();
    assert!(feed.notification_driven());
}

#[test]
fn deadline_tracks_only_its_subscription_and_latest_heartbeat() {
    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut left, left_token) = ready_feed(clock.clone(), "left");
    assert_eq!(left.next_deadline_us(), Some(100 + LEASE_US));

    clock.store(200, Ordering::Release);
    provider.heartbeat_native(&left_token).unwrap();
    assert_eq!(left.next_deadline_us(), Some(200), "heartbeat needs a current-state renewal");
    assert_eq!(left.poll().unwrap()[0].reason, Reason::Change);
    assert_eq!(left.next_deadline_us(), Some(200 + LEASE_US));

    provider.register_definition(definition("right", 'a')).unwrap();
    let right_token = token("right", 'a', 1);
    provider
        .baseline_native(
            &right_token,
            0,
            &[osg_mapping::engine::Change {
                control: "button".into(),
                value: Value::Button(false),
            }],
        )
        .unwrap();
    clock.store(300, Ordering::Release);
    provider.heartbeat_native(&right_token).unwrap();
    assert_eq!(left.next_deadline_us(), Some(200 + LEASE_US));
}

#[test]
fn pending_queue_keeps_immediate_deadline_until_all_ordered_pages_are_drained() {
    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, token) = ready_feed(clock.clone(), "left");
    for sequence in 1..=65 {
        provider
            .changes_native(
                &token,
                sequence,
                &[osg_mapping::engine::Change {
                    control: "button".into(),
                    value: Value::Button(sequence % 2 == 1),
                }],
            )
            .unwrap();
    }
    assert_eq!(feed.next_deadline_us(), Some(100));
    assert_eq!(feed.poll().unwrap().len(), 64);
    assert_eq!(feed.next_deadline_us(), Some(100), "one queued transition remains");
    let tail = feed.poll().unwrap();
    assert!(tail.iter().any(|frame| frame.sequence.0 == 66));
    assert!(feed.next_deadline_us().is_some(), "the current-state renewal or lease remains due");
}

#[test]
fn lease_query_does_not_renew_or_expire_the_owner() {
    let clock = Arc::new(AtomicU64::new(100));
    let (_provider, mut feed, _) = ready_feed(clock.clone(), "left");
    clock.store(100 + LEASE_US - 1, Ordering::Release);
    assert_eq!(feed.next_deadline_us(), Some(100 + LEASE_US));
    assert_eq!(feed.next_deadline_us(), Some(100 + LEASE_US));
    clock.store(100 + LEASE_US, Ordering::Release);
    assert_eq!(feed.next_deadline_us(), Some(100 + LEASE_US));
    let loss = feed.poll().unwrap();
    assert_eq!(loss.len(), 1);
    assert_eq!(loss[0].reason, Reason::Disconnect);
}

#[test]
fn removed_instance_returns_queued_loss_without_indexing_or_forging_baseline() {
    use osg_companion_service::SourceSink;

    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, _) = ready_feed(clock, "left");
    provider.unregister("left").unwrap();
    assert!(feed.next_deadline_us().is_some());
    let frames = feed.poll().unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].reason, Reason::Disconnect);
    assert!(feed.poll().is_err(), "a removed source cannot restart without a fresh descriptor");
    assert_eq!(feed.next_deadline_us(), None, "removed source has no immediate work after its error");
}

#[test]
fn publication_replacement_is_immediately_due_as_a_loss() {
    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, _) = ready_feed(clock, "left");
    provider.register_definition(definition("left", 'b')).unwrap();
    assert_eq!(feed.next_deadline_us(), Some(100));
    let frames = feed.poll().unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].reason, Reason::Disconnect);
}

#[test]
fn lease_deadline_overflow_is_due_immediately() {
    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, _) = ready_feed(clock.clone(), "left");
    clock.store(u64::MAX - 1, Ordering::Release);
    provider.state.lock().unwrap().instances.get_mut("left").unwrap().last_seen_us = u64::MAX - 1;
    assert_eq!(feed.next_deadline_us(), Some(u64::MAX - 1));
    assert!(feed.poll_audited().unwrap_err().contains("clock exhausted"));
    assert_eq!(feed.next_deadline_us(), Some(u64::MAX - 1), "queued loss is drained next");
    let loss = feed.poll().unwrap();
    assert_eq!(loss.len(), 1);
    assert_eq!(loss[0].reason, Reason::Disconnect);
    assert_eq!(feed.next_deadline_us(), None, "withdrawn owner does not spin");
}

#[test]
fn final_baseline_acceptance_rejects_unregister_after_readiness_check() {
    use osg_companion_service::SourceSink;

    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, _) = ready_feed(clock, "left");
    let frames = feed.feed.initial_audited().unwrap();
    feed.feed.require_live_ready().unwrap();
    let previous = feed.refreshed.clone();

    provider.unregister("left").unwrap();
    assert!(feed.accept_baseline(frames).is_err());
    assert_eq!(feed.refreshed, previous, "rejected baseline cannot refresh its marker");
}

#[test]
fn final_baseline_acceptance_rejects_publication_replacement_after_readiness_check() {
    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, _) = ready_feed(clock, "left");
    let frames = feed.feed.initial_audited().unwrap();
    feed.feed.require_live_ready().unwrap();
    let previous = feed.refreshed.clone();

    provider.register_definition(definition("left", 'b')).unwrap();
    assert!(feed.accept_baseline(frames).is_err());
    assert_eq!(feed.refreshed, previous, "rejected baseline cannot refresh its marker");
}

#[test]
fn baseline_preserves_independent_wire_and_native_generations() {
    let provider = CompanionProvider::new(Arc::new(|| 100));
    provider.register_definition(definition("left", 'a')).unwrap();
    let wire = token("left", 'a', 17);
    provider.baseline_native(&wire, 0, &[osg_mapping::engine::Change {
        control: "button".into(), value: Value::Button(false),
    }]).unwrap();
    let mut feed = provider.subscribe_native(&[NativeSelection {
        definition: definition("left", 'a'), controls: vec!["button".into()],
    }]).unwrap();
    let frames = feed.baseline().unwrap();
    assert_eq!(frames[0].generation.0, 1);
    assert_eq!(wire.generation, 17);
    provider.heartbeat_native(&wire).unwrap();
    assert_eq!(feed.next_deadline_us(), Some(100 + LEASE_US));
}

#[test]
fn final_baseline_acceptance_preserves_same_generation_edge_after_the_baseline() {
    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, token) = ready_feed(clock.clone(), "left");
    let frames = feed.feed.initial_audited().unwrap();
    feed.feed.require_live_ready().unwrap();

    clock.store(200, Ordering::Release);
    provider
        .changes_native(
            &token,
            1,
            &[osg_mapping::engine::Change {
                control: "button".into(),
                value: Value::Button(true),
            }],
        )
        .unwrap();
    let baseline = feed.accept_baseline(frames).unwrap();
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0].frame.reason, Reason::Baseline);
    assert_eq!(baseline[0].frame.sequence.0, 1);
    assert_eq!(baseline[0].frame.changes[0].value, Value::Button(false));

    let events = feed.poll().unwrap();
    assert_eq!(events.len(), 1, "the queued input edge remains ordered after the baseline");
    assert_eq!(events[0].reason, Reason::Change);
    assert_eq!(events[0].sequence.0, 2);
    assert_eq!(events[0].captured_us.0, 200);
    assert_eq!(events[0].changes[0].value, Value::Button(true));
}

#[test]
fn baseline_capture_returns_error_when_source_is_unregistered_before_initial_read() {
    use osg_companion_service::SourceSink;

    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, _) = ready_feed(clock, "left");
    provider.unregister("left").unwrap();
    assert!(feed.baseline_audited().is_err());
}

#[test]
fn baseline_capture_returns_error_when_source_is_unregistered_before_readiness_check() {
    use osg_companion_service::SourceSink;

    let clock = Arc::new(AtomicU64::new(100));
    let (provider, mut feed, _) = ready_feed(clock, "left");
    let frames = feed.feed.initial_audited().unwrap();
    provider.unregister("left").unwrap();
    assert!(feed.feed.require_live_ready().is_err());
    assert!(feed.accept_baseline(frames).is_err());
    assert_eq!(feed.refreshed.len(), 1, "failed acceptance leaves the prior marker intact");
}

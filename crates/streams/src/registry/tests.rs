use super::*;
use crate::{MAX_CHANGE_BYTES, MAX_SNAPSHOT_BYTES};
use serde_json::json;
use std::{sync::Barrier, thread};

fn broker() -> (Broker, Publisher) {
    let broker = Broker::new("service-session".into()).unwrap();
    let publisher = broker
        .register(
            spec("controls"),
            json!({"value": 0}),
            u64::MAX,
            "capture-clock".into(),
        )
        .unwrap();
    (broker, publisher)
}
fn spec(topic: &str) -> TopicSpec {
    TopicSpec {
        topic: topic.into(),
        source: "device:1".into(),
        audiences: vec!["ui".into(), "mcp".into()],
    }
}

#[test]
fn exact_wire_integers_and_scopes() {
    let (broker, publisher) = broker();
    let mut reader = broker.subscribe("client", "mcp", "controls").unwrap();
    let frame = reader.recv().unwrap();
    assert_eq!(
        serde_json::to_value(frame).unwrap(),
        json!({
            "kind":"snapshot", "topic":"controls", "source":"device:1", "session":"service-session",
            "generation":"1", "revision":"1", "capturedUs":u64::MAX.to_string(),
            "clockId":"capture-clock", "continuity":"baseline", "payload":{"value":0}
        })
    );
    publisher
        .publish(1, json!(1), json!({"edge": "down"}), u64::MAX - 1, "clock")
        .unwrap();
    let frame = reader.recv().unwrap();
    assert_eq!(frame.kind, "change");
    assert_eq!(frame.sequence.as_deref(), Some("1"));
    assert_eq!(frame.captured_us, (u64::MAX - 1).to_string());
    publisher
        .replace_generation(u64::MAX, json!(2), u64::MAX, "clock")
        .unwrap();
    assert_eq!(reader.recv().unwrap().generation, u64::MAX.to_string());
    assert_eq!(reader.recv().unwrap().kind, "snapshot");
}

#[test]
fn observing_unknown_or_unauthorized_topic_never_mutates() {
    let (broker, publisher) = broker();
    assert!(matches!(
        broker.subscribe("client", "panel", "controls"),
        Err(RegistryError::Unauthorized)
    ));
    assert!(matches!(
        broker.subscribe("client", "mcp", "unknown"),
        Err(RegistryError::UnknownTopic)
    ));
    assert!(matches!(
        broker.subscribe("client", "admin", "controls"),
        Err(RegistryError::InvalidAudience)
    ));
    assert_eq!(publisher.revision(), 1);
    assert_eq!(broker.shared.lock().topics.len(), 1);
    assert!(broker.shared.lock().readers.is_empty());
}

#[test]
fn identities_descriptors_and_topic_table_are_bounded() {
    assert!(matches!(
        Broker::new("bad space".into()),
        Err(RegistryError::InvalidIdentity)
    ));
    assert!(matches!(
        Broker::new("x".repeat(129)),
        Err(RegistryError::InvalidIdentity)
    ));
    assert!(matches!(
        Broker::new("é".into()),
        Err(RegistryError::InvalidIdentity)
    ));
    let (broker, _) = broker();
    let mut descriptor = spec("invalid");
    descriptor.audiences.clear();
    assert!(matches!(
        broker.register(descriptor, json!(0), 0, "clock".into()),
        Err(RegistryError::InvalidAudience)
    ));
    assert!(matches!(
        broker.register(spec("controls"), json!(0), 0, "clock".into()),
        Err(RegistryError::DuplicateTopic)
    ));
    for n in 1..MAX_TOPICS {
        broker
            .register(spec(&format!("topic/{n}")), json!(0), 0, "clock".into())
            .unwrap();
    }
    assert!(matches!(
        broker.register(spec("overflow"), json!(0), 0, "clock".into()),
        Err(RegistryError::TopicLimit)
    ));
}

#[test]
fn cap_admission_is_atomic_across_topics_and_clients() {
    let (broker, _) = broker();
    broker
        .register(spec("other"), json!(0), 0, "clock".into())
        .unwrap();
    let barrier = Arc::new(Barrier::new(33));
    let workers: Vec<_> = (0..32)
        .map(|n| {
            let broker = broker.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                broker.subscribe(
                    "same-client",
                    "ui",
                    if n % 2 == 0 { "controls" } else { "other" },
                )
            })
        })
        .collect();
    barrier.wait();
    let readers: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        readers.iter().filter(|reader| reader.is_ok()).count(),
        MAX_CLIENT_READERS
    );
    assert_eq!(
        readers
            .iter()
            .filter(|reader| matches!(reader, Err(RegistryError::ClientLimit)))
            .count(),
        16
    );
    let mut retained: Vec<_> = readers.into_iter().filter_map(Result::ok).collect();
    for n in 0..48 {
        retained.push(
            broker
                .subscribe(&format!("client-{n}"), "mcp", "other")
                .unwrap(),
        );
    }
    assert!(matches!(
        broker.subscribe("additional", "ui", "controls"),
        Err(RegistryError::SubscriberLimit)
    ));
    retained.clear();
    assert!(broker.shared.lock().readers.is_empty());
}

#[test]
fn cancellation_and_client_close_wake_and_release_caps() {
    let (broker, _) = broker();
    let mut reader = broker.subscribe("client", "ui", "controls").unwrap();
    reader.recv().unwrap();
    let cancellation = reader.cancellation_handle();
    let worker = thread::spawn(move || reader.recv());
    cancellation.cancel();
    assert!(worker.join().unwrap().is_err());
    assert!(broker.shared.lock().readers.is_empty());
    let mut reader = broker.subscribe("client", "ui", "controls").unwrap();
    reader.recv().unwrap();
    let retained_handle = reader.cancellation_handle();
    let worker = thread::spawn(move || reader.recv());
    broker.close_client("client");
    assert!(worker.join().unwrap().is_err());
    assert!(broker.shared.lock().readers.is_empty());
    let reader = broker.subscribe("client", "ui", "controls").unwrap();
    let handle = reader.cancellation_handle();
    drop(reader);
    assert!(broker.shared.lock().readers.is_empty());
    handle.cancel();
    retained_handle.cancel();
}

#[test]
fn close_is_permanent_and_clients_are_independent() {
    let (broker, publisher) = broker();
    let mut first = broker.subscribe("first", "mcp", "controls").unwrap();
    let mut second = broker.subscribe("second", "mcp", "controls").unwrap();
    first.recv().unwrap();
    second.recv().unwrap();
    broker.close_client("first");
    assert!(first.recv().is_err());
    publisher
        .publish(1, json!(1), json!(1), 1, "clock")
        .unwrap();
    assert_eq!(second.recv().unwrap().payload, Some(json!(1)));
    broker.close();
    assert!(second.recv().is_err());
    assert!(matches!(
        broker.subscribe("second", "ui", "controls"),
        Err(RegistryError::Closed)
    ));
    assert_eq!(
        publisher.publish(1, json!(2), json!(2), 2, "clock"),
        Err(RegistryError::Publish(PublishError::Closed))
    );
    assert!(matches!(
        broker.register(spec("later"), json!(0), 0, "clock".into()),
        Err(RegistryError::Closed)
    ));
}

#[test]
fn overflow_requires_fresh_baseline_and_rejects_old_generation() {
    let (broker, publisher) = broker();
    let mut reader = broker.subscribe("client", "mcp", "controls").unwrap();
    reader.recv().unwrap();
    for n in 0..300 {
        publisher
            .publish(1, json!(n), json!(n), n, "clock")
            .unwrap();
    }
    let loss = reader.recv().unwrap();
    assert_eq!(loss.reason.as_deref(), Some("overflow"));
    assert_eq!(loss.captured_us, "0");
    assert_eq!(loss.clock_id, "none");
    assert!(reader.try_recv().unwrap().is_none());
    publisher
        .publish(1, json!(301), json!(301), 301, "clock")
        .unwrap();
    assert!(reader.try_recv().unwrap().is_none());
    publisher
        .resynchronize(1, json!(302), 302, "clock")
        .unwrap();
    assert_eq!(reader.recv().unwrap().kind, "loss");
    assert_eq!(reader.recv().unwrap().payload, Some(json!(302)));
    publisher
        .replace_generation(2, json!(303), 303, "clock")
        .unwrap();
    assert_eq!(
        publisher.resynchronize(1, json!(999), 999, "clock"),
        Err(RegistryError::Publish(PublishError::StaleGeneration))
    );
    assert_eq!(
        publisher.publish(1, json!(999), json!(999), 999, "clock"),
        Err(RegistryError::Publish(PublishError::StaleGeneration))
    );
    assert_eq!(
        publisher.replace_generation(2, json!(999), 999, "clock"),
        Err(RegistryError::Publish(PublishError::StaleGeneration))
    );
    reader.recv().unwrap();
    assert_eq!(reader.recv().unwrap().payload, Some(json!(303)));
}

#[test]
fn rejects_complete_envelope_oversize_before_mutation() {
    let (broker, publisher) = broker();
    let mut reader = broker.subscribe("client", "mcp", "controls").unwrap();
    let frame = reader.recv().unwrap();
    assert!(serde_json::to_vec(&frame).unwrap().len() <= MAX_SNAPSHOT_BYTES);
    assert_eq!(
        publisher.publish(
            1,
            json!(0),
            json!("x".repeat(MAX_CHANGE_BYTES - 2)),
            0,
            "clock"
        ),
        Err(RegistryError::Publish(PublishError::FrameTooLarge))
    );
    assert_eq!(
        publisher.resynchronize(1, json!("x".repeat(MAX_SNAPSHOT_BYTES - 2)), 0, "clock"),
        Err(RegistryError::Publish(PublishError::SnapshotTooLarge))
    );
    assert_eq!(publisher.revision(), 1);
    assert!(reader.try_recv().unwrap().is_none());
    publisher
        .publish(
            1,
            json!(1),
            json!("x".repeat(MAX_CHANGE_BYTES - ENVELOPE_RESERVE - 2)),
            u64::MAX,
            "clock",
        )
        .unwrap();
    assert!(serde_json::to_vec(&reader.recv().unwrap()).unwrap().len() <= MAX_CHANGE_BYTES);
}

#[test]
fn concurrent_snapshot_boundary_never_skips_a_committed_change() {
    let (broker, publisher) = broker();
    let barrier = Arc::new(Barrier::new(2));
    let producer_barrier = barrier.clone();
    let producer = thread::spawn(move || {
        producer_barrier.wait();
        for n in 1..=100 {
            publisher
                .publish(1, json!(n), json!(n), n, "clock")
                .unwrap();
        }
    });
    barrier.wait();
    let mut reader = broker.subscribe("client", "ui", "controls").unwrap();
    producer.join().unwrap();
    let baseline = reader.recv().unwrap();
    let mut revision = baseline.revision.parse::<u64>().unwrap();
    while let Some(frame) = reader.try_recv().unwrap() {
        revision += 1;
        assert_eq!(frame.revision, revision.to_string());
        assert_eq!(frame.payload.unwrap().as_u64().unwrap() + 1, revision);
    }
    assert_eq!(revision, 101);
}

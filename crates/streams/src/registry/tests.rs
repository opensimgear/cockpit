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
fn discovery_reports_only_current_topics_for_the_audience_without_readers() {
    let (broker, publisher) = broker();
    assert!(broker.available_to("controls", "mcp"));
    assert!(!broker.available_to("controls", "browser"));
    assert!(!broker.available_to("unknown", "mcp"));
    assert!(!publisher.has_readers());
    publisher.close();
    assert!(!broker.available_to("controls", "mcp"));
    broker.close();
    assert!(!broker.available_to("controls", "mcp"));
}

#[test]
fn reader_demand_survives_cancellation_until_the_last_reader_leaves() {
    let (broker, publisher) = broker();
    let first = broker.subscribe("first", "ui", "controls").unwrap();
    let second = broker.subscribe("second", "mcp", "controls").unwrap();
    assert!(publisher.has_reader_demand());
    first.cancellation_handle().cancel();
    drop(first);
    assert!(publisher.has_reader_demand());
    drop(second);
    assert!(!publisher.has_reader_demand());
    let final_reader = broker.subscribe("final", "ui", "controls").unwrap();
    final_reader.cancellation_handle().cancel();
    assert!(!publisher.has_reader_demand());
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
                    "mcp",
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
    for n in 0..MAX_COCKPIT_READERS {
        let topic = if n % 2 == 0 { "controls" } else { "other" };
        retained.push(broker.subscribe("cockpit", "ui", topic).unwrap());
    }
    assert!(matches!(broker.subscribe("cockpit", "ui", "other"), Err(RegistryError::ClientLimit)));
    broker.register(spec("bulk"), json!(0), 0, "clock".into()).unwrap();
    for n in retained.len()..MAX_READERS {
        retained.push(
            broker
                .subscribe(&format!("client-{n}"), "mcp", "bulk")
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
#[test]
fn retirement_is_pointer_safe_idle_only_and_old_owner_cannot_remove_replacement() {
    let (broker, publisher) = broker();
    let reader = broker.subscribe("client", "ui", "controls").unwrap();
    assert!(!broker.retire_if_idle(&publisher));
    reader.cancellation_handle().cancel();
    assert!(broker.retire_if_idle(&publisher));
    assert!(!broker.retire_if_idle(&publisher));
    let replacement = broker
        .register(spec("controls"), json!({"value":42}), 1, "clock".into())
        .unwrap();
    assert!(!broker.retire_if_idle(&publisher));
    assert!(matches!(
        publisher.publish(1, json!({}), json!({}), 1, "clock"),
        Err(RegistryError::Publish(PublishError::Closed))
    ));
    let mut reader = broker.subscribe("new-client", "ui", "controls").unwrap();
    assert_eq!(reader.recv().unwrap().payload.unwrap()["value"], 42);
    assert!(!broker.retire_if_idle(&replacement));
    drop(reader);
    assert!(broker.retire_if_idle(&replacement));
}

#[test]
fn encoded_publication_fences_counter_exhaustion_without_partial_commit() {
    let (broker, publisher) = broker();
    let mut reader = broker.subscribe("client", "ui", "controls").unwrap();
    reader.recv().unwrap();
    for (revision, sequence) in [(u64::MAX, 0), (1, u64::MAX)] {
        {
            let mut state = publisher.topic.stream.shared.lock();
            state.revision = revision;
            state.sequence = sequence;
        }
        assert_eq!(
            publisher.publish(1, json!(42), json!(42), 42, "clock"),
            Err(RegistryError::Publish(PublishError::CounterExhausted))
        );
        let state = publisher.topic.stream.shared.lock();
        assert_eq!(state.revision, revision);
        assert_eq!(state.sequence, sequence);
        assert_eq!(
            serde_json::from_slice::<Value>(&state.snapshot.bytes).unwrap(),
            json!({"value":0})
        );
        drop(state);
        assert!(reader.try_recv().unwrap().is_none());
    }
}

#[test]
fn owner_retirement_closes_live_readers_and_fences_late_old_owner() {
    let (broker, publisher) = broker();
    let mut reader = broker.subscribe("client", "ui", "controls").unwrap();
    reader.recv().unwrap();
    publisher.publish(1, json!(1), json!(1), 2, "clock").unwrap();
    assert!(broker.retire(&publisher));
    let closed = reader.recv().unwrap();
    assert_eq!(closed.kind, "loss");
    assert_eq!(closed.reason.as_deref(), Some("closed"));
    assert!(matches!(broker.subscribe("other", "ui", "controls"), Err(RegistryError::UnknownTopic)));
    let replacement = broker.register(spec("controls"), json!(42), 3, "clock".into()).unwrap();
    assert!(!broker.retire(&publisher));
    assert!(matches!(publisher.publish(1, json!(2), json!(2), 4, "clock"),
        Err(RegistryError::Publish(PublishError::Closed))));
    let mut current = broker.subscribe("new-client", "ui", "controls").unwrap();
    assert_eq!(current.recv().unwrap().payload, Some(json!(42)));
    assert_eq!(replacement.revision(), 1);
}

#[test]
fn latest_absolute_states_coalesce_for_slow_wire_readers_without_losing_fast_readers() {
    let (broker,publisher) = broker();
    let mut slow = broker.subscribe("slow","ui","controls").unwrap();
    let mut fast = broker.subscribe("fast","mcp","controls").unwrap();
    fast.recv().unwrap();
    for value in 1..=1000 {
        assert_eq!(publisher.publish_latest_state(1,json!({"value":value}),value,"clock").unwrap(),value+1);
        let frame = fast.recv().unwrap();
        assert_eq!(frame.kind,"snapshot");assert_eq!(frame.sequence,None);
        assert_eq!(frame.topic,"controls");assert_eq!(frame.source,"device:1");
        assert_eq!(frame.session,"service-session");assert_eq!(frame.generation,"1");
        assert_eq!(frame.revision,(value+1).to_string());assert_eq!(frame.captured_us,value.to_string());
        assert_eq!(frame.payload,Some(json!({"value":value})));
    }
    let frame = slow.recv().unwrap();
    assert_eq!(frame.kind,"snapshot");assert_eq!(frame.payload,Some(json!({"value":1000})));
    assert!(slow.try_recv().unwrap().is_none());
    let state = publisher.topic.stream.shared.lock();
    for reader in state.subscribers.values() {assert!(reader.events.is_empty());assert_eq!(reader.pending_bytes,0);}
}

#[test]
fn latest_state_wire_path_uses_snapshot_budget_and_preserves_real_boundaries() {
    let (broker,publisher) = broker();
    let mut reader = broker.subscribe("client","ui","controls").unwrap();reader.recv().unwrap();
    let large = json!("x".repeat(MAX_CHANGE_BYTES+1));
    publisher.publish_latest_state(1,large.clone(),3,"clock").unwrap();
    assert_eq!(reader.recv().unwrap().payload,Some(large));
    let revision = publisher.revision();
    assert_eq!(publisher.publish_latest_state(2,json!(9),4,"clock"),
        Err(RegistryError::Publish(PublishError::StaleGeneration)));
    assert_eq!(publisher.publish_latest_state(1,json!("x".repeat(MAX_SNAPSHOT_BYTES)),4,"clock"),
        Err(RegistryError::Publish(PublishError::SnapshotTooLarge)));
    assert_eq!(publisher.revision(),revision);assert!(reader.try_recv().unwrap().is_none());
    publisher.replace_generation(2,json!(4),4,"clock").unwrap();
    publisher.publish_latest_state(2,json!(5),5,"clock").unwrap();
    let loss = reader.recv().unwrap();
    assert_eq!(loss.kind,"loss");assert_eq!(loss.reason.as_deref(),Some("generation-replaced"));
    let current = reader.recv().unwrap();assert_eq!(current.kind,"snapshot");assert_eq!(current.payload,Some(json!(5)));
    assert_eq!(current.generation,"2");
    publisher.close();
    assert_eq!(publisher.publish_latest_state(2,json!(6),6,"clock"),Err(RegistryError::Publish(PublishError::Closed)));
    assert_eq!(reader.recv().unwrap().reason.as_deref(),Some("closed"));
}

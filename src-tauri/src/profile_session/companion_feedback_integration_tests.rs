use super::*;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn armed() -> (Fixture, CompanionSources, Phase, Value) {
    armed_with_reconnect(false)
}

fn armed_with_reconnect(reconnect: bool) -> (Fixture, CompanionSources, Phase, Value) {
    let fixture = Fixture::new();
    let mut adapter = fixture.prepared();
    fixture.acquire(&mut adapter, &[]);
    let mut open = fixture.connect();
    if reconnect {
        open = fixture.connect();
    }
    let delivery = fixture.delivery();
    adapter.bind_delivery_plan(&delivery).unwrap();
    let frames = adapter
        .baseline(&fixture.hub, |_| Ok(()), 100)
        .unwrap()
        .unwrap();
    let mut phase = Phase::new(
        delivery.program,
        Counter(1),
        uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    for frame in frames {
        phase.accept(frame).unwrap();
    }
    phase.drain();
    adapter.arm(phase.run_token()).unwrap();
    (fixture, adapter, phase, open)
}

#[test]
fn feedback_fences_wire_and_native_generations_in_their_own_domains() {
    let (fixture, mut adapter, phase, open) = armed_with_reconnect(true);
    let native_generation = adapter.instances[0].generation;
    let wire_generation = fixture
        .hub
        .profile_feedback_token(adapter.instances[0].receipt.as_ref().unwrap())
        .unwrap()
        .generation;
    assert_ne!(
        native_generation, wire_generation,
        "Fixture must exercise independently advancing generations"
    );
    assert!(adapter
        .publish_feedback_background(&fixture.hub, &phase, |_| Ok(()), 100)
        .is_empty());
    assert!(complete(&mut adapter, &fixture.hub, &phase).is_empty());
    assert!(adapter.instances[0].feedback_live);
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_eq!(reply["feedback"]["values"][0]["value"], false);
}

fn complete(adapter: &mut CompanionSources, hub: &Hub, phase: &Phase) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut diagnostics = vec![];
    while adapter.feedback_worker.busy() {
        diagnostics.extend(adapter.publish_feedback_background(hub, phase, |_| Ok(()), 100));
        assert!(
            Instant::now() < deadline,
            "Feedback worker did not complete"
        );
        std::thread::yield_now();
    }
    diagnostics
}

#[test]
fn blocked_validation_never_blocks_mapping_and_expired_samples_are_discarded() {
    let (fixture, mut adapter, phase, open) = armed();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let held = std::sync::Mutex::new(Some(held));
    let began = Instant::now();
    assert!(adapter
        .publish_feedback_background(
            &fixture.hub,
            &phase,
            move |_| {
                if let Some(held) = held.lock().unwrap().take() {
                    entered.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(2)).unwrap();
                }
                Ok(())
            },
            100
        )
        .is_empty());
    assert!(began.elapsed() < Duration::from_millis(100));
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    let began = Instant::now();
    for _ in 0..100 {
        assert!(adapter
            .publish_feedback_background(
                &fixture.hub,
                &phase,
                |_| panic!("Busy feedback admitted a second validator"),
                100
            )
            .is_empty());
    }
    assert!(began.elapsed() < Duration::from_millis(100));
    std::thread::sleep(Duration::from_millis(275));
    release.send(()).unwrap();
    assert!(complete(&mut adapter, &fixture.hub, &phase).is_empty());
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_ne!(reply["feedback"]["values"][0]["status"], "fresh");
    assert!(!adapter.instances[0].feedback_live);
}

#[test]
fn queued_feedback_preserves_actual_validation_elapsed_source_age() {
    let (fixture, mut adapter, phase, open) = armed();
    assert!(adapter
        .publish_feedback_background(
            &fixture.hub,
            &phase,
            |_| {
                std::thread::sleep(Duration::from_millis(25));
                Ok(())
            },
            100
        )
        .is_empty());
    assert!(complete(&mut adapter, &fixture.hub, &phase).is_empty());
    assert!(adapter.instances[0].feedback_live);
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_eq!(reply["feedback"]["values"][0]["value"], false);
    assert!(
        reply["feedback"]["values"][0]["sourceAgeMs"]
            .as_u64()
            .unwrap()
            >= 25
    );
}

#[test]
fn cancelled_worker_cannot_publish_or_clear_a_reconnected_feedback_session() {
    let (fixture, mut adapter, phase, _) = armed();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let held = std::sync::Mutex::new(Some(held));
    adapter.publish_feedback_background(
        &fixture.hub,
        &phase,
        move |_| {
            if let Some(held) = held.lock().unwrap().take() {
                entered.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            Ok(())
        },
        100,
    );
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    adapter.stop(&fixture.hub).unwrap();
    let open = fixture.connect();
    let receipt = adapter.instances[0].receipt.clone().unwrap();
    let token = fixture.hub.profile_feedback_token(&receipt).unwrap();
    release.send(()).unwrap();
    assert!(complete(&mut adapter, &fixture.hub, &phase).is_empty());
    assert_eq!(fixture.hub.profile_feedback_token(&receipt).unwrap(), token);
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_ne!(reply["feedback"]["values"][0]["status"], "fresh");
}

#[test]
fn inactive_reducer_cancels_feedback_work_before_scheduling_a_snapshot() {
    let (fixture, mut adapter, phase, open) = armed();
    let revoked = adapter.instances[0].feedback_cancel.clone();
    adapter.run = None;

    let diagnostics = adapter.publish_feedback_background(
        &fixture.hub,
        &phase,
        |_| panic!("An inactive reducer must not validate feedback tokens"),
        100,
    );

    assert_eq!(
        diagnostics,
        vec!["The companion feedback reducer run is not armed by this native owner."]
    );
    assert!(!adapter.feedback_worker.busy());
    assert!(revoked.load(std::sync::atomic::Ordering::Acquire));
    assert!(!adapter.instances[0]
        .feedback_cancel
        .load(std::sync::atomic::Ordering::Acquire));
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_ne!(reply["feedback"]["values"][0]["status"], "fresh");
}

#[test]
fn feedback_admission_reports_missing_delivery_token_without_starting_worker() {
    let (fixture, mut adapter, phase, _) = armed();
    adapter.instances[0].outputs[0].token = None;

    let diagnostics = adapter.publish_feedback_background(
        &fixture.hub,
        &phase,
        |_| panic!("A missing token must fail before native validation"),
        100,
    );

    assert_eq!(
        diagnostics,
        vec!["companion feedback: Bind current companion feedback delivery tokens first."]
    );
    assert!(!adapter.feedback_worker.busy());
}

#[test]
fn native_generation_validation_failure_is_reported_when_worker_completion_is_polled() {
    let (fixture, mut adapter, phase, open) = armed();
    adapter.instances[0].outputs[0]
        .token
        .as_mut()
        .unwrap()
        .provider_generation = "unrelated-generation".into();

    assert!(adapter
        .publish_feedback_background(
            &fixture.hub,
            &phase,
            |_| { panic!("Native-generation mismatch must reject before route validation") },
            100
        )
        .is_empty());
    let diagnostics = complete(&mut adapter, &fixture.hub, &phase);

    assert_eq!(
        diagnostics,
        vec!["companion feedback: The companion feedback native generation changed."]
    );
    assert!(!adapter.instances[0].feedback_live);
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_ne!(reply["feedback"]["values"][0]["status"], "fresh");
}

#[test]
fn failed_completion_from_a_superseded_reducer_is_ignored() {
    let (fixture, mut adapter, old_phase, _) = armed();
    let (validated, validation_reached) = mpsc::channel();
    assert!(adapter
        .publish_feedback_background(
            &fixture.hub,
            &old_phase,
            move |_| {
                validated.send(()).unwrap();
                Err("old route validation failed".into())
            },
            100,
        )
        .is_empty());
    validation_reached
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    let next_phase = Phase::new_at(
        fixture.delivery().program,
        Counter(2),
        uuid::Uuid::new_v4().to_string(),
        Counter(200),
    )
    .unwrap();
    adapter.run = Some(next_phase.run_token());
    adapter.instances[0].feedback_at = Some(200);

    let deadline = Instant::now() + Duration::from_secs(1);
    let mut diagnostics = vec![];
    while adapter.feedback_worker.busy() {
        diagnostics.extend(adapter.publish_feedback_background(
            &fixture.hub,
            &next_phase,
            |_| panic!("A superseded completion must be discarded before new validation"),
            200,
        ));
        assert!(
            Instant::now() < deadline,
            "Superseded feedback completion was not polled"
        );
        std::thread::yield_now();
    }
    assert!(diagnostics.is_empty());
    assert!(!adapter.feedback_worker.busy());
    assert!(!adapter.instances[0].feedback_live);
}

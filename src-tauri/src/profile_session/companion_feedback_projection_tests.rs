use super::*;
use osg_companion_service::{FeedbackMode, FeedbackStatus, FeedbackValue};

fn publish(adapter: &mut CompanionSources, fixture: &Fixture, phase: &Phase) {
    assert!(adapter.publish_feedback_background(&fixture.hub, phase, |_| Ok(()), phase.at_us().0).is_empty());
}

fn complete_at(adapter: &mut CompanionSources, fixture: &Fixture, phase: &Phase) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while adapter.feedback_worker.busy() {
        publish(adapter, fixture, phase);
        assert!(Instant::now() < deadline, "Feedback worker did not complete");
        std::thread::yield_now();
    }
}

fn cooldown() { std::thread::sleep(Duration::from_millis(35)); }

fn capture(fixture: &Fixture, adapter: &mut CompanionSources, phase: &mut Phase,
    open: &Value, sequence: u64, at: u64, value: bool) {
    fixture.clock.store(at, Ordering::Release);
    let button = fixture.publication.controls.iter().find(|control| matches!(control.kind, SourceKind::Button)).unwrap();
    let mut input = fixture.frame(open, "input", &sequence.to_string());
    input["changes"] = json!([{"controlId":button.id,"value":value}]);
    assert_eq!(wire(&fixture.hub, input)["ok"], true);
    let poll = adapter.poll_live(&fixture.hub, |_| Ok(Some(())), at);
    assert!(poll.diagnostics.is_empty(), "{:?}", poll.diagnostics);
    assert!(!poll.frames.is_empty(), "Actual native capture was lost");
    for frame in poll.frames {
        crate::profile_admission_test_clock::frame(phase, frame).unwrap();
    }
    phase.drain();
}

#[test]
fn quiet_feedback_skips_validation_but_same_value_native_capture_sends_delta() {
    let (fixture, mut adapter, mut phase, open) = armed();
    publish(&mut adapter, &fixture, &phase);
    complete_at(&mut adapter, &fixture, &phase);
    let initial = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_eq!(initial["feedback"]["values"][0]["value"], false);
    cooldown();
    for _ in 0..20 {
        assert!(adapter.publish_feedback_background(&fixture.hub, &phase,
            |_| panic!("Quiet feedback must not validate or start a worker"), phase.at_us().0).is_empty());
        assert!(!adapter.feedback_worker.busy());
    }
    let revision = phase.sample_revision();
    let previous_capture = phase.inspected_observation(&endpoint("button")).unwrap().captured_at;
    capture(&fixture, &mut adapter, &mut phase, &open, 1, 200, false);
    assert_eq!(phase.sample_revision(), revision);
    let observation = phase.inspected_observation(&endpoint("button")).unwrap();
    assert_eq!(observation.captured_at, Some(Counter(200)));
    assert_ne!(observation.captured_at, previous_capture);
    publish(&mut adapter, &fixture, &phase);
    assert!(adapter.feedback_worker.busy());
    complete_at(&mut adapter, &fixture, &phase);
    let repeated = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "2"));
    assert_eq!(repeated["feedback"]["mode"], "delta");
    assert_eq!(repeated["feedback"]["values"][0]["value"], false);
    assert_ne!(repeated["feedback"]["sequence"], initial["feedback"]["sequence"]);
}

#[test]
fn constant_feedback_remains_fresh_after_reducer_time_advances_and_renews_at_low_rate() {
    let mut fixture = Fixture::new();
    fixture.plan.sources.clear();
    fixture.plan.program.sources.clear();
    let from = Endpoint { node_id: "condition".into(), port_id: "state".into() };
    fixture.plan.program.sinks[0].from = from.clone();
    fixture.plan.program.nodes.push(osg_mapping::program::Node {
        id: from.node_id.clone(), inputs: vec![],
        outputs: vec![PortDescriptor { id: from.port_id.clone(), name: "Condition".into(),
            direction: Direction::Output, contract: fixture.plan.sinks[0].contract.clone() }],
        operation: osg_mapping::program::Operation::Constant { value: PortValue::Boolean(true) },
    });
    let mut adapter = fixture.prepared();
    fixture.acquire(&mut adapter, &[]);
    let open = fixture.connect();
    adapter.bind_delivery_plan(&fixture.delivery()).unwrap();
    assert!(adapter.baseline(&fixture.hub, |_| Ok(()), 100).unwrap().unwrap().is_empty());
    let mut phase = Phase::new(fixture.plan.program.clone(), Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    adapter.arm(phase.run_token()).unwrap();
    publish(&mut adapter, &fixture, &phase);
    complete_at(&mut adapter, &fixture, &phase);
    let initial = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    phase.advance(Counter(2_000_100)).unwrap();
    let observation = phase.inspected_observation(&from).unwrap();
    assert!(observation.timeless);
    cooldown();
    assert!(adapter.publish_feedback_background(&fixture.hub, &phase,
        |_| panic!("Constant feedback must wait for its renewal deadline"), phase.at_us().0).is_empty());
    assert!(!adapter.feedback_worker.busy());
    std::thread::sleep(Duration::from_millis(760));
    publish(&mut adapter, &fixture, &phase);
    assert!(adapter.feedback_worker.busy());
    complete_at(&mut adapter, &fixture, &phase);
    let renewed = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "2"));
    assert_eq!(renewed["feedback"]["mode"], "delta");
    assert_ne!(renewed["feedback"]["sequence"], initial["feedback"]["sequence"]);
    assert_eq!(renewed["feedback"]["values"][0]["status"], "fresh");
    assert_eq!(renewed["feedback"]["values"][0]["value"], true);
    assert!(renewed["feedback"]["values"][0]["sourceAgeMs"].as_u64().unwrap() < 1000);
}

#[test]
fn expired_sample_publishes_stale_once_and_later_calls_cannot_renew_it_fresh() {
    let (fixture, mut adapter, mut phase, open) = armed();
    publish(&mut adapter, &fixture, &phase);
    complete_at(&mut adapter, &fixture, &phase);
    wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    phase.advance(Counter(1_100_100)).unwrap();
    cooldown();
    publish(&mut adapter, &fixture, &phase);
    complete_at(&mut adapter, &fixture, &phase);
    let expired = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "2"));
    assert_eq!(expired["feedback"]["values"][0]["status"], "stale");
    phase.advance(Counter(1_400_100)).unwrap();
    cooldown();
    assert!(adapter.publish_feedback_background(&fixture.hub, &phase,
        |_| panic!("Stale feedback must not renew itself"), phase.at_us().0).is_empty());
    assert!(!adapter.feedback_worker.busy());
    assert!(adapter.instances[0].feedback_delivery.deadline(None).is_none());
    let unchanged = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "3"));
    assert_eq!(unchanged["feedback"]["sequence"], expired["feedback"]["sequence"]);
    // Feedback ages at one second; the underlying companion contract expires
    // at 1.5 seconds and legitimately clears the retained historical value.
    phase.advance(Counter(1_600_100)).unwrap();
    publish(&mut adapter, &fixture, &phase);
    complete_at(&mut adapter, &fixture, &phase);
    let source_expired = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "4"));
    assert_eq!(source_expired["feedback"]["values"][0]["status"], "stale");
    assert!(source_expired["feedback"]["values"][0]["value"].is_null());
    phase.advance(Counter(1_900_100)).unwrap();
    cooldown();
    assert!(adapter.publish_feedback_background(&fixture.hub, &phase,
        |_| panic!("An expired source cannot restart a publication timer"), phase.at_us().0).is_empty());
    assert!(!adapter.feedback_worker.busy());
}

#[test]
fn busy_worker_retains_latest_capture_and_old_success_does_not_clear_pending_delta() {
    let (fixture, mut adapter, mut phase, open) = armed();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let held = std::sync::Mutex::new(Some(held));
    assert!(adapter.publish_feedback_background(&fixture.hub, &phase, move |_| {
        if let Some(held) = held.lock().unwrap().take() {
            entered.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        Ok(())
    }, phase.at_us().0).is_empty());
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    for sequence in 1..=9 {
        capture(&fixture, &mut adapter, &mut phase, &open, sequence, 100 + sequence * 100, sequence % 2 == 1);
        assert!(adapter.publish_feedback_background(&fixture.hub, &phase,
            |_| panic!("Busy feedback admitted a second validation slot"), phase.at_us().0).is_empty());
        assert!(adapter.feedback_worker.busy());
    }
    release.send(()).unwrap();
    complete_at(&mut adapter, &fixture, &phase);
    let pending = adapter.instances[0].feedback_delivery.prepare(Instant::now(), None).unwrap();
    assert_eq!(pending.mode, FeedbackMode::Delta);
    assert_eq!(pending.samples.len(), 1);
    assert_eq!(pending.samples[0].value, Some(FeedbackValue::Boolean(true)));
    assert_eq!(pending.samples[0].status, FeedbackStatus::Fresh);
    cooldown();
    publish(&mut adapter, &fixture, &phase);
    complete_at(&mut adapter, &fixture, &phase);
    assert!(adapter.instances[0].feedback_delivery.prepare(Instant::now(), None).is_none());
    let current = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "10"));
    assert_eq!(current["feedback"]["values"][0]["value"], true);
}

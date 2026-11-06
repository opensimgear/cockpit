use super::*;
use crate::module_console::actor::Input;
use crate::module_usb::stream::Capture;

struct Producer { sequence: u32, head: u32, sampled: u32, held: bool, axis: u16 }
struct UsbIo {
    legacy: Arc<Fake>, capture: Arc<Capture>, producer: Mutex<Producer>, captures: AtomicUsize,
}
impl Io for UsbIo {
    fn can(&self, _: Value) -> Value { panic!("USB capture dispatched CAN I/O") }
    fn can_cached(&self, _: &str) -> Result<Value, String> { panic!("USB capture queried CAN") }
    fn usb(&self, _: &str, _: UsbRequest) -> Result<UsbSnapshot, String> { panic!("USB capture dispatched a device query") }
    fn snapshots(&self) -> Vec<SlotSnapshot> { self.legacy.snapshots() }
    fn usb_capture(&self, key: &str) -> Result<Arc<Capture>, String> {
        assert_eq!(key, "panel0");
        self.captures.fetch_add(1, Ordering::AcqRel);
        Ok(self.capture.clone())
    }
}
impl UsbIo {
    fn send(&self, edges: &[bool], advance: u32, age: u32) -> Instant {
        let mut producer = self.producer.lock().unwrap();
        producer.sampled = producer.sampled.wrapping_add(advance);
        let first = producer.sequence == 0;
        producer.sequence += 1;
        let after = producer.head;
        let mut entries = vec![];
        for &pressed in edges {
            producer.head += 1;
            producer.held = pressed;
            entries.push(json!({"sequence":producer.head,"control":0,"pressed":pressed,
                "capturedAtMs":producer.sampled.wrapping_sub(age)}));
        }
        let mut metadata = events();
        metadata["stream"] = json!(3);
        metadata["sampledAtMs"] = json!(producer.sampled);
        metadata["head"] = json!(producer.head);
        metadata["held"] = json!([producer.held]);
        metadata["axes"] = json!([producer.axis]);
        if !first {
            metadata["events"] = json!(entries);
            metadata["after"] = json!(after);
            metadata["next"] = json!(producer.head);
            metadata["more"] = json!(false);
        }
        let envelope = json!({"boot":2,"sequence":producer.sequence,
            "kind":if first {"baseline"} else {"update"},"state":metadata});
        let at = Instant::now();
        self.capture.accept(Input::Line { line: format!("@events {envelope}"), received_at: at });
        at
    }
}
fn usb_fixture() -> (ExecutionPlan, Arc<UsbIo>) {
    let (plan, legacy) = fixture(false, 1);
    let capture = Capture::new(usb_token()).unwrap();
    let io = Arc::new(UsbIo { legacy, capture, captures: AtomicUsize::new(0),
        producer: Mutex::new(Producer { sequence: 0, head: 0, sampled: 10, held: false, axis: 65535 }) });
    io.send(&[], 0, 0);
    (plan, io)
}
fn acquired(plan: &ExecutionPlan, io: Arc<UsbIo>) -> PanelInputs {
    let mut inputs = PanelInputs::prepare_io(plan, io).unwrap();
    inputs.acquire(1_000_000).unwrap();
    await_usb_baseline(&mut inputs, plan, false, "inactive acquisition");
    inputs
}
fn activate(inputs: &mut PanelInputs, plan: &ExecutionPlan) -> Vec<Frame> {
    inputs.begin_live_baseline().unwrap();
    await_usb_baseline(inputs, plan, true, "activation admission")
}

fn await_usb_baseline(inputs: &mut PanelInputs, plan: &ExecutionPlan, live: bool, stage: &str) -> Vec<Frame> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let result = if live { inputs.live_baseline(plan, |_| Ok(())) }
            else { inputs.baseline(plan, |_| Ok(())) };
        if let Some(frames) = result.unwrap_or_else(|error| panic!("{stage}: {error}")) { return frames; }
        if Instant::now() >= deadline {
            let capture = inputs.specs.iter().find_map(|spec| match &spec.target {
                Target::Usb { key, .. } => inputs.io.usb_capture(key).ok(), _ => None,
            }).map(|capture| capture.snapshot());
            let mailbox = inputs.mailbox.lock().unwrap();
            panic!("{stage}: baseline timed out; active={}, admitted={}, native={capture:?}, baselines={:?}, diagnostics={:?}, liveness={:?}, workers_finished={:?}",
                inputs.active.load(Ordering::Acquire), inputs.admitted.load(Ordering::Acquire),
                mailbox.baselines, mailbox.diagnostics, mailbox.liveness,
                inputs.workers.iter().map(JoinHandle::is_finished).collect::<Vec<_>>());
        }
        thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn usb_is_passive_quiet_and_activation_anchors_current_metadata_without_replaying_history() {
    let (plan, io) = usb_fixture();
    let wake = Arc::new(NativeWake::default());
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    assert!(!inputs.hold_uses_notifications(), "A hold needs an admitted baseline and a host wake");
    inputs.set_wake(Some(wake.clone()));
    assert!(!inputs.hold_uses_notifications(), "Registering a wake alone cannot admit a hold");
    inputs.acquire(1_000_000).unwrap();
    let initial = await_usb_baseline(&mut inputs, &plan, false, "quiet fixture inactive acquisition");
    assert!(inputs.hold_uses_notifications());
    assert!(initial[0].captured_us.0 < 1_000_000, "Pre-acquisition capture is preserved by subtraction");
    let (started, origin) = inputs.clock.unwrap();
    let clock = usb_source::Clock { started, origin };
    assert_eq!(sample(&initial[0], "axis:0"), Some(&SignalSample::scalar(PortValue::Number(65535.0))));
    assert_eq!(initial[0].captured_us.0, clock.at(io.capture.snapshot().received_at.unwrap()).unwrap());
    assert_eq!(sample(&initial[0], "button-event:0"), Some(&SignalSample::Unknown));
    let sequence = inputs.mailbox.lock().unwrap().baselines["logical0"].sequence;
    let notified = wake.observed();
    thread::sleep(Duration::from_millis(25));
    assert_eq!(inputs.mailbox.lock().unwrap().baselines["logical0"].sequence, sequence);
    assert_eq!(wake.observed(), notified, "Idle USB has no acquisition interval");
    assert_eq!(io.captures.load(Ordering::Acquire), 1);
    assert_eq!(io.legacy.calls.load(Ordering::Acquire), 0);
    let live_at = io.send(&[true, false], 1, 0);
    let sampled = io.producer.lock().unwrap().sampled;
    let baseline = activate(&mut inputs, &plan);
    assert_eq!(sample(&baseline[0], "axis:0"), Some(&SignalSample::scalar(PortValue::Number(65535.0))));
    assert_eq!(baseline[0].captured_us.0, clock.at(live_at).unwrap());
    assert_eq!(io.producer.lock().unwrap().sampled, sampled, "Activation needs no later sample or health interval");
    assert_eq!(sample(&baseline[0], "button-event:0"), Some(&SignalSample::Unknown));
    assert!(inputs.poll(|_| Ok(())).unwrap().frames.is_empty());
    inputs.stop();
}

#[test]
fn usb_button_edges_and_final_axis_keep_original_capture_times() {
    let (plan, io) = usb_fixture();
    let mut inputs = acquired(&plan, io.clone());
    activate(&mut inputs, &plan);
    io.producer.lock().unwrap().axis = 12345;
    let at = io.send(&[true, false], 1, 0);
    let frames = eventually(|| {
        let poll = inputs.poll(|_| Ok(())).unwrap();
        poll.frames.iter().any(|frame| sample(frame, "button-event:0").is_some()).then_some(poll.frames)
    });
    let occurrences = frames.iter().filter(|frame| sample(frame, "button-event:0").is_some()).collect::<Vec<_>>();
    assert_eq!(occurrences.len(), 2);
    assert_eq!(sample(occurrences[0], "button-event:0"), Some(&SignalSample::scalar(PortValue::Boolean(true))));
    assert_eq!(sample(occurrences[1], "button-event:0"), Some(&SignalSample::scalar(PortValue::Boolean(false))));
    let (started, origin) = inputs.clock.unwrap();
    let expected = usb_source::Clock { started, origin }.at(at).unwrap();
    assert!(occurrences.iter().all(|frame| frame.captured_us.0 == expected));
    assert_eq!(sample(frames.last().unwrap(), "axis:0"), Some(&SignalSample::scalar(PortValue::Number(12345.0))));
    assert!(frames.windows(2).all(|pair| pair[0].captured_us <= pair[1].captured_us));
    assert_eq!(io.legacy.calls.load(Ordering::Acquire), 0);
    inputs.stop();
}

#[test]
fn usb_preview_and_live_profile_share_capture_without_owning_each_other() {
    use crate::module_usb_fleet::input_observations_tests::{fleet_with_capture, ready, subscribe_to};
    const KEY: &str = "panel0";
    const CLIENT: &str = "shared-usb-profile-preview";
    let (plan, io) = usb_fixture();
    let fleet = fleet_with_capture(KEY, io.capture.clone());
    assert!(Arc::ptr_eq(&fleet.input_capture(KEY).unwrap(), &io.capture));
    let runtime = crate::observations::Runtime::new();
    let (subscription, preview) = subscribe_to(&fleet, &runtime, CLIENT, KEY);
    let _ = ready(&runtime, CLIENT, &preview);
    let mut inputs = acquired(&plan, io.clone());
    activate(&mut inputs, &plan);

    let mut source = io.capture.subscribe().unwrap();
    assert!(matches!(source.recv().unwrap(), osg_streams::Delivery::Snapshot(_)));
    io.producer.lock().unwrap().axis = 12345;
    let at = io.send(&[true, false], 1, 0);
    let osg_streams::Delivery::Event(event) = source.recv().unwrap() else {
        panic!("expected a shared capture update");
    };
    let update = ready(&runtime, CLIENT, &preview);
    assert_eq!(update.frame.captured_us, event.captured_us.to_string());
    assert_eq!(update.frame.clock_id, event.clock_id);
    assert_eq!(event.value.witnessed_at, at);
    let state = &update.frame.payload.as_ref().unwrap()["state"];
    assert_eq!(state["axes"][0], 12345);
    assert!(state.get("events").is_none(), "Preview exposes absolute state only");

    let (started, origin) = inputs.clock.unwrap();
    let clock = usb_source::Clock { started, origin };
    let frames = eventually(|| {
        let frames = inputs.poll(|_| Ok(())).unwrap().frames;
        (!frames.is_empty()).then_some(frames)
    });
    assert_eq!(frames.last().unwrap().captured_us.0, clock.at(at).unwrap());
    assert_eq!(sample(frames.last().unwrap(), "axis:0"), Some(&SignalSample::scalar(PortValue::Number(12345.0))));

    runtime.unsubscribe(CLIENT, &subscription).unwrap();
    io.producer.lock().unwrap().axis = 23456;
    let at = io.send(&[], 1, 0);
    let next = eventually(|| inputs.poll(|_| Ok(())).unwrap().frames.into_iter().last());
    assert_eq!(next.captured_us.0, clock.at(at).unwrap());
    assert_eq!(sample(&next, "axis:0"), Some(&SignalSample::scalar(PortValue::Number(23456.0))));
    assert_eq!(io.captures.load(Ordering::Acquire), 1);
    assert_eq!(io.legacy.calls.load(Ordering::Acquire), 0);
    inputs.stop();
    runtime.close();
    fleet.shutdown();
}

#[test]
fn repeated_health_retains_liveness_time_then_native_expiry_wakes_a_gap() {
    let (plan, io) = usb_fixture();
    let mut inputs = acquired(&plan, io.clone());
    activate(&mut inputs, &plan);
    let before = inputs.mailbox.lock().unwrap().liveness["logical0"].2;
    io.send(&[], 0, 0);
    thread::sleep(Duration::from_millis(20));
    assert!(inputs.poll(|_| Ok(())).unwrap().frames.is_empty());
    assert_eq!(inputs.mailbox.lock().unwrap().liveness["logical0"].2, before);
    let lost = eventually(|| {
        let poll = inputs.poll(|_| Ok(())).unwrap();
        poll.frames.iter().any(|frame| frame.reason == Reason::Gap).then_some(poll)
    });
    assert!(lost.liveness.is_empty());
    assert!(!io.capture.snapshot().ready);
    inputs.stop();
}

#[test]
fn aged_edges_are_discarded_with_gap_then_occurrence_free_baseline() {
    let (plan, io) = usb_fixture();
    let mut inputs = acquired(&plan, io.clone());
    activate(&mut inputs, &plan);
    io.send(&[true], 390, 380);
    let frames = eventually(|| {
        let frames = inputs.poll(|_| Ok(())).unwrap().frames;
        frames.iter().any(|frame| frame.reason == Reason::Baseline).then_some(frames)
    });
    assert_eq!(frames[0].reason, Reason::Gap);
    assert!(frames.iter().filter_map(|frame| sample(frame, "button-event:0")).all(|sample| *sample == SignalSample::Unknown));
    assert_eq!(sample(frames.last().unwrap(), "button:0"), Some(&SignalSample::scalar(PortValue::Boolean(true))));
    inputs.stop();
}

#[test]
fn close_and_revocation_wake_parked_readers_and_drop_both_native_leases() {
    let (plan, io) = usb_fixture();
    let mut inputs = acquired(&plan, io.clone());
    activate(&mut inputs, &plan);
    io.capture.close();
    eventually(|| inputs.poll(|_| Ok(())).unwrap().frames.iter().any(|frame| frame.reason == Reason::Gap).then_some(()));
    inputs.stop();
    let (plan, io) = usb_fixture();
    let mut inputs = acquired(&plan, io.clone());
    let wake = Arc::new(NativeWake::default());
    inputs.set_wake(Some(wake.clone()));
    assert!(inputs.hold_uses_notifications());
    let before = wake.observed();
    io.capture.close();
    eventually(|| (wake.observed() > before).then_some(()));
    assert!(!inputs.poll(|_| Ok(())).unwrap().diagnostics.is_empty());
    inputs.stop();
    assert!(!inputs.hold_uses_notifications());
    let (plan, io) = usb_fixture();
    let mut inputs = acquired(&plan, io.clone());
    inputs.stop();
    eventually(|| {
        let readers = (0..8).map(|_| io.capture.subscribe()).collect::<Vec<_>>();
        readers.iter().all(Result::is_ok).then_some(())
    });
    assert_eq!(io.legacy.calls.load(Ordering::Acquire), 0);
}

#[test]
fn wrong_native_capture_route_is_rejected_without_device_commands() {
    let (plan, io) = usb_fixture();
    io.legacy.usb.lock().unwrap().connection_generation += 1;
    assert!(PanelInputs::prepare_io(&plan, io.clone()).is_err());
    assert_eq!(io.legacy.calls.load(Ordering::Acquire), 0);
}

#[test]
fn coalesced_sample_clocks_and_same_sample_pages_keep_a_coherent_native_witness() {
    use crate::{module_console::push, module_usb::stream::{Batch, Edge, Snapshot}};
    let (plan, io) = usb_fixture();
    let inputs = PanelInputs::prepare_io(&plan, io).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    let started = Instant::now();
    let clock = usb_source::Clock { started, origin: 1_000_000 };
    let mut metadata = events();
    metadata["stream"] = json!(3);
    metadata["sampledAtMs"] = json!(100);
    source.usb_baseline(&Snapshot { route: usb_token(), stream: 3,
        state: Some(Arc::new(metadata.clone())), received_at: Some(started), ready: true, error: None }, clock, started).unwrap();
    let batch = |sequence, sampled, pressed, received_at, witnessed_at| {
        let mut state = metadata.clone();
        state["head"] = json!(sequence);
        state["held"] = json!([pressed]);
        state["sampledAtMs"] = json!(sampled);
        Batch { witnessed_at, events: vec![Edge { sequence, control: 0, pressed, captured_at_ms: 100,
            received_at }], latest: push::Frame { boot: 2, sequence: sequence + 1,
            kind: push::Kind::Update, state: Arc::new(state), reason: None, exhausted: false,
            received_at, sampled_at_ms: sampled, stream: 3, head: sequence, cursor: sequence } }
    };
    let first = started + Duration::from_micros(2000);
    source.usb_batch(&batch(1, 100, true, first, first), clock, first).unwrap();
    let sequence = source.sequence;
    let second = started + Duration::from_micros(2100);
    let frames = source.usb_batch(&batch(2, 101, false, second, second), clock, second).unwrap();
    assert!(frames.iter().all(|frame| frame.captured_us.0 == 1_002_100 && frame.reason == Reason::Change));
    assert!(source.sequence > sequence);
    assert_eq!(source.cursor, Some((3, 2)));
    assert_eq!(source.held, vec![false]);
    let page_rx = started + Duration::from_micros(3000);
    let frames = source.usb_batch(&batch(3, 101, true, page_rx, second), clock, page_rx).unwrap();
    assert!(frames.iter().all(|frame| frame.captured_us.0 == 1_002_100));
    assert!(frames.iter().all(|frame| sample(frame, "axis:0").is_none()), "Cached axes are not refreshed by later RX");
}

#[test]
fn host_admission_discards_usb_frames_that_aged_in_its_mailbox() {
    let (plan, io) = usb_fixture();
    let mut inputs = acquired(&plan, io.clone());
    let baseline = activate(&mut inputs, &plan).remove(0);
    let mut stale = baseline;
    stale.reason = Reason::Change;
    stale.captured_us = Counter(1);
    inputs.mailbox.lock().unwrap().frames.push_back(stale);
    let poll = inputs.poll(|_| Ok(())).unwrap();
    assert_eq!(poll.frames.len(), 1);
    assert_eq!(poll.frames[0].reason, Reason::Gap);
    assert!(poll.diagnostics.iter().any(|message| message.contains("before Host admission")));
    inputs.stop();
}

#[test]
fn encoder_pair_stays_ordered_and_baseline_has_no_occurrence() {
    use crate::{module_console::push, module_usb::stream::{Batch, Edge, Snapshot}};
    let (plan, io) = usb_fixture();
    let inputs = PanelInputs::prepare_io(&plan, io).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    source.spec.ports.retain(|port| port.observed == "button-event:0");
    source.spec.ports[0].observed = "button-event:1".into();
    let at = Instant::now();
    let clock = usb_source::Clock { started: at, origin: 1_000_000 };
    let mut metadata = events();
    metadata["stream"] = json!(3);
    metadata["buttonCount"] = json!(3);
    metadata["held"] = json!([false,false,false]);
    metadata["encoderButtons"] = json!([1,2]);
    let baseline = source.usb_baseline(&Snapshot { route: usb_token(), stream: 3,
        state: Some(Arc::new(metadata.clone())), received_at: Some(at), ready: true, error: None }, clock, at).unwrap();
    assert_eq!(sample(&baseline, "button-event:0"), Some(&SignalSample::Unknown));
    metadata["head"] = json!(2);
    let batch = Batch { witnessed_at: at, events: [true,false].into_iter().enumerate().map(|(index, pressed)| Edge {
        sequence: index as u32 + 1, control: 1, pressed, captured_at_ms: 10, received_at: at,
    }).collect(), latest: push::Frame { boot: 2, sequence: 2, kind: push::Kind::Update,
        state: Arc::new(metadata), reason: None, exhausted: false, received_at: at,
        sampled_at_ms: 10, stream: 3, head: 2, cursor: 2 } };
    let frames = source.usb_batch(&batch, clock, at).unwrap();
    assert_eq!(frames.len(), 2);
    for (frame, pressed) in frames.iter().zip([true,false]) {
        assert_eq!(sample(frame, "button-event:0"), Some(&SignalSample::scalar(PortValue::Boolean(pressed))));
    }
}

#[test]
fn terminal_worker_failure_withdraws_only_its_logical_source() {
    let frame = |source: &str, sequence, captured_us| Frame { source: source.into(),
        generation: Counter(7), sequence: Counter(sequence), captured_us: Counter(captured_us),
        reason: Reason::Baseline, changes: vec![] };
    let healthy = frame("healthy", 4, 2000);
    let mut mailbox = Mailbox::default();
    mailbox.frames.extend([frame("failed", 5, 1300), healthy.clone()]);
    mailbox.baselines.insert("failed".into(), frame("failed", 1, 1000));
    mailbox.baselines.insert("healthy".into(), healthy.clone());
    mailbox.liveness.insert("failed".into(), (7,5,1300));
    mailbox.liveness.insert("healthy".into(), (7,4,2000));
    let wake = Arc::new(NativeWake::default());
    mailbox.wake = Some(wake.clone());
    usb::terminal_failure(&mut mailbox, "failed", "reader failed", Some((7,5,1300)));
    assert_eq!(mailbox.loss_epoch, 0);
    assert_eq!(mailbox.baselines.len(), 1);
    assert_eq!(mailbox.baselines["healthy"].captured_us, healthy.captured_us);
    assert_eq!(mailbox.liveness.get("healthy"), Some(&(7,4,2000)));
    assert!(!mailbox.liveness.contains_key("failed"));
    assert_eq!(mailbox.frames.len(), 2);
    assert_eq!(mailbox.frames[0].source, "healthy");
    let gap = &mailbox.frames[1];
    assert_eq!(gap.source, "failed");
    assert_eq!(gap.reason, Reason::Gap);
    assert_eq!(gap.sequence, Counter(6));
    assert_eq!(gap.captured_us, Counter(1300));
    assert!(wake.observed() > 0);
}

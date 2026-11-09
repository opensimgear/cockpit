use super::*;
use osg_desktop_output::{coordinator::Channel, protocol::{Adapter, NativeEvent, native_count}, worker::Worker};
use osg_mapping::{desktop::{Control, Intent, Key}, engine::{Counter, typed::Dependency}, program::Endpoint};
use std::{sync::mpsc::{Receiver, SyncSender}, time::Duration};

struct RecordingAdapter(Arc<Mutex<Vec<NativeEvent>>>);
impl Adapter for RecordingAdapter {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.lock().unwrap().extend_from_slice(events);
        Ok(native_count(events))
    }
}
struct Block {
    kind: &'static str,
    entered: SyncSender<()>,
    release: Receiver<()>,
}
struct RecordingChannel {
    worker: Worker<RecordingAdapter>,
    requests: Arc<Mutex<Vec<Request>>>,
    block: Option<Block>,
    fail: Option<&'static str>,
    panic: Option<&'static str>,
}
fn kind(request: &Request) -> &'static str {
    match request.command {
        Command::Activate { .. } => "activate", Command::Arm { .. } => "arm",
        Command::Deliver { .. } => "deliver", Command::DeliverGuarded { .. } => "guarded", Command::DeliverPointerBatch { .. } => "pointer",
        Command::Pause { .. } => "pause", Command::Renew { .. } => "renew", Command::Stop => "stop",
        Command::Recover { .. } => "recover",
    }
}
impl Channel for RecordingChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        self.requests.lock().unwrap().push(request.clone());
        if self.block.as_ref().is_some_and(|block| block.kind == kind(request)) {
            let block = self.block.take().unwrap();
            block.entered.send(()).unwrap();
            block.release.recv().unwrap();
        }
        if self.panic == Some(kind(request)) { panic!("recording channel panic"); }
        if self.fail == Some(kind(request)) { return Err("recording channel failure".into()); }
        Ok(self.worker.receive(request.clone(), 10))
    }
}
type Requests = Arc<Mutex<Vec<Request>>>;
type Events = Arc<Mutex<Vec<NativeEvent>>>;
fn channel() -> (RecordingChannel, Requests, Events) {
    let requests = Arc::new(Mutex::new(vec![]));
    let events = Arc::new(Mutex::new(vec![]));
    (RecordingChannel { worker: Worker::new(RecordingAdapter(events.clone())), requests: requests.clone(), block: None, fail: None, panic: None }, requests, events)
}
fn run() -> RunToken { RunToken { session: "owner-tests".into(), generation: Counter(1) } }
fn delivery(action: &str, operation: u64, intent: Intent) -> Delivery {
    Delivery {
        producer: Token { run: run(), node_id: "source".into(), dependencies: vec![Dependency {
            endpoint: Endpoint { node_id: "source".into(), port_id: "value".into() }, generation: Counter(1), epoch: Counter(1),
        }] },
        action_id: action.into(), operation_id: Counter(operation), sequence: Counter(operation), at_us: Counter(10), deadline_us: None, intent,
    }
}
fn held(action: &str, operation: u64) -> Delivery {
    delivery(action, operation, Intent::Held { controls: vec![Control::Key { key: Key(4) }] })
}
fn pointer(action: &str, operation: u64) -> Delivery { delivery(action, operation, Intent::Pointer { dx: 1, dy: -1 }) }
fn inline() -> (Output, Requests, Events) {
    let (channel, requests, events) = channel();
    let output = Output::inline(Coordinator::with_channel(Box::new(channel), || 10));
    output.activate(run(), Scope::DesktopWide).unwrap();
    (output, requests, events)
}
fn reconcile(output: &Output, deliveries: &[Delivery]) {
    output.reconcile(&deliveries.iter().map(|d| (d.action_id.clone(), d.producer.clone())).collect());
}
fn wait_until(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < Duration::from_secs(3), "owner condition timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn block(channel: &mut RecordingChannel, kind: &'static str) -> (Receiver<()>, SyncSender<()>) {
    let (entered, arrival) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    channel.block = Some(Block { kind, entered, release: resume });
    (arrival, release)
}
fn threaded(channel: RecordingChannel) -> Output {
    let output = Output::threaded(Coordinator::with_channel(Box::new(channel), || 10), Arc::new(|| {}), BTreeMap::new()).unwrap();
    output.activate(run(), Scope::DesktopWide).unwrap();
    output
}

#[test]
fn guarded_annotation_requires_nonempty_held_delivery_for_known_tap_action() {
    let durations = BTreeMap::from([("tap".to_string(), Counter(12_000))]);
    assert_eq!(execute::guarded_duration(&held("tap", 1), &durations), Some(Counter(12_000)));
    assert_eq!(execute::guarded_duration(&held("hold", 2), &durations), None);
    assert_eq!(execute::guarded_duration(&delivery("tap", 3, Intent::Held { controls: vec![] }), &durations), None);
    assert_eq!(execute::guarded_duration(&delivery("tap", 4, Intent::Cleanup), &durations), None);
    assert_eq!(execute::guarded_duration(&delivery("tap", 5, Intent::Repeat { key: Key(4) }), &durations), None);
}

#[test]
fn current_gate_rejects_guarded_delivery_before_ipc() {
    let (output, requests, _) = inline();
    let delivery = held("tap", 1);
    let mut stale = delivery.producer.clone();
    stale.run.generation = Counter(2);
    let admission = output.child_scope().map(|receipt| receipt.admission);
    output.shared.lock().executing_admission = admission;
    output.reconcile(&BTreeMap::from([("tap".into(), delivery.producer.clone())]));
    let request = Request { admission, sequence: Counter(1), deadline_us: None,
        command: Command::DeliverGuarded { delivery, release_after_us: Counter(12_000) } };
    assert!(output.shared.permitted(&request));
    output.reconcile(&BTreeMap::from([("tap".into(), stale)]));
    assert!(!output.shared.permitted(&request));
    assert!(!requests.lock().unwrap().iter().any(|request| matches!(request.command, Command::DeliverGuarded { .. })));
}

#[test]
fn mapped_held_tap_uses_guarded_command() {
    let (channel, requests, _) = channel();
    let output = Output::inline_with_tap_durations(
        Coordinator::with_channel(Box::new(channel), || 10),
        BTreeMap::from([("tap".into(), Counter(12_000))]),
    );
    output.activate(run(), Scope::DesktopWide).unwrap();
    let delivery = held("tap", 1);
    reconcile(&output, std::slice::from_ref(&delivery));
    output.dispatch(vec![delivery], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    assert!(requests.lock().unwrap().iter().any(|request| matches!(
        request.command,
        Command::DeliverGuarded { release_after_us: Counter(12_000), .. }
    )));
}

#[test]
fn inline_uses_ordered_executor_and_keeps_ticket_until_completion_taken() {
    let (output, requests, events) = inline();
    let deliveries = vec![pointer("x", 1), pointer("y", 2), held("key", 3), pointer("x", 4), pointer("y", 5)];
    output.dispatch(deliveries, 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    assert!(output.busy());
    assert!(output.pending());
    let completion = output.take_completion().unwrap();
    assert_eq!(completion.outcomes.len(), 5);
    assert!(completion.error.is_none());
    assert!(!output.busy());
    let kinds: Vec<_> = requests.lock().unwrap().iter().map(kind).collect();
    assert_eq!(kinds, ["activate", "arm", "arm", "pointer", "arm", "deliver", "pointer"]);
    assert_eq!(events.lock().unwrap().len(), 3);
}

#[test]
fn elapsed_queue_age_expires_held_and_suppresses_following_taps_but_allows_cleanup() {
    let (output, _, events) = inline();
    let mut first = held("key", 1);
    first.deadline_us = Some(Counter(100));
    let repeat = delivery("key", 2, Intent::Repeat { key: Key(4) });
    let cleanup = delivery("key", 3, Intent::Cleanup);
    {
        let mut state = output.shared.lock();
        state.busy = true;
        state.job = Some(Job { deliveries: vec![first, repeat, cleanup], prearm: None, source_now: 10, key_holds: Default::default(),
            admission: state.child_scope.map(|receipt| receipt.admission), admitted: Instant::now() - Duration::from_millis(2), epoch: state.epoch, audit_shadow: 0 });
    }
    output.drive();
    let completion = output.take_completion().unwrap();
    assert!(completion.error.is_none());
    assert!(completion.outcomes[0].1.skipped);
    assert_eq!(completion.dropped.len(), 1);
    assert_eq!(completion.dropped[0].1, 2);
    assert_eq!(completion.outcomes.len(), 2);
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn cleanup_requires_the_exact_armed_producer_and_pause_clears_owner_slots() {
    let (output, _, _) = inline();
    output.dispatch(vec![delivery("key", 1, Intent::Cleanup)], 10, 42, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    let completion = output.take_completion().unwrap();
    assert_eq!(completion.audit_shadow, 42, "Delayed drops retain their admission's isolated reducer identity");
    assert_eq!(completion.dropped[0].1, 1);
    output.dispatch(vec![held("key", 2)], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    output.take_completion().unwrap();
    output.pause(run()).unwrap();
    output.dispatch(vec![delivery("key", 3, Intent::Cleanup)], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    assert_eq!(output.take_completion().unwrap().dropped[0].1, 1);
}

#[test]
fn pointer_groups_split_at_max_batch_and_encoded_frame_limit() {
    for large in [false, true] {
        let (output, requests, _) = inline();
        let count = if large { 3 } else { 70 };
        let deliveries = (1..=count).map(|id| {
            let mut delivery = pointer(&format!("p{id}"), id);
            if large {
                let width = if id == 3 { 256 } else { 128 };
                delivery.producer.dependencies = (0..64).map(|dependency| Dependency {
                    endpoint: Endpoint {
                        node_id: format!("{dependency:02}{}", "n".repeat(width - 2)),
                        port_id: "p".repeat(width),
                    }, generation: Counter(1), epoch: Counter(1),
                }).collect();
            }
            delivery
        }).collect();
        output.dispatch(deliveries, 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
        let completion = output.take_completion().unwrap();
        assert!(completion.error.is_none(), "{:?}", completion.error);
        assert_eq!(completion.outcomes.len(), count as usize);
        let requests = requests.lock().unwrap();
        let groups = requests.iter().filter_map(|request| match &request.command {
            Command::DeliverPointerBatch { deliveries } => {
                assert!(serde_json::to_vec(request).unwrap().len() <= osg_desktop_output::protocol::FRAME_LIMIT);
                assert!(deliveries.len() <= osg_desktop_output::protocol::MAX_BATCH);
                Some(deliveries.len())
            },
            Command::Deliver { delivery } if matches!(delivery.intent, Intent::Pointer { .. }) => {
                assert!(serde_json::to_vec(request).unwrap().len() <= osg_desktop_output::protocol::FRAME_LIMIT);
                Some(1)
            }, _ => None,
        }).collect::<Vec<_>>();
        assert!(groups.len() >= 2);
        assert_eq!(groups, if large { vec![2, 1] } else { vec![64, 6] });
    }
}

#[test]
fn bounds_and_second_outstanding_batch_fail_closed_explicitly() {
    let (output, _, _) = inline();
    output.dispatch(vec![held("key", 1)], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    assert!(output.dispatch(vec![], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap_err().contains("outstanding"));
    assert!(output.snapshot().error.is_some());
    assert!(output.take_completion().is_some());
    let (output, _, _) = inline();
    assert!(output.dispatch(vec![pointer("x", 1); 4097], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap_err().contains("4096"));
    let (output, _, _) = inline();
    assert!(output.dispatch(vec![pointer(&"x".repeat(400), 1); 4096], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap_err().contains("1 MiB"));
}

#[path = "threaded_tests.rs"]
mod threaded_tests;
#[path = "focus_tests.rs"]
mod focus_tests;
#[path = "child_tests.rs"]
mod child_tests;
#[path = "late_press_tests.rs"]
mod late_press_tests;

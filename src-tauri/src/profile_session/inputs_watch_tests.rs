use super::*;
use osg_controller_input::registry::{Definition, Identity, Registry};
use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};

struct NativeCapture {
    registry: Mutex<Registry>,
    selected: String,
    other: String,
    fail: Cell<bool>,
    race: Cell<bool>,
}
impl HidCapture for NativeCapture {
    fn snapshot(&self, ids: &[String]) -> Result<stream::Snapshot, String> {
        if self.fail.get() { return Err("capture failed".into()); }
        let mut registry = self.registry.lock().unwrap();
        if self.race.replace(false) {
            // Deliberately admit input after registration but before baseline.
            registry.report(&self.selected, 0, &BTreeMap::from([(0, 2)])).unwrap();
        }
        registry.stream_snapshot(ids)
    }
    fn page(&self, session: &str, cursor: u64) -> stream::Page {
        self.registry.lock().unwrap().stream_page(session, cursor)
    }
    fn watch_reports(&self, ids: &[String], wake: Arc<dyn Fn() + Send + Sync>)
        -> Result<Option<stream::ReportWatch>, String> {
        self.registry.lock().unwrap().watch_reports(ids, wake).map(Some)
    }
}
fn native() -> (ExecutionPlan, NativeCapture, Arc<AtomicUsize>, Arc<dyn Fn() + Send + Sync>) {
    let (mut plan, _) = captured();
    let mut registry = Registry::default();
    let definition = |path: &str| Definition {
        identity: Identity { backend: "test".into(), vendor_id: 1, product_id: 2, serial: None,
            collection: "axis".into(), fingerprint: String::new(), path: path.into() },
        name: path.into(), controls: fixture().0.controls, diagnostics: vec![],
    };
    let selected = registry.connect(definition("selected")).unwrap();
    let other = registry.connect(definition("other")).unwrap();
    for id in [&selected, &other] { registry.report(id, 0, &BTreeMap::from([(0, 0)])).unwrap(); }
    let route = plan.sources[0].route.as_mut().unwrap();
    route.id = format!("hid:{selected}"); route.provider_generation = "1".into();
    let token = plan.sources[0].token.as_mut().unwrap();
    token.route_id = format!("hid:{selected}"); token.provider_generation = "1".into();
    let count = Arc::new(AtomicUsize::new(0)); let sent = count.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {sent.fetch_add(1, Ordering::SeqCst);});
    (plan, NativeCapture {registry: Mutex::new(registry), selected, other,
        fail: Cell::new(false), race: Cell::new(false)}, count, wake)
}
fn report(capture: &NativeCapture, selected: bool) {
    capture.registry.lock().unwrap().report(if selected {&capture.selected} else {&capture.other},
        0, &BTreeMap::from([(0, 3)])).unwrap();
}

#[test]
fn reader_owns_exact_source_wake_and_replacement_overlap_has_no_gap() {
    let (plan, capture, count, wake) = native();
    let (first, _) = HidInputs::capture(&plan, &capture, Some(wake.clone()), |_| Ok(())).unwrap();
    report(&capture, false); assert_eq!(count.load(Ordering::SeqCst), 0);
    report(&capture, true); assert_eq!(count.load(Ordering::SeqCst), 1);
    let (second, _) = HidInputs::capture(&plan, &capture, Some(wake), |_| Ok(())).unwrap();
    report(&capture, true); assert_eq!(count.load(Ordering::SeqCst), 3);
    drop(first);
    report(&capture, true); assert_eq!(count.load(Ordering::SeqCst), 4);
    drop(second);
    report(&capture, true); assert_eq!(count.load(Ordering::SeqCst), 4);
}

#[test]
fn registration_before_baseline_cannot_lose_an_interleaved_native_report() {
    let (plan, capture, count, wake) = native(); capture.race.set(true);
    let (mut reader, baseline) = HidInputs::capture(&plan, &capture, Some(wake), |_| Ok(())).unwrap();
    assert!(reader.notification_driven());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(baseline.len(), 1); assert_eq!(baseline[0].reason, Reason::Baseline);
    assert_eq!(baseline[0].changes[0].sample, SignalSample::scalar(osg_profiles::PortValue::Number(2.0)));
    assert!(reader.poll(&capture, |_| Ok(())).unwrap().0.is_empty());
    report(&capture, true);
    assert_eq!(reader.poll(&capture, |_| Ok(())).unwrap().0[0].reason, Reason::Change);
}

#[test]
fn copied_pages_acknowledge_unassigned_history_but_unread_overflow_still_wakes() {
    let (plan, capture, count, wake) = native();
    let (mut reader, _) = HidInputs::capture(&plan, &capture, Some(wake), |_| Ok(())).unwrap();
    for index in 0..stream::REPORT_LIMIT+100 {
        report(&capture, false);
        if index%32 == 0 { assert!(reader.poll(&capture, |_| Ok(())).unwrap().0.is_empty()); }
    }
    assert!(reader.poll(&capture, |_| Ok(())).unwrap().0.is_empty());
    assert_eq!(count.load(Ordering::SeqCst), 0, "Already copied unrelated history cannot wake routing.");
    for _ in 0..stream::REPORT_LIMIT+1 { report(&capture, false); }
    assert!(count.load(Ordering::SeqCst)>0, "Unread global history loss must wake every affected reader.");
    let (frames, errors) = reader.poll(&capture, |_| Ok(())).unwrap();
    assert!(errors.is_empty());
    assert_eq!(frames.iter().map(|f|f.reason.clone()).collect::<Vec<_>>(), vec![Reason::Gap, Reason::Baseline]);
}

#[test]
fn failed_baseline_releases_watch_and_empty_plan_never_registers() {
    let (mut plan, capture, count, wake) = native(); capture.fail.set(true);
    assert!(HidInputs::capture(&plan, &capture, Some(wake.clone()), |_| Ok(())).is_err());
    report(&capture, true); assert_eq!(count.load(Ordering::SeqCst), 0);
    plan.sources.clear();
    let (reader, frames) = HidInputs::capture(&plan, &capture, Some(wake), |_| Ok(())).unwrap();
    assert!(reader.watch.is_none()); assert!(frames.is_empty());
    assert!(reader.notification_driven());
}

#[test]
fn a_live_hid_source_without_a_report_watch_requires_service() {
    let (plan, capture, _, _) = native();
    let (reader, _) = HidInputs::capture(&plan, &capture, None, |_| Ok(())).unwrap();
    assert!(!reader.notification_driven());
}

#[test]
fn a_connected_hid_source_stays_known_after_its_last_report_ages() {
    use osg_mapping::{
        program::{Endpoint, Program, SourcePort},
        signal::{PortContract, ScalarSchema},
    };
    let (plan, capture, count, wake) = native();
    let (reader, frames) = HidInputs::capture(&plan, &capture, Some(wake), |_| Ok(())).unwrap();
    let endpoint = Endpoint { node_id: "pedal".into(), port_id: "raw".into() };
    let program = Program {
        sources: vec![SourcePort {
            endpoint: endpoint.clone(),
            contract: PortContract::State {
                schema: ScalarSchema::Number { min: -2048.0, max: 2047.0, unit: None },
                freshness_ms: 50,
                fallback: None,
            },
        }],
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    let mut reducer = Phase::new(program, Counter(1), "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into()).unwrap();
    let baseline = capture.snapshot(&[capture.selected.clone()]).unwrap();
    let original = &baseline.sources[0].baseline;
    let admitted_at = frames[0].captured_us;
    let sampled = frames[0].changes[0].sample.clone();
    reducer.accept_batch(frames, Counter(baseline.captured_us)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(80));
    let current = capture.snapshot(&[capture.selected.clone()]).unwrap();
    let quiet = &current.sources[0].baseline;
    assert_eq!(quiet.sequence, original.sequence);
    assert!(capture.page(&baseline.session, baseline.cursor).events.is_empty());
    assert!(current.captured_us - admitted_at.0 >= 50_000);
    let now = Counter(current.captured_us);
    reader.refresh_verified_liveness(&capture, &mut reducer, now).unwrap();
    reducer.advance(now).unwrap();
    assert_eq!(reducer.sample(&endpoint), Some(&sampled));
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn paced_hid_edges_reach_the_virtual_reducer_with_a_stalled_preview_and_recover_after_overflow() {
    use osg_mapping::{
        engine::ReportReason,
        profile::OutputControl,
        program::{ControllerLayout, Endpoint, Program, Sink, SinkKind, SourcePort, VirtualController},
        signal::{PortContract, ScalarSchema},
    };
    use std::time::{Duration, Instant};
    let (mut plan, capture, _, wake) = native();
    let contact = Control::new(9, 1, 0, 0, 0);
    let encoder = Control::new(9, 2, 0, 0, 1);
    let id = {
        let mut registry = capture.registry.lock().unwrap();
        registry.connect(Definition {
            identity: Identity { backend: "test".into(), vendor_id: 1, product_id: 2, serial: None,
                collection: "panel".into(), fingerprint: String::new(), path: "panel".into() },
            name: "Panel".into(), controls: vec![contact.clone(), encoder.clone()], diagnostics: vec![],
        }).unwrap()
    };
    let report = |contact, encoder| capture.registry.lock().unwrap()
        .report(&id, 0, &BTreeMap::from([(0, contact), (1, encoder)])).unwrap();
    report(0, 0);
    let broker = osg_streams::Broker::new("paced-hid-routing".into()).unwrap();
    let topic = format!("controllers.input.{id}");
    let observation = capture.registry.lock().unwrap().prepare_observation(&broker, &id).unwrap();
    let mut stalled = observation.with_subscription(|| {
        broker.subscribe("stalled-preview", "ui", &topic).map_err(|error| error.to_string())
    }).unwrap();
    let initial_preview = stalled.recv().unwrap();
    assert_eq!(initial_preview.kind, "snapshot");
    let route_id = format!("hid:{id}");
    let first = &mut plan.sources[0];
    first.route.as_mut().unwrap().id = route_id.clone();
    first.token.as_mut().unwrap().route_id = route_id;
    first.endpoint.port_id = "contact".into();
    first.observed_port_id = Some(contact.id);
    let mut second = first.clone();
    second.endpoint.port_id = "encoder".into();
    second.observed_port_id = Some(encoder.id);
    plan.sources.push(second);

    let contract = PortContract::State { schema: ScalarSchema::Boolean {}, freshness_ms: 1000, fallback: None };
    let endpoints = ["contact", "encoder"].map(|port| Endpoint { node_id: "pedal".into(), port_id: port.into() });
    let program = Program {
        sources: endpoints.iter().cloned().map(|endpoint| SourcePort { endpoint, contract: contract.clone() }).collect(),
        nodes: vec![],
        sinks: endpoints.into_iter().enumerate().map(|(index, from)| Sink {
            id: from.port_id.clone(), endpoint: Endpoint { node_id: "target".into(), port_id: from.port_id.clone() },
            from, contract: contract.clone(), kind: SinkKind::Virtual { controller_id: "target".into(), control: OutputControl::Button { index: index as u16 } },
        }).collect(),
        controllers: vec![VirtualController { id: "target".into(), layout: ControllerLayout {
            buttons: vec!["contact".into(), "encoder".into()], axes: vec![], hats: vec![],
        } }],
    };
    let mut reducer = Phase::new(program, Counter(1), "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into()).unwrap();
    let admit = |reducer: &mut Phase, frames: Vec<Frame>| {
        let now = frames.last().unwrap().captured_us;
        reducer.accept_batch(frames, now).unwrap()
    };
    let (mut reader, baseline) = HidInputs::capture(&plan, &capture, Some(wake), |_| Ok(())).unwrap();
    admit(&mut reducer, baseline);
    reducer.drain_reports();
    let mut expected = Vec::new();
    let mut delivered = Vec::new();
    let started = Instant::now();
    let edges = [(1, 0), (1, 1), (1, 0), (0, 0)];
    for tick in 1..=256 {
        let deadline = started + Duration::from_millis(tick);
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        let (contact, encoder) = edges[(tick as usize - 1) % edges.len()];
        report(contact, encoder);
        expected.push(vec![contact != 0, encoder != 0]);
        if tick % 8 == 0 {
            capture.registry.lock().unwrap()
                .report(&capture.other, 0, &BTreeMap::from([(0, tick as u32)])).unwrap();
        }
        if tick % 16 == 0 {
            let (frames, errors) = reader.poll(&capture, |_| Ok(())).unwrap();
            assert!(errors.is_empty());
            assert!(admit(&mut reducer, frames).is_empty());
            delivered.extend(reducer.drain_reports().into_iter().map(|report| report.state.buttons));
        }
    }
    assert_eq!(delivered, expected);
    let fresh = capture.registry.lock().unwrap().prepare_observation(&broker, &id).unwrap();
    let mut reopened = fresh.with_subscription(|| {
        broker.subscribe("reopened-preview", "ui", &topic).map_err(|error| error.to_string())
    }).unwrap();
    let latest_preview = reopened.recv().unwrap();
    assert_eq!(latest_preview.kind, "snapshot");
    let sequence = latest_preview.payload.unwrap()["source"]["sequence"].clone();
    let snapshot = capture.snapshot(&[id.clone()]).unwrap();
    assert_eq!(sequence, snapshot.sources[0].baseline.sequence.to_string());
    drop(stalled);

    report(1, 0);
    admit(&mut reducer, reader.poll(&capture, |_| Ok(())).unwrap().0);
    assert!(reducer.drain_reports().last().unwrap().state.buttons[0]);
    for index in 0..=stream::REPORT_LIMIT { report(1, (index % 2) as u32); }
    report(0, 0);
    let (frames, errors) = reader.poll(&capture, |_| Ok(())).unwrap();
    assert!(errors.is_empty());
    assert_eq!(frames.iter().map(|frame| &frame.reason).collect::<Vec<_>>(), vec![&Reason::Gap, &Reason::Baseline]);
    assert!(!admit(&mut reducer, frames).is_empty());
    let reports = reducer.drain_reports();
    assert!(reports.iter().any(|report| report.reason == ReportReason::SourceLost));
    assert!(reports.iter().all(|report| report.state.buttons == [false, false]));
    report(0, 1);
    admit(&mut reducer, reader.poll(&capture, |_| Ok(())).unwrap().0);
    assert_eq!(reducer.drain_reports().last().unwrap().state.buttons, [false, true]);
}

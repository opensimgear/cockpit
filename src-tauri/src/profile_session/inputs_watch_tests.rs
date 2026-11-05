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

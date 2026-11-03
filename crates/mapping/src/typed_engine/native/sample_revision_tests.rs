use super::super::*;

fn endpoint(node: &str, port: &str) -> Endpoint { Endpoint {node_id:node.into(),port_id:port.into()} }
fn pressed(value: bool) -> SignalSample { SignalSample::scalar(PortValue::Boolean(value)) }
fn phase() -> Phase {
    let contract = PortContract::State {schema:ScalarSchema::Boolean {},freshness_ms:1000,fallback:None};
    Phase::new(Program {sources:vec![SourcePort {endpoint:endpoint("source","value"),contract}],
        nodes:vec![],sinks:vec![],controllers:vec![]},Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into()).unwrap()
}
fn feed(engine: &mut Phase, sequence: u64, at: u64, sample: SignalSample) {
    engine.accept(Frame {source:"source".into(),generation:Counter(1),sequence:Counter(sequence),captured_us:Counter(at),
        reason:if sequence == 1 {Reason::Baseline}else{Reason::Change},
        changes:vec![Change {port_id:"value".into(),sample}]},Counter(at)).unwrap();
}

#[test]
fn value_revisions_ignore_idle_clocks_same_values_and_liveness() {
    let mut engine = phase();
    assert_eq!(engine.sample_revision(), Some(1));
    assert_eq!(engine.sample_revision_us(), Counter(0));
    engine.advance(Counter(10)).unwrap();
    assert_eq!(engine.sample_revision(), Some(1));
    feed(&mut engine, 1, 20, pressed(false));
    let revision = engine.sample_revision();
    assert!(revision.unwrap() > 1);
    assert_eq!(engine.sample_revision_us(), Counter(20));
    engine.observe_liveness("source", Counter(1), Counter(30)).unwrap();
    engine.advance(Counter(30)).unwrap();
    feed(&mut engine, 2, 40, pressed(false));
    assert_eq!(engine.sample_revision(), revision);
    assert_eq!(engine.sample_revision_us(), Counter(20));
    assert_eq!(engine.inspected_sample(&endpoint("source", "value")).unwrap().1, Some(Counter(40)));
}

#[test]
fn change_expiry_and_stop_advance_values_and_terminal_change_only_once() {
    let mut engine = phase();
    feed(&mut engine, 1, 10, pressed(false));
    let baseline = engine.sample_revision().unwrap();
    feed(&mut engine, 2, 20, pressed(true));
    assert!(engine.sample_revision().unwrap() > baseline);
    let changed = engine.sample_revision().unwrap();
    engine.advance(Counter(1_000_020)).unwrap();
    assert!(engine.sample_revision().unwrap() > changed);
    assert_eq!(engine.sample_revision_us(), Counter(1_000_020));
    assert_eq!(engine.sample(&endpoint("source", "value")), Some(&SignalSample::Stale));
    let expired = engine.sample_revision().unwrap();
    engine.stop();
    assert_eq!(engine.sample_revision(), Some(expired + 1));
    engine.stop();
    assert_eq!(engine.sample_revision(), Some(expired + 1));
}

#[test]
fn revision_exhaustion_is_permanent_and_does_not_fault_or_stop_mapping() {
    let mut engine = phase();
    engine.sample_revision = Some(u64::MAX);
    feed(&mut engine, 1, 10, pressed(false));
    assert_eq!(engine.sample_revision(), None);
    assert!(engine.active());
    assert_eq!(engine.fault(), None);
    feed(&mut engine, 2, 20, pressed(true));
    assert_eq!(engine.sample_revision(), None);
    assert_eq!(engine.sample(&endpoint("source", "value")), Some(&pressed(true)));
    assert!(engine.active());
}

#[test]
fn output_overflow_marks_its_direct_terminal_value_change_once() {
    let mut engine = phase();
    feed(&mut engine, 1, 10, pressed(true));
    let revision = engine.sample_revision().unwrap();
    for sequence in 0..=MAX_REPORTS {
        engine.reports.push_back(Report {session:engine.run.session.clone(),run_generation:Counter(1),
            sequence:Counter(sequence as u64),at_us:Counter(10),target:"target".into(),reason:ReportReason::Start,
            state:TargetState {buttons:vec![],axes:vec![],hats:vec![]}});
    }
    engine.publish(BTreeSet::new(), ReportReason::Change, false);
    assert_eq!(engine.sample_revision(), Some(revision + 1));
    assert_eq!(engine.sample(&endpoint("source", "value")), Some(&SignalSample::Unavailable));
    assert!(!engine.active());
    engine.stop();
    assert_eq!(engine.sample_revision(), Some(revision + 1));
}

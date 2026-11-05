use super::super::super::*;
use crate::signal::{Direction, PortDescriptor};

fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint { node_id: node.into(), port_id: port.into() }
}

fn contract() -> PortContract {
    PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 100,
        fallback: None,
    }
}

fn source_phase() -> Phase {
    Phase::new(
        Program {
            sources: vec![SourcePort { endpoint: endpoint("source", "value"), contract: contract() }],
            nodes: vec![],
            sinks: vec![],
            controllers: vec![],
        },
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    ).unwrap()
}

fn identity_phase() -> Phase {
    Phase::new(
        Program {
            sources: vec![SourcePort { endpoint: endpoint("source", "value"), contract: contract() }],
            nodes: vec![Node {
                id: "identity".into(),
                inputs: vec![InputBinding {
                    port_id: "input".into(),
                    from: endpoint("source", "value"),
                    contract: contract(),
                    edge_id: "edge-source-value-identity-input".into(),
                }],
                outputs: vec![PortDescriptor {
                    id: "value".into(),
                    name: "Value".into(),
                    direction: Direction::Output,
                    contract: contract(),
                }],
                operation: Operation::Identity {},
            }],
            sinks: vec![],
            controllers: vec![],
        },
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    ).unwrap()
}

fn feed(phase: &mut Phase, sequence: u64, at: u64, sample: SignalSample) {
    phase.accept(Frame {
        source: "source".into(),
        generation: Counter(1),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason: if sequence == 1 { Reason::Baseline } else { Reason::Change },
        changes: vec![Change { port_id: "value".into(), sample }],
    }, Counter(at)).unwrap();
}

fn known(value: bool) -> SignalSample {
    SignalSample::scalar(PortValue::Boolean(value))
}

#[test]
fn same_value_capture_advances_capture_evidence_without_advancing_value_revision() {
    let mut phase = source_phase();
    let endpoint = endpoint("source", "value");
    let revision_before = phase.sample_revision();
    let initial = phase.inspected_observation(&endpoint).unwrap();
    assert_eq!(initial.sample, &SignalSample::Unknown);
    assert_eq!(initial.observed_at, None);
    assert_eq!(initial.captured_at, None);

    feed(&mut phase, 1, 10, known(false));
    let first_revision = phase.sample_revision();
    let first = phase.inspected_observation(&endpoint).unwrap();
    assert_eq!(first.observed_at, Some(Counter(10)));
    assert_eq!(first.captured_at, Some(Counter(10)));
    assert_eq!(first.captured_sequence, Counter(1));

    feed(&mut phase, 2, 20, known(false));
    let repeated = phase.inspected_observation(&endpoint).unwrap();
    assert_eq!(phase.sample_revision(), first_revision);
    assert_eq!(repeated.observed_at, Some(Counter(20)));
    assert_eq!(repeated.captured_at, Some(Counter(20)));
    assert_eq!(repeated.captured_sequence, Counter(2));
    assert_eq!(revision_before, Some(1));
}

#[test]
fn liveness_advances_observation_but_preserves_actual_capture_metadata() {
    let mut phase = source_phase();
    let endpoint = endpoint("source", "value");
    feed(&mut phase, 1, 10, known(true));
    let revision = phase.sample_revision();
    phase.observe_liveness("source", Counter(1), Counter(30)).unwrap();

    let observation = phase.inspected_observation(&endpoint).unwrap();
    assert_eq!(observation.sample, &known(true));
    assert_eq!(observation.observed_at, Some(Counter(30)));
    assert_eq!(observation.captured_at, Some(Counter(10)));
    assert_eq!(observation.captured_sequence, Counter(1));
    assert!(!observation.timeless);
    assert_eq!(phase.sample_revision(), revision);
}

#[test]
fn derived_sample_tracks_graph_evaluations_but_not_liveness_as_capture() {
    let mut phase = identity_phase();
    let derived = endpoint("identity", "value");
    feed(&mut phase, 1, 10, known(true));
    let initial = phase.inspected_observation(&derived).unwrap();
    assert_eq!(initial.sample, &known(true));
    assert_eq!(initial.captured_at, Some(Counter(10)));

    phase.observe_liveness("source", Counter(1), Counter(30)).unwrap();
    let live = phase.inspected_observation(&derived).unwrap();
    assert_eq!(live.observed_at, Some(Counter(30)));
    assert_eq!(live.captured_at, Some(Counter(10)));
    assert_eq!(live.captured_sequence, Counter(0));

    feed(&mut phase, 2, 40, known(true));
    let repeated = phase.inspected_observation(&derived).unwrap();
    assert_eq!(repeated.sample, &known(true));
    assert_eq!(repeated.captured_at, Some(Counter(40)));
    assert_eq!(repeated.captured_sequence, Counter(0));
}

#[test]
fn constant_observations_are_timeless_across_clock_advances() {
    let endpoint = endpoint("constant", "value");
    let mut phase = Phase::new(
        Program {
            sources: vec![],
            nodes: vec![Node {
                id: endpoint.node_id.clone(),
                inputs: vec![],
                outputs: vec![PortDescriptor {
                    id: endpoint.port_id.clone(),
                    name: "Value".into(),
                    direction: Direction::Output,
                    contract: contract(),
                }],
                operation: Operation::Constant { value: PortValue::Boolean(true) },
            }],
            sinks: vec![],
            controllers: vec![],
        },
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    ).unwrap();
    let initial = phase.inspected_observation(&endpoint).unwrap();
    assert_eq!(initial.sample, &known(true));
    assert!(initial.timeless);
    assert_eq!(initial.captured_at, Some(Counter(0)), "The constant retains its initial graph evaluation stamp");
    let revision = phase.sample_revision();
    phase.advance(Counter(50)).unwrap();
    let later = phase.inspected_observation(&endpoint).unwrap();
    assert_eq!(later.sample, &known(true));
    assert!(later.timeless);
    assert_eq!(phase.sample_revision(), revision);
}

#[test]
fn unavailable_unknown_and_expired_samples_keep_their_quality_and_evidence() {
    let endpoint = endpoint("source", "value");

    let mut unknown = source_phase();
    feed(&mut unknown, 1, 10, SignalSample::Unknown);
    let observation = unknown.inspected_observation(&endpoint).unwrap();
    assert_eq!(observation.sample, &SignalSample::Unknown);
    assert_eq!(observation.observed_at, Some(Counter(10)));
    assert_eq!(observation.captured_at, Some(Counter(10)));
    assert_eq!(observation.captured_sequence, Counter(1));

    let mut unavailable = source_phase();
    feed(&mut unavailable, 1, 10, known(false));
    feed(&mut unavailable, 2, 20, SignalSample::Unavailable);
    let observation = unavailable.inspected_observation(&endpoint).unwrap();
    assert_eq!(observation.sample, &SignalSample::Unavailable);
    assert_eq!(observation.observed_at, Some(Counter(20)));
    assert_eq!(observation.captured_at, Some(Counter(20)));
    assert_eq!(observation.captured_sequence, Counter(2));

    let mut stale = source_phase();
    feed(&mut stale, 1, 10, known(true));
    stale.advance(Counter(100_010)).unwrap();
    let observation = stale.inspected_observation(&endpoint).unwrap();
    assert_eq!(observation.sample, &SignalSample::Stale);
    assert_eq!(observation.observed_at, Some(Counter(100_010)));
    assert_eq!(observation.captured_at, Some(Counter(100_010)));
    assert_eq!(observation.captured_sequence, Counter(1));
}

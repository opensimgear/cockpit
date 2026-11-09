use super::*;
use osg_controller_input::registry::Sample;

fn event(sequence: u64, read: Option<u64>, pressed: bool) -> Event {
    Event {
        cursor: sequence + 20,
        source: "device".into(),
        generation: 3,
        sequence,
        captured_us: sequence * 10 + 5,
        native_read_us: read,
        reason: Reason::Change,
        values: [("button".into(), Sample {
            native: i64::from(pressed),
            normalized: Some(i64::from(pressed)),
        })].into(),
    }
}

fn trace() -> Trace {
    Trace { generation: 3, sequence: 10, last_read_us: 0, pressed: false, edges: vec![] }
}

#[test]
fn ordered_native_reports_emit_each_changed_level_and_ignore_repeats() {
    let mut trace = trace();
    for (sequence, level) in [(11, false), (12, true), (13, true), (14, false)] {
        trace.advance(event(sequence, Some(sequence * 10), level), "button", 100).unwrap();
    }
    assert_eq!(trace.sequence, 14);
    assert_eq!(trace.edges.len(), 2);
    assert_eq!(trace.edges[0].native_read_us, 120);
    assert_eq!(trace.edges[0].captured_us, 125);
    assert!(trace.edges[0].pressed);
    assert!(!trace.edges[1].pressed);
}

#[test]
fn trace_rejects_loss_replacement_missing_or_unordered_native_time() {
    let mut replaced = event(11, Some(110), true);
    replaced.generation += 1;
    let mut lost = event(11, Some(110), true);
    lost.reason = Reason::Disconnect;
    for report in [
        replaced, lost, event(12, Some(110), true), event(11, None, true),
        event(11, Some(116), true), event(11, Some(99), true),
    ] {
        assert!(trace().advance(report, "button", 100).is_err());
    }
    let mut trace = trace();
    trace.advance(event(11, Some(110), false), "button", 100).unwrap();
    assert!(trace.advance(event(12, Some(109), true), "button", 100).is_err());
    assert!(pressed(&event(11, Some(110), false), "other-button").is_err());
}

#[test]
fn trace_rejects_too_many_edges() {
    let mut trace = trace();
    for index in 0..MAX_EDGES as u64 {
        let sequence = 11 + index;
        trace.advance(event(sequence, Some(sequence * 10), index % 2 == 0), "button", 100).unwrap();
    }
    let sequence = 11 + MAX_EDGES as u64;
    assert!(trace.advance(event(sequence, Some(sequence * 10), true), "button", 100).is_err());
    assert_eq!(trace.edges.len(), MAX_EDGES);
}

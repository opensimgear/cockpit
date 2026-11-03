//! Admission time schedules authored work; capture time qualifies its input.
use super::*;

fn captured(sequence: u64, at: u64, reason: Reason, value: bool) -> Frame {
    Frame {
        source: "source".into(),
        generation: Counter(1),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: if matches!(reason, Reason::Gap | Reason::Disconnect) {
            vec![]
        } else {
            vec![Change { port_id: "value".into(), sample: known(value) }]
        },
    }
}

#[test]
fn delayed_pulse_starts_at_admission_and_retains_original_capture_provenance() {
    let mut phase = engine(true);
    phase.accept(captured(1, 0, Reason::Baseline, false), Counter(0)).unwrap();
    phase.drain();
    phase.accept(captured(2, 10, Reason::Change, true), Counter(5000)).unwrap();
    let effects = phase.drain();
    let press = effects.iter().find(|effect| effect.sample == known(true)).unwrap();
    assert_eq!(press.at_us, Counter(5000));
    let observations = phase.source_observations(&phase.token("processor").unwrap());
    assert_eq!(observations[0].at_us, Counter(10));
    assert_eq!(observations[0].sequence, Counter(2));
    assert_eq!(phase.next_deadline_us(), Some(Counter(6000)));
    phase.advance(Counter(5999)).unwrap();
    assert_eq!(phase.sample(&endpoint("processor")), Some(&known(true)));
    phase.advance(Counter(6000)).unwrap();
    assert_eq!(phase.sample(&endpoint("processor")), Some(&known(false)));
}

#[test]
fn delayed_script_invocation_and_its_timer_use_current_admission() {
    let mut program = engine(false).program().clone();
    program.nodes[0].operation = Operation::Script {
        source: "fn on_start(ctx) { ctx } fn on_event(ctx) { ctx }".into(),
        parameters: BTreeMap::new(),
    };
    program.sinks.clear();
    let mut phase = Phase::new(program, Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    phase.accept(captured(1, 0, Reason::Baseline, false), Counter(0)).unwrap();
    let start = phase.script_requests().pop().unwrap();
    phase.script_reply("processor", &start.ticket, Default::default()).unwrap();
    phase.accept(captured(2, 10, Reason::Change, true), Counter(5000)).unwrap();
    let request = phase.script_requests().pop().unwrap();
    assert_eq!(request.callback, "on_event");
    assert_eq!(request.at_us, Counter(5000));
    phase.script_reply("processor", &request.ticket, ScriptTransaction {
        timers: vec![ScriptTimer { id: "later".into(), after_us: Counter(1000), outputs: BTreeMap::new() }],
        ..Default::default()
    }).unwrap();
    assert_eq!(phase.next_deadline_us(), Some(Counter(6000)));
    phase.advance(Counter(5999)).unwrap();
    assert_eq!(phase.timers.len(), 1);
    phase.advance(Counter(6000)).unwrap();
    assert!(phase.timers.is_empty());
    assert_eq!(phase.values[&endpoint("source")].captured, 10);
}

#[test]
fn stale_backlog_revokes_before_due_timer_without_a_transient_known_output() {
    for batch in [false, true] {
        let mut phase = engine(false);
        phase.accept(captured(1, 1_000_000, Reason::Baseline, false), Counter(1_000_000)).unwrap();
        timer(&mut phase, "processor", "dependent", 2_500_000);
        timer(&mut phase, "constant", "independent", 3_000_001);
        // Current connection evidence keeps the previous value alive. The
        // delayed replacement still owns its older, already expired capture.
        phase.observe_liveness("source", Counter(1), Counter(3_000_000)).unwrap();
        phase.drain();
        let frame = captured(2, 1_000_001, Reason::Change, true);
        if batch {
            phase.accept_batch([frame], Counter(3_000_000)).unwrap();
        } else {
            phase.accept(frame, Counter(3_000_000)).unwrap();
        }
        assert_eq!(phase.sample(&endpoint("source")), Some(&SignalSample::Stale));
        assert_eq!(phase.sample(&endpoint("processor")), Some(&SignalSample::Stale));
        assert_eq!(phase.next_deadline_us(), Some(Counter(3_000_001)));
        assert!(phase.drain().iter().all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
        assert_eq!(phase.timers.len(), 1);
        assert_eq!(phase.values[&endpoint("source")].captured, 1_000_001);
        phase.advance(Counter(3_000_001)).unwrap();
        assert!(phase.timers.is_empty());
    }
}

#[test]
fn current_admission_installs_old_loss_before_due_work() {
    let mut phase = engine(false);
    phase.accept(captured(1, 0, Reason::Baseline, false), Counter(0)).unwrap();
    timer(&mut phase, "processor", "dependent", 5000);
    timer(&mut phase, "constant", "independent", 5001);
    phase.drain();
    phase.accept_batch([captured(2, 10, Reason::Disconnect, false)], Counter(5000)).unwrap();
    assert_eq!(phase.sample(&endpoint("processor")), Some(&SignalSample::Unavailable));
    assert_eq!(phase.next_deadline_us(), Some(Counter(5001)));
    assert!(phase.drain().iter().all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
    phase.advance(Counter(5001)).unwrap();
    assert!(phase.timers.is_empty());
}

#[test]
fn malformed_capture_or_admission_cannot_change_clock_samples_or_pending_work() {
    for (capture, admission) in [(101, 100), (9, 9), (100, u64::MAX)] {
        for batch in [false, true] {
            let mut phase = engine(true);
            phase.accept(captured(1, 10, Reason::Baseline, false), Counter(10)).unwrap();
            timer(&mut phase, "processor", "pending", 50);
            let before = (phase.at_us(), phase.effects.len(), phase.reports.len(), phase.timers.len(), phase.sources["source"].sequence);
            let frame = captured(2, capture, Reason::Change, true);
            let rejected = if batch {
                phase.accept_batch([frame], Counter(admission)).map(|_| ())
            } else {
                phase.accept(frame, Counter(admission))
            };
            assert!(rejected.is_err(), "capture={capture}, admission={admission}, batch={batch}");
            assert_eq!((phase.at_us(), phase.effects.len(), phase.reports.len(), phase.timers.len(), phase.sources["source"].sequence), before);
            assert_eq!(phase.sample(&endpoint("source")), Some(&known(false)));
        }
    }
}

#[test]
fn expired_wrong_type_and_out_of_range_values_cannot_hide_as_stale() {
    for numeric in [false, true] {
        for batch in [false, true] {
            let mut program = engine(false).program().clone();
            if numeric {
                let contract = PortContract::State {
                    schema: ScalarSchema::Number { min: 0.0, max: 10.0, unit: None },
                    freshness_ms: 1000,
                    fallback: None,
                };
                program.sources[0].contract = contract.clone();
                program.nodes[0].inputs[0].contract = contract.clone();
                program.nodes[0].outputs[0].contract = contract.clone();
                program.sinks[0].contract = contract;
            }
            let mut phase = Phase::new(program, Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
            let mut baseline = captured(1, 0, Reason::Baseline, false);
            if numeric { baseline.changes[0].sample = SignalSample::scalar(PortValue::Number(0.0)); }
            phase.accept(baseline, Counter(0)).unwrap();
            timer(&mut phase, "constant", "independent", 3_000_000);
            phase.drain();
            let mut malformed = captured(2, 1, Reason::Change, true);
            malformed.changes[0].sample = SignalSample::scalar(PortValue::Number(if numeric { 11.0 } else { 1.0 }));
            let rejected = if batch {
                phase.accept_batch([malformed], Counter(2_000_000)).map(|_| ())
            } else {
                phase.accept(malformed, Counter(2_000_000))
            };
            assert!(rejected.is_err(), "numeric={numeric}, batch={batch}");
            assert_eq!(phase.at_us(), Counter(0));
            assert_eq!(phase.sources["source"].sequence, 1);
            assert_eq!(phase.sample(&endpoint("source")), Some(&SignalSample::Unknown));
            assert!(phase.drain().iter().all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
            assert_eq!(phase.timers.len(), 1);
        }
    }
}

#[test]
fn retained_baseline_query_cannot_rearm_lost_or_other_generation_sources() {
    let mut phase = engine(false);
    assert!(!phase.has_source_baseline("source", Counter(1)));
    assert!(!phase.has_source_baseline("missing", Counter(1)));
    phase.accept(captured(1, 0, Reason::Baseline, false), Counter(0)).unwrap();
    assert!(phase.has_source_baseline("source", Counter(1)));
    assert!(!phase.has_source_baseline("source", Counter(2)));
    phase.accept(captured(2, 10, Reason::Gap, false), Counter(10)).unwrap();
    assert!(!phase.has_source_baseline("source", Counter(1)));
    assert!(phase.observe_liveness("source", Counter(1), Counter(10)).is_err());
    let mut replacement = captured(1, 20, Reason::Baseline, false);
    replacement.generation = Counter(2);
    phase.accept(replacement, Counter(20)).unwrap();
    assert!(!phase.has_source_baseline("source", Counter(1)));
    assert!(phase.has_source_baseline("source", Counter(2)));
}

#[test]
fn queued_changes_keep_occurrence_order_at_one_current_admission() {
    let mut phase = engine(false);
    phase.accept(captured(1, 0, Reason::Baseline, false), Counter(0)).unwrap();
    phase.drain();
    phase.accept_batch([
        captured(2, 10, Reason::Change, true),
        captured(3, 20, Reason::Change, false),
        captured(4, 30, Reason::Change, true),
    ], Counter(5000)).unwrap();
    let effects = phase.drain();
    assert_eq!(effects.iter().map(|effect| effect.sample.clone()).collect::<Vec<_>>(), vec![known(true), known(false), known(true)]);
    assert!(effects.iter().all(|effect| effect.at_us == Counter(5000)));
    assert_eq!(phase.values[&endpoint("source")].captured, 30);
    assert_eq!(phase.values[&endpoint("source")].captured_sequence, 4);
    assert_eq!(phase.next_deadline_us(), Some(Counter(1_000_030)));
}

use super::*;

#[test]
fn genuine_lease_recovery_restores_retained_level_as_baseline_and_never_replays_held_press() {
    let mut phase = phase(false, NativePolicy::Latched);
    admit(
        &mut phase,
        latched(baseline(1, 1, false), 1, 1, 1_000_000),
        1,
    );
    admit(&mut phase, latched(update(2, 2, true), 2, 2, 1_000_000), 2);
    phase.advance(Counter(100_002)).unwrap();
    assert_eq!(phase.sample(&ep("game", "x")), Some(&SignalSample::Stale));
    phase.drain();
    admit(
        &mut phase,
        latched(frame(3, &[]), 3, 150_000, 300_000),
        150_000,
    );
    assert_eq!(phase.sample(&ep("game", "x")), Some(&value(true)));
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(true)));
    assert_eq!(phase.sample(&ep("pulse", "value")), Some(&value(false)));
    assert!(phase.states["pulse"].rule.next().is_none());
    assert_eq!(phase.values[&ep("game", "x")].captured, 2);
    assert!(phase
        .drain()
        .iter()
        .all(|effect| effect.reason == ReportReason::Baseline || effect.sample != value(true)));
    assert_eq!(phase.next_deadline_us(), Some(Counter(250_000)));
}

#[test]
fn direct_only_admission_keeps_large_native_fence_storage_in_place() {
    let mut phase = phase(false, NativePolicy::Latched);
    admit(
        &mut phase,
        latched(baseline(1, 1, false), 1, 1, 1_000_000),
        1,
    );
    for input in 0..10_000 {
        phase.native.inputs.insert(
            ("producer".into(), 1, NativeInput::Byte(input)),
            capture(1, 1),
        );
    }
    let key = ("producer".into(), 1, NativeInput::Channel("x".into()));
    let original = phase.native.inputs[&key].producer_id.as_ptr();
    phase
        .accept_admissions([direct(1, Reason::Baseline, Some(false), 1)], Counter(1))
        .unwrap();
    let started = std::time::Instant::now();
    for sequence in 2..102 {
        phase
            .accept_admissions(
                [direct(
                    sequence,
                    Reason::Change,
                    Some(sequence % 2 == 0),
                    sequence,
                )],
                Counter(sequence),
            )
            .unwrap();
        assert_eq!(phase.native.inputs[&key].producer_id.as_ptr(), original);
    }
    assert_eq!(phase.native.inputs.len(), 10_001);
    eprintln!(
        "100 Direct admissions with 10k Native input fences: {:?}",
        started.elapsed()
    );
}

#[test]
fn direct_only_drain_preexpires_native_authority_before_running_due_authored_work() {
    let mut phase = phase(false, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    phase
        .schedule(
            phase.token("mapped").unwrap(),
            "old".into(),
            Counter(50_000),
            BTreeMap::from([(
                "value".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    phase.drain();
    phase
        .accept_admissions(
            [direct(1, Reason::Baseline, Some(false), 1)],
            Counter(110_000),
        )
        .unwrap();
    assert_eq!(phase.sample(&ep("game", "x")), Some(&SignalSample::Stale));
    assert!(phase
        .drain()
        .iter()
        .all(|effect| effect.reason != ReportReason::Timer));
}

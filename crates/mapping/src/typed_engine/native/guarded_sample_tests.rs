use super::*;

fn guarded(
    mut frame: NativeFrame,
    guard: NativeCapture,
    guard_deadline: u64,
    sample_deadline: u64,
) -> NativeFrame {
    frame.leases.push(NativeLease {
        domain_id: "session-guard".into(),
        generation: Counter(1),
        observation: frame.captures.len() as u32,
        valid_until_us: Counter(guard_deadline),
    });
    frame.captures.push(guard);
    for contributor in frame
        .changes
        .iter_mut()
        .flat_map(|change| &mut change.contributors)
    {
        contributor.validity = NativeValidity::GuardedSampled {
            valid_until_us: Counter(sample_deadline),
            lease: 0,
        };
    }
    frame
}

#[test]
fn older_guard_renews_without_resampling_or_extending_component_freshness() {
    let mut phase = phase(true, NativePolicy::Sampled);
    let input = ep("game", "x");
    // The guard belongs to an earlier acquisition than the dataref.
    let initial = guarded(
        frame(
            1,
            &[
                ("x", NativeChangeKind::Baseline, 1, Some(false), 2, 20),
                ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
            ],
        ),
        capture(1, 10),
        70,
        150_000,
    );
    admit(&mut phase, initial, 20);
    let request = phase.script_requests().pop().unwrap();
    phase
        .script_reply("script", &request.ticket, ScriptTransaction::default())
        .unwrap();
    let token = phase.token("pulse").unwrap();
    let original = phase.values[&input].proof.clone();
    phase.drain();
    assert_eq!(phase.next_deadline_us(), Some(Counter(70)));

    let renewal = guarded(frame(2, &[]), capture(3, 60), 300_000, 0);
    admit(&mut phase, renewal, 60);
    assert_eq!(phase.next_deadline_us(), Some(Counter(100_020)));
    assert_eq!(phase.values[&input].proof, original);
    assert_eq!(phase.token("pulse").unwrap(), token);
    assert!(phase.script_requests().is_empty());
    assert!(phase.drain().is_empty());

    // Reprojecting the same component with a new guard is also idempotent.
    let retained = guarded(
        frame(3, &[("x", NativeChangeKind::Update, 1, Some(false), 2, 20)]),
        capture(4, 80),
        310_000,
        150_000,
    );
    admit(&mut phase, retained, 80);
    assert!(phase.script_requests().is_empty());
    assert!(phase.drain().is_empty());
    let sample = phase.inspected_observation(&input).unwrap();
    assert_eq!(sample.sample, &value(false));
    assert_eq!(sample.captured_at, Some(Counter(20)));
    assert_eq!(sample.captured_sequence, Counter(2));
    phase.advance(Counter(100_020)).unwrap();
    assert_eq!(phase.sample(&input), Some(&SignalSample::Stale));

    let late = guarded(frame(4, &[]), capture(5, 110_000), 350_000, 0);
    admit(&mut phase, late, 110_000);
    assert_eq!(phase.sample(&input), Some(&SignalSample::Stale));
}

#[test]
fn guard_expiry_withdraws_a_still_fresh_component() {
    let mut phase = phase(false, NativePolicy::Sampled);
    let guarded = guarded(baseline(1, 20, true), capture(2, 25), 60, 100);
    admit(&mut phase, guarded, 25);
    phase.advance(Counter(59)).unwrap();
    assert_eq!(phase.sample(&ep("game", "x")), Some(&value(true)));
    phase.advance(Counter(60)).unwrap();
    assert_eq!(phase.sample(&ep("game", "x")), Some(&SignalSample::Stale));
}

#[test]
fn mixed_sample_expires_with_its_retained_component_after_guard_renewal() {
    let mut phase = phase(false, NativePolicy::Sampled);
    let retained_component = |mut frame: NativeFrame| {
        let index = frame.captures.len() as u32;
        frame.captures.push(capture(3, 30));
        frame.changes[0].contributors.push(NativeContributor {
            input: NativeInput::Channel("retained-component".into()),
            capture: index,
            validity: NativeValidity::GuardedSampled {
                valid_until_us: Counter(120),
                lease: 0,
            },
        });
        frame
    };
    let initial = retained_component(guarded(
        frame(
            1,
            &[
                ("x", NativeChangeKind::Baseline, 1, Some(false), 2, 20),
                ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
            ],
        ),
        capture(1, 10),
        60,
        100,
    ));
    admit(&mut phase, initial, 30);
    let refreshed = retained_component(guarded(
        frame(2, &[("x", NativeChangeKind::Update, 1, Some(true), 5, 55)]),
        capture(4, 50),
        140,
        145,
    ));
    admit(&mut phase, refreshed, 55);
    let input = ep("game", "x");
    let sample = phase.inspected_observation(&input).unwrap();
    assert_eq!(sample.sample, &value(true));
    assert_eq!(sample.captured_at, Some(Counter(30)));
    assert_eq!(sample.captured_sequence, Counter(3));
    phase.advance(Counter(120)).unwrap();
    assert_eq!(phase.sample(&input), Some(&SignalSample::Stale));
}

#[test]
fn guarded_samples_require_the_same_acquisition_domain_and_fixed_deadline() {
    for guard in [
        NativeCapture {
            producer_id: "other".into(),
            ..capture(2, 25)
        },
        NativeCapture {
            generation: Counter(2),
            ..capture(2, 25)
        },
        NativeCapture {
            byte_offset: Some(0),
            ..capture(2, 25)
        },
    ] {
        let mut phase = phase(false, NativePolicy::Sampled);
        let initial = guarded(baseline(1, 20, true), guard, 60, 150_000);
        assert!(phase
            .accept_admissions([initial.into()], Counter(25))
            .is_err());
    }

    let initial = guarded(baseline(1, 20, true), capture(2, 25), 60, 150_000);
    assert!(phase(false, NativePolicy::Latched)
        .accept_admissions([initial.clone().into()], Counter(25))
        .is_err());
    let mut phase = phase(false, NativePolicy::Sampled);
    admit(&mut phase, initial, 25);
    let renewal = guarded(frame(2, &[]), capture(2, 25), 61, 0);
    assert!(phase
        .accept_admissions([renewal.into()], Counter(26))
        .is_err());

    let changed = guarded(
        frame(2, &[("x", NativeChangeKind::Update, 1, Some(true), 1, 20)]),
        capture(3, 26),
        62,
        150_001,
    );
    assert!(phase
        .accept_admissions([changed.into()], Counter(26))
        .is_err());
    assert_eq!(phase.next_deadline_us(), Some(Counter(60)));
}

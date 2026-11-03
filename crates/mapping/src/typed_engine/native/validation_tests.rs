use super::*;

#[test]
fn malformed_clock_refs_conflicts_future_and_regression_are_atomic() {
    for case in 0..8 {
        let mut phase = phase(false, NativePolicy::Sampled);
        admit(&mut phase, baseline(1, 10, false), 10);
        let mut bad = update(2, 11, true);
        match case {
            0 => bad.clock_id = "another-clock".into(),
            1 => bad.changes[0].contributors[0].capture = 9,
            2 => bad.captures.push(bad.captures[0].clone()),
            3 => bad.captures[0].captured_us = Counter(30),
            4 => {
                bad.captures[0].sequence = Counter(1);
                bad.captures[0].captured_us = Counter(11);
            }
            5 => bad.captures[0].sequence = Counter(0),
            6 => bad.captures[0].captured_us = Counter(0),
            _ => bad.changes[0].contributors[0].validity = NativeValidity::Latched { lease: 0 },
        }
        assert!(
            phase.accept_admissions([bad.into()], Counter(20)).is_err(),
            "case {case}"
        );
        assert_eq!(phase.at_us(), Counter(10));
        assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(false)));
        assert_eq!(phase.native.sources["game"], (1, 1));
    }
}

#[test]
fn per_input_regression_allows_retained_other_channel_and_original_byte_offsets() {
    let mut phase = phase(false, NativePolicy::Sampled);
    let mut first = frame(
        1,
        &[
            ("x", NativeChangeKind::Baseline, 1, Some(false), 12, 12),
            ("y", NativeChangeKind::Baseline, 1, Some(false), 8, 8),
        ],
    );
    first.changes[0].contributors[0].input = NativeInput::Byte(1);
    first.changes[1].contributors[0].input = NativeInput::Byte(2);
    first.captures[0].byte_offset = Some(10);
    first.captures[1].byte_offset = Some(2);
    admit(&mut phase, first, 12);
    let mut retained = frame(2, &[("y", NativeChangeKind::Update, 1, Some(false), 8, 8)]);
    retained.changes[0].contributors[0].input = NativeInput::Byte(2);
    retained.captures[0].byte_offset = Some(2);
    admit(&mut phase, retained.clone(), 13);
    retained.sequence = Counter(3);
    retained.captures[0].byte_offset = Some(1);
    assert!(phase
        .accept_admissions([retained.into()], Counter(14))
        .is_err());
}

#[test]
fn invalid_drain_cannot_install_a_valid_lease_renewal_ahead_of_validation() {
    let mut phase = phase(false, NativePolicy::Latched);
    admit(&mut phase, latched(baseline(1, 1, false), 1, 1, 200_000), 1);
    let renewal = latched(frame(2, &[]), 2, 90_000, 250_000);
    let mut bad = latched(update(3, 90_001, true), 3, 90_001, 250_000);
    bad.changes[0].contributors[0].capture = u32::MAX;
    assert!(phase
        .accept_admissions([renewal.into(), bad.into()], Counter(90_001))
        .is_err());
    assert_eq!(phase.next_deadline_us(), Some(Counter(100_001)));
    assert_eq!(phase.native.sources["game"], (1, 1));
}

#[test]
fn native_source_cannot_fallback_to_direct_and_prepared_unknown_cannot_gain_proof_by_update() {
    let mut phase = phase(false, NativePolicy::Sampled);
    let direct = Frame {
        source: "game".into(),
        generation: Counter(1),
        sequence: Counter(1),
        captured_us: Counter(1),
        reason: Reason::Baseline,
        changes: ["x", "y", "z"]
            .map(|port| Change {
                port_id: port.into(),
                sample: value(false),
            })
            .into(),
    };
    assert!(phase
        .accept_admissions([direct.into()], Counter(1))
        .is_err());
    admit(
        &mut phase,
        frame(1, &[("z", NativeChangeKind::Baseline, 1, None, 0, 0)]),
        1,
    );
    assert!(phase
        .accept_admissions(
            [frame(2, &[("z", NativeChangeKind::Update, 1, Some(false), 2, 2)]).into()],
            Counter(2)
        )
        .is_err());
}

#[test]
fn native_budgets_are_enforced_before_any_clock_or_authority_mutation() {
    let mut phase = phase(false, NativePolicy::Sampled);
    let mut huge = baseline(1, 1, false);
    huge.captures = vec![capture(1, 1); MAX_NATIVE_CAPTURES + 1];
    assert!(phase.accept_admissions([huge.into()], Counter(1)).is_err());
    assert_eq!(phase.at_us(), Counter(0));
    let mut huge = baseline(1, 1, false);
    huge.leases = vec![
        NativeLease {
            domain_id: "domain".into(),
            generation: Counter(1),
            observation: 0,
            valid_until_us: Counter(2)
        };
        MAX_NATIVE_LEASES + 1
    ];
    assert!(phase.accept_admissions([huge.into()], Counter(1)).is_err());
    let mut huge = baseline(1, 1, false);
    huge.changes[0].contributors =
        vec![huge.changes[0].contributors[0].clone(); MAX_NATIVE_REFERENCES + 1];
    assert!(phase.accept_admissions([huge.into()], Counter(1)).is_err());
    assert!(phase.native.sources.is_empty());
}

#[test]
fn logical_source_generation_replacement_requires_complete_closure_and_scopes_continuity() {
    let mut phase = phase(false, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    admit(
        &mut phase,
        frame(
            2,
            &[
                ("x", NativeChangeKind::Baseline, 2, Some(false), 2, 2),
                ("y", NativeChangeKind::Baseline, 2, None, 0, 0),
            ],
        ),
        2,
    );
    let mut replacement = frame(
        1,
        &[
            ("x", NativeChangeKind::Baseline, 1, Some(true), 3, 3),
            ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
        ],
    );
    replacement.generation = Counter(2);
    assert!(phase
        .accept_admissions([replacement.clone().into()], Counter(3))
        .is_err());
    replacement.changes.push(NativeChange {
        port_id: "z".into(),
        continuity: Counter(1),
        kind: NativeChangeKind::Baseline,
        sample: SignalSample::Unknown,
        contributors: vec![],
    });
    admit(&mut phase, replacement, 3);
    assert_eq!(phase.native.sources["game"], (2, 1));
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(true)));
    assert_eq!(phase.sample(&ep("pulse", "value")), Some(&value(false)));
}

#[test]
fn original_positions_include_live_async_proofs_and_every_prepared_frame_in_the_drain() {
    let mut phase = phase(true, NativePolicy::Sampled);
    let original = frame(
        1,
        &[
            ("x", NativeChangeKind::Baseline, 1, Some(false), 10, 10),
            ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
        ],
    );
    admit(&mut phase, original, 10);
    let request = phase.script_requests().pop().unwrap();
    admit(
        &mut phase,
        frame(2, &[("x", NativeChangeKind::Update, 1, Some(true), 20, 20)]),
        20,
    );
    let contradictory = frame(
        3,
        &[("y", NativeChangeKind::Update, 1, Some(false), 10, 11)],
    );
    assert!(phase
        .accept_admissions([contradictory.into()], Counter(21))
        .is_err());
    assert!(phase.script_ticket_current(&request.ticket));

    let mut phase = super::phase(false, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    let batch = [
        frame(2, &[("x", NativeChangeKind::Update, 1, Some(true), 20, 20)]),
        frame(
            3,
            &[("x", NativeChangeKind::Update, 1, Some(false), 30, 30)],
        ),
        frame(
            4,
            &[("y", NativeChangeKind::Update, 1, Some(false), 20, 21)],
        ),
    ];
    assert!(phase
        .accept_admissions(batch.map(Admission::from), Counter(31))
        .is_err());
    assert_eq!(phase.at_us(), Counter(1));
}

#[test]
fn family_rebase_prunes_historical_producers_but_withdraw_keeps_its_pinned_owner() {
    let mut phase = phase(false, NativePolicy::Sampled);
    for continuity in 1..20 {
        let mut rebase = frame(
            continuity,
            &[
                (
                    "x",
                    NativeChangeKind::Baseline,
                    continuity,
                    Some(false),
                    continuity,
                    continuity,
                ),
                ("y", NativeChangeKind::Baseline, continuity, None, 0, 0),
            ],
        );
        rebase.captures[0].producer_id = format!("producer-{continuity}");
        admit(&mut phase, rebase, continuity);
        assert_eq!(phase.native.producers.len(), 1);
        assert_eq!(phase.native.inputs.len(), 1);
    }
    admit(
        &mut phase,
        frame(
            20,
            &[
                ("x", NativeChangeKind::Withdraw, 19, None, 0, 0),
                ("y", NativeChangeKind::Withdraw, 19, None, 0, 0),
            ],
        ),
        20,
    );
    assert_eq!(phase.native.producers.len(), 1);
    admit(
        &mut phase,
        frame(
            21,
            &[
                ("x", NativeChangeKind::Baseline, 20, None, 0, 0),
                ("y", NativeChangeKind::Baseline, 20, None, 0, 0),
            ],
        ),
        21,
    );
    assert!(phase.native.producers.is_empty());
    assert!(phase.native.inputs.is_empty());
}

#[test]
fn latched_inputs_cannot_be_in_the_future_of_their_observation_or_hide_stamp_positions() {
    for case in 0..5 {
        let mut phase = phase(false, NativePolicy::Latched);
        let mut baseline = latched(baseline(1, 1, false), 1, 1, 100_000);
        match case {
            0 => {
                baseline.captures[0].sequence = Counter(100);
                baseline.captures.push(capture(1, 1));
                baseline.leases[0].observation = 1;
            }
            1 => baseline.changes[0].contributors[0].input = NativeInput::Byte(1),
            2 => baseline.captures[0].byte_offset = Some(1),
            3 => {
                baseline.changes[0].contributors[0].input = NativeInput::Byte(1);
                baseline.captures[0].byte_offset = Some(1);
                baseline.captures.push(capture(1, 1));
                baseline.leases[0].observation = 1;
            }
            _ => baseline.leases[0].valid_until_us = Counter(MAX_NATIVE_VALIDITY_US + 2),
        }
        assert!(phase
            .accept_admissions([baseline.into()], Counter(1))
            .is_err());
    }
}

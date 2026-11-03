use super::*;

fn reply(phase: &mut Phase, request: &ScriptInvocation, value: bool) -> Result<(), String> {
    phase.script_reply(
        "script",
        &request.ticket,
        ScriptTransaction {
            outputs: BTreeMap::from([(
                "value".into(),
                SignalValue::Scalar(PortValue::Boolean(value)),
            )]),
            ..Default::default()
        },
    )
}

#[test]
fn slow_fifo_invocations_keep_original_unexpired_bindings_across_newer_samples() {
    let mut phase = phase(true, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    let first = phase.script_requests().pop().unwrap();
    admit(&mut phase, update(2, 10, true), 10);
    admit(&mut phase, update(3, 20, false), 20);
    assert!(phase.script_ticket_current(&first.ticket));
    reply(&mut phase, &first, true).unwrap();
    assert_eq!(phase.sample(&ep("script", "value")), Some(&value(true)));
    let effect = phase
        .drain()
        .into_iter()
        .find(|effect| effect.sink_id == "script-sink" && effect.sample == value(true))
        .unwrap();
    assert_eq!(
        effect.observations,
        vec![Observation {
            endpoint: ep("game", "x"),
            at_us: Counter(1),
            sequence: Counter(1)
        }]
    );
    assert!(!phase.effect_has_current_native_level(&effect));
    assert!(!phase.endpoint_has_current_native_level(&ep("script", "value")));
    let second = phase.script_requests().pop().unwrap();
    assert_eq!(second.inputs["input"], value(true));
    reply(&mut phase, &second, false).unwrap();
    let third = phase.script_requests().pop().unwrap();
    assert_eq!(third.inputs["input"], value(false));
    reply(&mut phase, &third, false).unwrap();
    assert!(phase.endpoint_has_current_native_level(&ep("script", "value")));
}

#[test]
fn expired_inflight_retires_and_late_reply_cannot_fault_or_block_newer_fifo() {
    let mut phase = phase(true, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    let first = phase.script_requests().pop().unwrap();
    admit(&mut phase, update(2, 90_000, true), 90_000);
    phase.advance(Counter(91_000)).unwrap();
    assert_eq!(phase.next_deadline_us(), Some(Counter(100_001)));
    phase.advance(Counter(100_001)).unwrap();
    assert!(!phase.script_ticket_current(&first.ticket));
    let second = phase.script_requests().pop().unwrap();
    assert_eq!(second.inputs["input"], value(true));
    assert!(reply(&mut phase, &first, false).is_err());
    assert!(phase.script_ticket_current(&second.ticket));
    assert!(!phase.states["script"].fault);
    reply(&mut phase, &second, true).unwrap();
}

#[test]
fn original_sampled_script_output_expires_even_when_current_root_is_newer() {
    let mut phase = phase(true, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    let first = phase.script_requests().pop().unwrap();
    admit(&mut phase, update(2, 90_000, true), 90_000);
    reply(&mut phase, &first, true).unwrap();
    phase.advance(Counter(91_000)).unwrap();
    assert_eq!(phase.next_deadline_us(), Some(Counter(100_001)));
    phase.advance(Counter(100_001)).unwrap();
    assert_eq!(phase.sample(&ep("game", "x")), Some(&value(true)));
    assert_eq!(
        phase.sample(&ep("script", "value")),
        Some(&SignalSample::Stale)
    );
    assert!(phase.script_requests().len() == 1);
}

#[test]
fn family_expiry_recovery_and_mixed_other_family_update_are_independent() {
    let mut phase = phase(false, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    admit(
        &mut phase,
        frame(
            2,
            &[("z", NativeChangeKind::Baseline, 1, Some(false), 2, 50_000)],
        ),
        50_000,
    );
    phase.advance(Counter(100_001)).unwrap();
    assert_eq!(phase.sample(&ep("game", "x")), Some(&SignalSample::Stale));
    assert_eq!(phase.sample(&ep("other", "value")), Some(&value(false)));
    let recovery = frame(
        3,
        &[
            ("x", NativeChangeKind::Baseline, 2, Some(true), 3, 110_000),
            ("y", NativeChangeKind::Baseline, 2, None, 0, 0),
            ("z", NativeChangeKind::Update, 1, Some(true), 3, 110_000),
        ],
    );
    admit(&mut phase, recovery, 110_000);
    assert_eq!(phase.sample(&ep("pulse", "value")), Some(&value(false)));
    assert_eq!(phase.sample(&ep("other", "value")), Some(&value(true)));
    phase.advance(Counter(210_000)).unwrap();
    assert_eq!(
        phase.sample(&ep("mapped", "value")),
        Some(&SignalSample::Stale)
    );
    assert_eq!(
        phase.sample(&ep("other", "value")),
        Some(&SignalSample::Stale)
    );
}

#[test]
fn family_rebase_fences_old_replies_and_effects_while_independent_native_family_survives() {
    let mut phase = phase(true, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    let first = phase.script_requests().pop().unwrap();
    admit(
        &mut phase,
        frame(2, &[("z", NativeChangeKind::Baseline, 1, Some(true), 2, 2)]),
        2,
    );
    let other = phase.endpoint_token(&ep("game", "z")).unwrap();
    let rebase = frame(
        3,
        &[
            ("x", NativeChangeKind::Baseline, 2, Some(true), 3, 3),
            ("y", NativeChangeKind::Baseline, 2, None, 0, 0),
        ],
    );
    admit(&mut phase, rebase, 3);
    assert!(reply(&mut phase, &first, true).is_err());
    assert_eq!(phase.endpoint_token(&ep("game", "z")).unwrap(), other);
    assert_eq!(phase.sample(&ep("other", "value")), Some(&value(true)));
    let next = phase.script_requests().pop().unwrap();
    reply(&mut phase, &next, true).unwrap();
}

#[test]
fn desktop_fork_and_lease_mirror_preserve_exact_native_proof_without_receipt_capture() {
    let mut canonical = phase(false, NativePolicy::Latched);
    admit(
        &mut canonical,
        latched(baseline(1, 1, false), 1, 1, 200_000),
        1,
    );
    let mut desktop = canonical.fork_desktop().unwrap();
    admit(
        &mut canonical,
        latched(frame(2, &[]), 2, 90_000, 250_000),
        90_000,
    );
    desktop.mirror_native_state(&canonical).unwrap();
    assert_eq!(desktop.values[&ep("game", "x")].captured, 1);
    assert!(desktop.endpoint_has_current_native_level(&ep("game", "x")));
    desktop.advance(Counter(100_001)).unwrap();
    assert_eq!(desktop.sample(&ep("game", "x")), Some(&value(false)));
}

#[test]
fn mixed_family_baseline_seeds_old_other_level_before_its_real_press() {
    let mut p = program(false);
    let mut chord = node(
        "chord",
        "game",
        "x",
        Operation::Builtin {
            rule: crate::rules::Rule::Chord,
            inputs: vec!["input".into(), "other".into()],
        },
    );
    chord.inputs.push(InputBinding {
        port_id: "other".into(),
        from: ep("game", "z"),
        contract: contract(),
        edge_id: "chord-other".into(),
    });
    p.nodes.push(chord);
    let mut phase =
        Phase::new(p, Counter(1), "b3738156-6794-42dc-978b-2376dce0c6c6".into()).unwrap();
    phase.pin_native_clock("clock").unwrap();
    phase
        .register_native_source(
            super::phase(false, NativePolicy::Sampled)
                .native
                .registrations["game"]
                .clone(),
        )
        .unwrap();
    admit(&mut phase, baseline(1, 1, false), 1);
    admit(
        &mut phase,
        frame(
            2,
            &[("z", NativeChangeKind::Baseline, 1, Some(false), 2, 2)],
        ),
        2,
    );
    let mixed = frame(
        3,
        &[
            ("z", NativeChangeKind::Baseline, 2, Some(true), 3, 3),
            ("x", NativeChangeKind::Update, 1, Some(true), 3, 3),
        ],
    );
    admit(&mut phase, mixed, 3);
    assert_eq!(phase.sample(&ep("chord", "value")), Some(&value(true)));
}

#[test]
fn unrelated_direct_expiry_cannot_expire_native_values_with_a_renewed_lease() {
    let mut phase = phase(false, NativePolicy::Latched);
    admit(&mut phase, latched(baseline(1, 1, false), 1, 1, 200_000), 1);
    phase
        .accept_admissions([direct(1, Reason::Baseline, Some(false), 1)], Counter(1))
        .unwrap();
    admit(
        &mut phase,
        latched(frame(2, &[]), 2, 90_000, 250_000),
        90_000,
    );
    phase.advance(Counter(100_001)).unwrap();
    assert_eq!(
        phase.sample(&ep("pad", "value")),
        Some(&SignalSample::Stale)
    );
    assert_eq!(phase.sample(&ep("game", "x")), Some(&value(false)));
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(false)));
    assert_eq!(phase.values[&ep("game", "x")].captured, 1);
    assert_eq!(phase.next_deadline_us(), Some(Counter(190_000)));
}

#[test]
fn native_loss_clamps_queued_virtual_slot_but_preserves_independent_queued_edges() {
    let mut p = program(false);
    p.sinks.clear();
    p.controllers.push(VirtualController {
        id: "target".into(),
        layout: ControllerLayout {
            buttons: vec!["native".into(), "direct".into()],
            axes: vec![],
            hats: vec![],
        },
    });
    for (index, (id, node, port)) in [("native", "game", "x"), ("direct", "pad", "value")]
        .into_iter()
        .enumerate()
    {
        p.sinks.push(Sink {
            id: id.into(),
            endpoint: ep("target", id),
            from: ep(node, port),
            contract: contract(),
            kind: SinkKind::Virtual {
                controller_id: "target".into(),
                control: crate::profile::OutputControl::Button {
                    index: index as u16,
                },
            },
        });
    }
    let mut phase =
        Phase::new(p, Counter(1), "b3738156-6794-42dc-978b-2376dce0c6c6".into()).unwrap();
    phase.pin_native_clock("clock").unwrap();
    phase
        .register_native_source(
            super::phase(false, NativePolicy::Sampled)
                .native
                .registrations["game"]
                .clone(),
        )
        .unwrap();
    admit(&mut phase, baseline(1, 1, false), 1);
    phase
        .accept_admissions([direct(1, Reason::Baseline, Some(false), 1)], Counter(1))
        .unwrap();
    phase.drain_reports();
    phase.drain();
    admit(&mut phase, update(2, 2, true), 2);
    phase
        .accept_admissions(
            [
                direct(2, Reason::Change, Some(true), 3),
                direct(3, Reason::Change, Some(false), 4),
            ],
            Counter(4),
        )
        .unwrap();
    admit(
        &mut phase,
        frame(
            3,
            &[
                ("x", NativeChangeKind::Withdraw, 1, None, 0, 0),
                ("y", NativeChangeKind::Withdraw, 1, None, 0, 0),
            ],
        ),
        5,
    );
    let reports = phase.drain_reports();
    assert!(reports.iter().all(|report| !report.state.buttons[0]));
    let independent = reports
        .iter()
        .map(|report| report.state.buttons[1])
        .collect::<Vec<_>>();
    assert!(independent.windows(2).any(|values| values == [true, false]));
}

#[test]
fn expired_lease_only_transaction_revokes_before_timers_without_rewinding_the_clock() {
    let mut phase = phase(false, NativePolicy::Latched);
    admit(
        &mut phase,
        latched(baseline(1, 1, false), 1, 1, 1_000_000),
        1,
    );
    phase.advance(Counter(100_000)).unwrap();
    phase
        .schedule(
            phase.token("mapped").unwrap(),
            "pending".into(),
            Counter(105_000),
            BTreeMap::from([(
                "value".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    phase.drain();
    phase.drain_reports();
    admit(
        &mut phase,
        latched(frame(2, &[]), 2, 90_000, 95_000),
        110_000,
    );
    assert_eq!(phase.at_us(), Counter(110_000));
    assert_eq!(phase.sample(&ep("game", "x")), Some(&SignalSample::Stale));
    assert!(phase
        .drain()
        .iter()
        .all(|effect| effect.at_us >= Counter(100_000) && effect.reason != ReportReason::Timer));
    assert!(phase
        .drain_reports()
        .iter()
        .all(|report| report.at_us >= Counter(100_000)));
    assert!(phase.timers.is_empty());
}

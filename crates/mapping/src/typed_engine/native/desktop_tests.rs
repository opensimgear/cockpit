use super::*;

fn desktop_phase(script: bool) -> Phase {
    let mut program = program(script);
    let action = crate::desktop::Action::KeyHold {
        key: crate::desktop::Key(4),
    };
    program.sinks = vec![Sink {
        id: "keyboard-hold".into(),
        endpoint: ep("keyboard", "action"),
        from: ep(if script { "script" } else { "pulse" }, "value"),
        contract: action.contract(),
        kind: SinkKind::Desktop { action },
    }];
    let mut phase = Phase::new(
        program,
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    )
    .unwrap();
    phase.pin_native_clock("clock").unwrap();
    phase
        .register_native_source(NativeSourceRegistration {
            source: "game".into(),
            clock_id: "clock".into(),
            ports: [("x", "bios"), ("y", "bios"), ("z", "ownship")]
                .into_iter()
                .map(|(port, family)| NativePortRegistration {
                    port_id: port.into(),
                    family: family.into(),
                    policy: NativePolicy::Latched,
                })
                .collect(),
        })
        .unwrap();
    phase.drain();
    phase
}

#[test]
fn native_focus_return_needs_a_real_neutral_and_old_pulse_off_keeps_original_evidence() {
    let mut canonical = desktop_phase(false);
    admit(
        &mut canonical,
        latched(baseline(1, 1, false), 1, 1, 1_000_000),
        1,
    );
    canonical
        .accept_admissions([direct(1, Reason::Baseline, Some(false), 1)], Counter(1))
        .unwrap();
    admit(
        &mut canonical,
        latched(update(2, 10, true), 2, 10, 1_000_000),
        10,
    );
    assert_eq!(canonical.sample(&ep("pulse", "value")), Some(&value(true)));
    assert!(canonical
        .require_desktop_neutral_after(Counter(10))
        .is_err());

    let mut shadow = canonical.fork_desktop().unwrap();
    assert_eq!(shadow.program().nodes.len(), 1);
    assert_eq!(shadow.program().nodes[0].id, "pulse");
    assert_eq!(shadow.sample(&ep("pulse", "value")), Some(&value(false)));
    assert!(shadow.drain().is_empty());
    shadow.require_desktop_neutral_after(Counter(10)).unwrap();
    assert!(shadow.require_desktop_neutral_after(Counter(11)).is_err());

    canonical
        .observe_liveness("pad", Counter(1), Counter(25))
        .unwrap();
    shadow.mirror_source_liveness(&canonical).unwrap();
    assert_eq!(
        shadow.inspected_sample(&ep("pad", "value")).unwrap().1,
        Some(Counter(25))
    );
    assert_eq!(shadow.values[&ep("pad", "value")].captured, 1);
    let lease = latched(frame(3, &[]), 3, 20, 1_000_000);
    canonical
        .accept_admissions([lease.clone().into()], Counter(25))
        .unwrap();
    shadow
        .accept_admissions([lease.into()], Counter(25))
        .unwrap();
    shadow.mirror_native_state(&canonical).unwrap();
    assert!(
        shadow.drain().is_empty(),
        "A genuine lease cannot invent neutral input after focus return"
    );
    assert!(shadow.states["pulse"].rule.next().is_none());

    let release = latched(update(4, 30, false), 4, 30, 1_000_000);
    admit(&mut canonical, release.clone(), 30);
    shadow.mirror_source_liveness(&canonical).unwrap();
    assert_eq!(
        shadow.values[&ep("game", "x")].captured,
        10,
        "Mirroring must not borrow the newer native report"
    );
    admit(&mut shadow, release, 30);
    let neutral = shadow.drain();
    assert!(neutral
        .iter()
        .any(|effect| effect.sample == value(false)
            && shadow.effect_has_current_native_level(effect)));
    assert!(neutral
        .iter()
        .flat_map(|effect| &effect.observations)
        .all(|observation| observation.at_us == Counter(30)));

    let press = latched(update(5, 40, true), 5, 40, 1_000_000);
    admit(&mut canonical, press.clone(), 40);
    admit(&mut shadow, press, 40);
    let pressed = shadow
        .drain()
        .into_iter()
        .find(|effect| effect.sample == value(true))
        .unwrap();
    assert!(shadow.effect_has_current_native_level(&pressed));
    let release = latched(update(6, 50, false), 6, 50, 1_000_000);
    admit(&mut canonical, release.clone(), 50);
    admit(&mut shadow, release, 50);
    assert!(!shadow.effect_has_current_native_level(&pressed));
    shadow.advance(Counter(1040)).unwrap();
    let timer_release = shadow
        .drain()
        .into_iter()
        .find(|effect| effect.sample == value(false))
        .unwrap();
    assert!(
        !shadow.effect_has_current_native_level(&timer_release),
        "Authored pulse completion cannot rearm a later focus scope"
    );
    assert!(timer_release
        .observations
        .iter()
        .all(|observation| observation.at_us == Counter(40)));

    admit(
        &mut shadow,
        latched(update(7, 1050, false), 7, 1050, 1_000_000),
        1050,
    );
    let fresh = shadow
        .drain()
        .into_iter()
        .find(|effect| effect.sample == value(false))
        .unwrap();
    assert!(shadow.effect_has_current_native_level(&fresh));
    assert!(fresh
        .observations
        .iter()
        .all(|observation| observation.at_us == Counter(1050)));
    let mut another_run = desktop_phase(false);
    another_run.run.generation = Counter(2);
    assert!(shadow.mirror_source_liveness(&another_run).is_err());
    assert!(shadow.mirror_native_state(&another_run).is_err());
    canonical.stop();
    assert!(canonical.fork_desktop().is_err());
}

#[test]
fn native_desktop_replays_the_whole_lease_drain_before_mirroring_and_retains_qualification_gate() {
    assert!(desktop_phase(true).fork_desktop().is_err());
    let mut canonical = desktop_phase(false);
    admit(
        &mut canonical,
        latched(baseline(1, 1, false), 1, 1, 1_000_000),
        1,
    );
    let mut shadow = canonical.fork_desktop().unwrap();
    let drain = vec![
        latched(frame(2, &[]), 2, 20, 1_000_000).into(),
        latched(frame(3, &[]), 3, 30, 1_000_000).into(),
    ];
    canonical
        .accept_admissions(drain.clone(), Counter(30))
        .unwrap();
    shadow.accept_admissions(drain, Counter(30)).unwrap();
    shadow.mirror_native_state(&canonical).unwrap();
    shadow.mirror_source_liveness(&canonical).unwrap();
    assert_eq!(shadow.native.leases, canonical.native.leases);
    assert_eq!(shadow.values[&ep("game", "x")].captured, 1);
    assert!(shadow.endpoint_has_current_native_level(&ep("game", "x")));
    assert!(shadow.drain().is_empty());
    shadow.native.clock = Some("different-clock".into());
    assert!(shadow.mirror_native_state(&canonical).is_err());
}

use super::*;

#[test]
fn bios_freshness_cannot_arm_an_unavailable_ownship_only_source() {
    let mut sources = setup();
    sources.sources.get_mut("game").unwrap().ports.retain(|port| port.family == "dcs-ownship");
    physics(&mut sources, false);
    let now = sources.clock.now_us();
    let poll = sources.frames(snapshot(None, None, true), now);
    assert!(sources.fresh, "BIOS command readiness stays separate");
    assert!(!sources.read_ready());
    assert!(!poll.fresh);
    assert!(poll.frames.is_empty());
}

#[test]
fn inventory_revocation_keeps_numbered_native_updates_inside_the_loss_barrier() {
    let mut sources = setup();
    let mut phase = reducer(&sources);
    let stamp = capture("bios", 1);
    let now = sources.clock.now_us();
    let initial = sources.frames(snapshot(Some(value(stamp.clone(), stamp, false)), None, true), now);
    phase.accept_admissions(initial.frames, Counter(now)).unwrap();
    let stamp = capture("bios", 2);
    let now = sources.clock.now_us();
    let update = sources.frames(snapshot(Some(value(stamp.clone(), stamp, false)), None, true), now);
    let loss = sources.pending(now, "Inventory identity validation is pending".into());
    assert_eq!(native(&loss).sequence.0, native(&update).sequence.0 + 1);
    let combined = update.revoked_by(loss);
    assert_eq!(combined.frames.len(), 2);
    phase.accept_admissions(combined.frames, Counter(now)).unwrap();
    let endpoint = osg_mapping::program::Endpoint { node_id: "game".into(), port_id: "bios".into() };
    assert_eq!(phase.sample(&endpoint), Some(&SignalSample::Stale));
    let stamp = capture("bios", 3);
    let now = sources.clock.now_us();
    let recovery = sources.frames(snapshot(Some(value(stamp.clone(), stamp, false)), None, true), now);
    phase.accept_admissions(recovery.frames, Counter(now)).unwrap();
    assert!(matches!(phase.sample(&endpoint), Some(SignalSample::Known(_))));
}

#[test]
fn healthy_ownship_can_supply_a_read_baseline_without_granting_bios_command_authority() {
    let mut sources = setup();
    physics(&mut sources, true);
    let stamp = capture("ownship", 1);
    let now = sources.clock.now_us();
    let poll = sources.frames(snapshot(None, Some(value(stamp.clone(), stamp, true)), false), now);
    assert!(poll.fresh, "A qualified independent source can arm read-only routing");
    assert!(!sources.fresh, "Ownship cannot manufacture BIOS command readiness");
    assert!(!sources.can_command());
    assert!(native(&poll).changes.iter().all(|change| change.port_id == "ownship"));
    reducer(&sources).accept_admissions(poll.frames, Counter(now)).unwrap();
}

#[test]
fn an_all_missing_family_still_rebases_when_its_native_context_changes() {
    let mut sources = setup();
    let first = sources.frames(snapshot(None, None, true), sources.clock.now_us());
    let mut next = snapshot(None, None, true);
    next.epoch = 2;
    let rebase = sources.frames(next, sources.clock.now_us());
    assert_eq!(native(&rebase).changes.len(), 2);
    assert!(native(&rebase).changes.iter().all(|change| change.kind == NativeChangeKind::Baseline && change.sample == SignalSample::Unknown));
    assert!(native(&rebase).changes[0].continuity > native(&first).changes[0].continuity);
}

fn reducer(sources: &GameSources) -> osg_mapping::engine::typed::Phase {
    let program = osg_mapping::program::Program {
        sources: sources.sources["game"]
            .ports
            .iter()
            .map(|port| osg_mapping::program::SourcePort {
                endpoint: osg_mapping::program::Endpoint {
                    node_id: "game".into(),
                    port_id: port.logical.clone(),
                },
                contract: port.contract.clone(),
            })
            .collect(),
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    let mut phase = osg_mapping::engine::typed::Phase::new(
        program,
        Counter(1),
        uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    phase
        .pin_native_clock(&sources.clock.id().to_string())
        .unwrap();
    for registration in sources.native_registrations() {
        phase.register_native_source(registration).unwrap();
    }
    phase
}

#[test]
fn reader_replacement_after_family_recovery_inherits_continuity_without_copying_old_levels() {
    let mut sources = setup();
    let mut phase = reducer(&sources);
    let mut previous_continuity = 0;
    for iteration in 1..=2 {
        let stamp = capture("bios", iteration);
        let now = sources.clock.now_us();
        let poll = sources.frames(
            snapshot(Some(value(stamp.clone(), stamp, false)), None, true),
            now,
        );
        previous_continuity = native(&poll)
            .changes
            .iter()
            .find(|change| change.port_id == "bios")
            .unwrap()
            .continuity
            .0;
        phase.accept_admissions(poll.frames, Counter(now)).unwrap();
        if iteration == 1 {
            let now = sources.clock.now_us();
            let poll = sources.frames(snapshot(None, None, false), now);
            phase.accept_admissions(poll.frames, Counter(now)).unwrap();
        }
    }
    let previous_generation = sources.sources["game"].generation;
    let mut replacement = setup();
    replacement.clock = sources.clock;
    replacement.inherit_source_counters(&sources);
    let stamp = capture("bios", 3);
    let now = replacement.clock.now_us();
    let poll = replacement.frames(
        snapshot(Some(value(stamp.clone(), stamp, false)), None, true),
        now,
    );
    let frame = native(&poll);
    assert_eq!(frame.generation.0, previous_generation + 1);
    assert_eq!(
        frame.changes.len(),
        3,
        "A reader replacement closes every prepared family"
    );
    assert!(frame
        .changes
        .iter()
        .all(|change| change.kind == NativeChangeKind::Baseline));
    assert!(frame
        .changes
        .iter()
        .filter(|change| change.port_id != "ownship")
        .all(|change| change.continuity.0 > previous_continuity));
    assert_eq!(
        frame
            .changes
            .iter()
            .find(|change| change.port_id == "ownship")
            .unwrap()
            .sample,
        SignalSample::Unknown
    );
    phase.accept_admissions(poll.frames, Counter(now)).unwrap();
}

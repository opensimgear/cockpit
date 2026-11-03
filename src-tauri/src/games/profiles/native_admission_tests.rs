use super::*;
use crate::native_capture::{CapturedValue, Contribution, InputId, Lease, Stamp, Validity};
use std::{sync::Arc, time::Duration};
fn setup() -> GameSources {
    let clock = osg_controller_input::ControllerService::new(None, false).capture_clock();
    let mut sources = GameSources::prepare(&empty_plan(), None, clock).unwrap();
    let mut state = SourceState {
        generation: 1,
        ..Default::default()
    };
    for (logical, family, policy) in [
        ("bios", "dcs-bios", NativePolicy::Latched),
        ("absent", "dcs-bios", NativePolicy::Latched),
        ("ownship", "dcs-ownship", NativePolicy::Sampled),
    ] {
        state.ports.push(SourcePort {
            logical: logical.into(),
            native: logical.into(),
            family: family.into(),
            policy,
            contract: PortContract::State {
                schema: osg_profiles::resources_ports::ScalarSchema::Integer {
                    min: 0,
                    max: 1,
                    unit: None,
                },
                freshness_ms: 100,
                fallback: None,
            },
        });
    }
    sources.sources.insert("game".into(), state);
    sources
}
fn capture(producer: &str, sequence: u64) -> Stamp {
    Stamp {
        producer_id: producer.into(),
        generation: 1,
        sequence,
        byte_offset: (producer == "bios").then_some(0),
        captured_at: Instant::now(),
    }
}
fn value(stamp: Stamp, observation: Stamp, sampled: bool) -> CapturedValue {
    let validity = if sampled {
        Validity::Sampled {
            valid_until: stamp.captured_at + Duration::from_millis(100),
        }
    } else {
        Validity::Latched(Arc::new(Lease {
            domain_id: "bios-lease".into(),
            generation: 1,
            valid_until: observation.captured_at + Duration::from_millis(100),
            observation,
        }))
    };
    CapturedValue::native(
        json!(1),
        vec![Arc::new(Contribution {
            input: if sampled {
                InputId::Channel("ownship".into())
            } else {
                InputId::Byte(1)
            },
            capture: stamp,
            validity,
        })],
        Instant::now(),
    )
    .unwrap()
}
fn snapshot(
    bios: Option<CapturedValue>,
    ownship: Option<CapturedValue>,
    fresh: bool,
) -> ExportSnapshot {
    ExportSnapshot {
        epoch: 1,
        aircraft: Some("A-10C".into()),
        mission_active: true,
        fresh,
        age_ms: Some(0),
        counter: Some(1),
        values: [("bios", bios), ("ownship", ownship)]
            .into_iter()
            .filter_map(|(id, value)| value.map(|value| (id.into(), value)))
            .collect(),
        malformed_frames: 0,
    }
}
fn physics(sources: &mut GameSources, fresh: bool) {
    sources.physics = Some(PhysicsMerge {
        witness: PhysicsWitness {
            producer_id: Some("ownship".into()),
            session: Some("mission".into()),
            epoch: 1,
            sequence: Some(1),
            fresh,
        },
        keys: BTreeSet::from(["ownship".into()]),
        stale: BTreeSet::new(),
    });
}
fn native(poll: &GamePoll) -> &NativeFrame {
    match &poll.frames[0] {
        Admission::Native(frame) => frame,
        _ => panic!("Native evidence must remain native"),
    }
}
#[test]
fn prepared_absent_members_baseline_and_lease_renewal_preserve_original_capture() {
    let mut sources = setup();
    assert_eq!(sources.native_registrations()[0].ports.len(), 3);
    let input = capture("bios", 1);
    let first = value(input.clone(), input.clone(), false);
    let baseline = sources.frames(snapshot(Some(first), None, true), sources.clock.now_us());
    let frame = native(&baseline);
    assert_eq!(frame.changes.len(), 2);
    assert_eq!(
        frame
            .changes
            .iter()
            .find(|c| c.port_id == "absent")
            .unwrap()
            .sample,
        SignalSample::Unknown
    );
    let original = frame
        .captures
        .iter()
        .find(|c| c.sequence.0 == 1)
        .unwrap()
        .clone();
    let observation = capture("bios", 2);
    let renewed = sources.frames(
        snapshot(Some(value(input, observation, false)), None, true),
        sources.clock.now_us(),
    );
    let frame = native(&renewed);
    assert!(frame.changes.is_empty());
    assert!(frame.captures.contains(&original));
    assert_eq!(frame.leases.len(), 1);
}
#[test]
fn bios_loss_and_recovery_do_not_rebaseline_live_ownship() {
    let mut sources = setup();
    physics(&mut sources, true);
    let bios = capture("bios", 1);
    let own = capture("ownship", 1);
    let baseline = sources.frames(
        snapshot(
            Some(value(bios.clone(), bios.clone(), false)),
            Some(value(own.clone(), own.clone(), true)),
            true,
        ),
        sources.clock.now_us(),
    );
    assert_eq!(native(&baseline).changes.len(), 3);
    let loss = sources.frames(
        snapshot(None, Some(value(own.clone(), own.clone(), true)), false),
        sources.clock.now_us(),
    );
    assert!(native(&loss)
        .changes
        .iter()
        .all(|c| c.port_id != "ownship" && c.kind == NativeChangeKind::Withdraw));
    let bios = capture("bios", 2);
    let recovery = sources.frames(
        snapshot(
            Some(value(bios.clone(), bios, false)),
            Some(value(own.clone(), own, true)),
            true,
        ),
        sources.clock.now_us(),
    );
    assert!(native(&recovery)
        .changes
        .iter()
        .all(|c| c.port_id != "ownship" && c.kind == NativeChangeKind::Baseline));
    assert_eq!(native(&baseline).generation, native(&recovery).generation);
}
#[test]
fn capture_table_deduplicates_and_rejects_conflicting_original_time() {
    let sources = setup();
    let mut tables = native_tables::Tables::new(sources.clock);
    let stamp = capture("ownship", 1);
    let captured = value(stamp.clone(), stamp.clone(), true);
    let contributors = captured.contributors().unwrap();
    tables
        .contributors(contributors, NativePolicy::Sampled, 100)
        .unwrap();
    tables
        .contributors(contributors, NativePolicy::Sampled, 100)
        .unwrap();
    assert_eq!(tables.captures.len(), 1);
    let mut conflict = stamp;
    conflict.captured_at += Duration::from_nanos(1);
    let captured = value(conflict.clone(), conflict, true);
    assert!(tables
        .contributors(captured.contributors().unwrap(), NativePolicy::Sampled, 100)
        .is_err());
}

fn empty_plan() -> ExecutionPlan {
    osg_profiles::compiler::compile(
        &osg_profiles::default_document(),
        &osg_profiles::compiler_models::CompileSnapshot {
            inventory_session: "native-test".into(),
            inventory_revision: "1".into(),
            assignments: vec![],
            resources: vec![],
            game_context: None,
            backend: osg_profiles::compiler_models::BackendCapabilities::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        },
    )
    .plan
    .unwrap()
}
#[test]
fn original_sample_expiry_and_authored_proof_loss_fail_closed() {
    let mut sources = setup();
    physics(&mut sources, true);
    let old = Instant::now() - Duration::from_millis(200);
    let stamp = Stamp {
        captured_at: old,
        ..capture("ownship", 1)
    };
    let expired = CapturedValue::native(
        json!(1),
        vec![Arc::new(Contribution {
            input: InputId::Channel("ownship".into()),
            capture: stamp,
            validity: Validity::Sampled {
                valid_until: old + Duration::from_millis(100),
            },
        })],
        old,
    )
    .unwrap();
    let poll = sources.frames(snapshot(None, Some(expired), false), sources.clock.now_us());
    assert!(
        native(&poll)
            .changes
            .iter()
            .all(|change| change.sample == SignalSample::Unknown),
        "An old sample cannot acquire receipt-time authority"
    );
    let stamp = capture("ownship", 2);
    let live = sources.frames(
        snapshot(None, Some(value(stamp.clone(), stamp, true)), false),
        sources.clock.now_us(),
    );
    assert_eq!(native(&live).changes[0].kind, NativeChangeKind::Baseline);
    let lost = sources.frames(
        snapshot(None, Some(CapturedValue::fixture(json!(1))), false),
        sources.clock.now_us(),
    );
    assert_eq!(native(&lost).changes[0].sample, SignalSample::Unknown);
    assert!(matches!(lost.frames[0], Admission::Native(_)));
}
#[test]
fn native_clock_rejects_original_capture_before_journal_origin() {
    let sources = setup();
    let mut tables = native_tables::Tables::new(sources.clock);
    let stamp = Stamp {
        captured_at: Instant::now() - Duration::from_secs(2),
        ..capture("ownship", 1)
    };
    let items = vec![Arc::new(Contribution {
        input: InputId::Channel("ownship".into()),
        capture: stamp.clone(),
        validity: Validity::Sampled {
            valid_until: stamp.captured_at + Duration::from_millis(100),
        },
    })];
    assert!(tables
        .contributors(&items, NativePolicy::Sampled, 100)
        .is_err());
}
#[test]
fn lease_domain_generation_rebases_every_prepared_family_member() {
    let mut sources = setup();
    let stamp = capture("bios", 1);
    let first = value(stamp.clone(), stamp.clone(), false);
    let baseline = sources.frames(
        snapshot(Some(first.clone()), None, true),
        sources.clock.now_us(),
    );
    let mut contribution = first.contributors().unwrap()[0].as_ref().clone();
    let Validity::Latched(lease) = &contribution.validity else {
        panic!()
    };
    contribution.validity = Validity::Latched(Arc::new(Lease {
        generation: 2,
        ..lease.as_ref().clone()
    }));
    let replacement =
        CapturedValue::native(json!(1), vec![Arc::new(contribution)], Instant::now()).unwrap();
    let poll = sources.frames(
        snapshot(Some(replacement), None, true),
        sources.clock.now_us(),
    );
    assert_eq!(native(&poll).changes.len(), 2);
    assert!(native(&poll)
        .changes
        .iter()
        .all(|change| change.kind == NativeChangeKind::Baseline));
    assert!(native(&poll).changes[0].continuity > native(&baseline).changes[0].continuity);
}
#[test]
fn replacement_generation_closes_unavailable_prepared_families() {
    let mut sources = setup();
    sources.sources.get_mut("game").unwrap().replacement = true;
    let stamp = capture("bios", 1);
    let baseline = sources.frames(
        snapshot(Some(value(stamp.clone(), stamp, false)), None, true),
        sources.clock.now_us(),
    );
    assert_eq!(native(&baseline).changes.len(), 3);
    let ownship = native(&baseline)
        .changes
        .iter()
        .find(|change| change.port_id == "ownship")
        .unwrap();
    assert_eq!(ownship.kind, NativeChangeKind::Baseline);
    assert_eq!(ownship.sample, SignalSample::Unknown);
}
#[test]
fn exhausted_native_sequence_is_an_explicit_fatal_boundary() {
    let mut sources = setup();
    sources.sources.get_mut("game").unwrap().sequence = u64::MAX;
    let stamp = capture("bios", 1);
    let poll = sources.frames(
        snapshot(Some(value(stamp.clone(), stamp, false)), None, true),
        sources.clock.now_us(),
    );
    assert!(poll
        .fatal
        .as_ref()
        .is_some_and(|error| error.contains("sequence exhausted")));
}

#[test]
fn shared_provider_lease_keeps_independent_port_freshness_through_reducer_admission() {
    let mut sources = setup();
    let ports = &mut sources.sources.get_mut("game").unwrap().ports;
    if let PortContract::State { freshness_ms, .. } = &mut ports[1].contract { *freshness_ms = 1000; }
    let stamp = capture("bios", 1);
    let expiry = stamp.captured_at + Duration::from_millis(1500);
    let input = CapturedValue::native(json!(1), vec![Arc::new(Contribution {
        input: InputId::Byte(1), capture: stamp.clone(),
        validity: Validity::Latched(Arc::new(Lease {
            domain_id: "shared-provider-lease".into(), generation: 1,
            observation: stamp.clone(), valid_until: expiry,
        })),
    })], Instant::now()).unwrap();
    let mut first_snapshot = snapshot(Some(input.clone()), None, true);
    first_snapshot.values.insert("absent".into(), input);
    let registrations = sources.native_registrations();
    let program = osg_mapping::program::Program {
        sources: sources.sources["game"].ports.iter().map(|port| osg_mapping::program::SourcePort {
            endpoint: osg_mapping::program::Endpoint { node_id: "game".into(), port_id: port.logical.clone() },
            contract: port.contract.clone(),
        }).collect(), nodes: vec![], sinks: vec![], controllers: vec![],
    };
    let mut phase = osg_mapping::engine::typed::Phase::new(program, Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    phase.pin_native_clock(&sources.clock.id().to_string()).unwrap();
    for registration in registrations { phase.register_native_source(registration).unwrap(); }
    let now = sources.clock.now_us();
    let poll = sources.frames(first_snapshot, now);
    let frame = native(&poll);
    assert_eq!(frame.leases.len(), 1, "Port policy cannot duplicate or rewrite shared provider authority");
    assert_eq!(frame.leases[0].valid_until_us.0, sources.clock.deadline_us(expiry).unwrap());
    phase.accept_admissions(poll.frames, Counter(now)).unwrap();
    let endpoint = |port: &str| osg_mapping::program::Endpoint { node_id: "game".into(), port_id: port.into() };
    phase.advance(Counter(sources.clock.captured_us(stamp.captured_at).unwrap() + 100_000)).unwrap();
    assert_eq!(phase.sample(&endpoint("bios")), Some(&SignalSample::Stale));
    assert!(matches!(phase.sample(&endpoint("absent")), Some(SignalSample::Known(_))));
    std::thread::sleep(Duration::from_millis(110));
    let observation = Stamp { sequence: 2, captured_at: Instant::now(), ..stamp.clone() };
    let renewed = CapturedValue::native(json!(1), vec![Arc::new(Contribution {
        input: InputId::Byte(1), capture: stamp.clone(),
        validity: Validity::Latched(Arc::new(Lease {
            domain_id: "shared-provider-lease".into(), generation: 1,
            valid_until: observation.captured_at + Duration::from_millis(1500), observation,
        })),
    })], Instant::now()).unwrap();
    let mut renewed_snapshot = snapshot(Some(renewed.clone()), None, true);
    renewed_snapshot.values.insert("absent".into(), renewed);
    let now = sources.clock.now_us();
    let poll = sources.frames(renewed_snapshot, now);
    assert!(native(&poll).changes.is_empty(), "Genuine heartbeat carries no new raw level");
    phase.accept_admissions(poll.frames, Counter(now)).unwrap();
    assert!(matches!(phase.sample(&endpoint("bios")), Some(SignalSample::Known(_))), "Expired short port must safely recover unchanged latched state from genuine authority");
    assert!(matches!(phase.sample(&endpoint("absent")), Some(SignalSample::Known(_))));
}

#[test]
fn expired_retained_field_stays_stale_until_a_new_explicit_packet_or_capture() {
    let mut sources = setup();
    physics(&mut sources, true);
    let stamp = capture("ownship", 1);
    let input = value(stamp.clone(), stamp, true);
    sources.frames(snapshot(None, Some(input), false), sources.clock.now_us());
    std::thread::sleep(Duration::from_millis(110));
    let expired = sources.frames(snapshot(None, None, false), sources.clock.now_us());
    assert_eq!(native(&expired).changes[0].sample, SignalSample::Stale);
    assert!(sources.frames(snapshot(None, None, false), sources.clock.now_us()).frames.is_empty());
    sources.physics.as_mut().unwrap().witness.sequence = Some(2);
    let explicitly_absent = sources.frames(snapshot(None, None, false), sources.clock.now_us());
    assert_eq!(native(&explicitly_absent).changes[0].sample, SignalSample::Unknown);
}
#[test]
fn same_sampled_acquisition_cannot_hide_changed_validity_in_a_noop_envelope() {
    let mut sources = setup();
    physics(&mut sources, true);
    let stamp = capture("ownship", 1);
    let first = value(stamp.clone(), stamp.clone(), true);
    sources.frames(snapshot(None, Some(first), false), sources.clock.now_us());
    let renewed = CapturedValue::native(json!(1), vec![Arc::new(Contribution {
        input: InputId::Channel("ownship".into()), capture: stamp.clone(),
        validity: Validity::Sampled { valid_until: stamp.captured_at + Duration::from_millis(200) },
    })], Instant::now()).unwrap();
    let rejected = sources.frames(snapshot(None, Some(renewed), false), sources.clock.now_us());
    assert!(rejected.context_lost);
    assert!(rejected.diagnostics.iter().any(|error| error.contains("cannot renew")));
    assert!(native(&rejected).changes.iter().all(|change| change.kind == NativeChangeKind::Withdraw));
}

#[path = "native_lifecycle_tests.rs"]
mod lifecycle;

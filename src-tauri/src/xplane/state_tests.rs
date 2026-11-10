fn evidence(clock: f64) -> Vec<(usize, f64)> {
    let mut values = vec![(0, 120550.), (1, 0.), (2, 0.), (3, clock), (4, 1.)];
    values.extend(
        b"Aircraft/Test.acf\0"
            .iter()
            .enumerate()
            .map(|(i, b)| (GUARDS.len() + i, *b as f64)),
    );
    values
}
fn live(now: Instant) -> (Session, NativeSnapshot) {
    let mut session = Session::new(&defaults(12));
    session.ingest(evidence(1.), now);
    session.native_snapshot(now, Some(now), 1, "producer", false);
    let mut values = evidence(2.);
    values.extend([
        (GUARDS.len() + PATH_BYTES, 0.),
        (GUARDS.len() + PATH_BYTES + 10, 0.),
    ]);
    session.ingest(values, now);
    let (snapshot, rotate) = session.native_snapshot(now, Some(now), 2, "producer", false);
    assert!(!rotate);
    assert!(snapshot.fresh_at(now));
    (session, snapshot)
}
#[test]
fn native_projection_preserves_zero_false_aliases_and_original_capture() {
    let now = Instant::now();
    let (session, snapshot) = live(now);
    let outputs = vec![
        json!({"id":"alias","channel":"airspeed_kts"}),
        json!({"id":"false","channel":"gear_unsafe"}),
    ];
    let (first, status) = snapshot.project(now, &outputs, &["airspeed_kts".into()]);
    assert_eq!(first.values["alias"].value().clone(), 0.);
    assert_eq!(first.values["airspeed_kts"].value().clone(), 0.);
    assert_eq!(first.values["false"].value().clone(), false);
    let proof=first.values["alias"].contributors().unwrap();
    assert_eq!(proof[0].input,crate::native_capture::InputId::Channel("sim/flightmodel/position/indicated_airspeed".into()));
    assert_eq!(proof[0].capture.sequence,2);
    assert_eq!(proof[0].capture.captured_at,now);
    assert_eq!(first.values["alias"].contributors(),first.values["airspeed_kts"].contributors());
    assert_eq!(status["channels"]["airspeed_kts"]["status"], "ready");
    let later = now + Duration::from_millis(100);
    let (second, _) = snapshot.project(later, &outputs, &[]);
    assert_eq!(second.counter, first.counter);
    assert_eq!(second.epoch, first.epoch);
    assert_eq!(snapshot.capture_sequence, 2);
    assert_eq!(snapshot.captured_at, Some(now));
    assert_eq!(snapshot.channels["airspeed_kts"].captured_at, Some(now));
    assert_eq!(snapshot.channels["airspeed_kts"].capture_sequence, 2);
    assert_eq!(session.advanced.map(|sample| sample.captured_at), Some(now));
    assert_eq!(second.age_ms, Some(100));
    assert!(snapshot
        .project(now + session.freshness, &outputs, &[])
        .0
        .values
        .is_empty());
}

#[test]
fn native_component_sequence_changes_only_on_its_own_accepted_capture() {
    let now = Instant::now();
    let (mut session, initial) = live(now);
    let airspeed = initial.channels["airspeed_kts"].clone();
    let later = now + Duration::from_millis(100);
    session.ingest(evidence(3.), later);
    let (guards, _) = session.native_snapshot(later, Some(later), 3, "producer", false);
    assert_eq!(guards.channels["airspeed_kts"].capture_sequence, airspeed.capture_sequence);
    assert_eq!(guards.channels["airspeed_kts"].captured_at, airspeed.captured_at);
    assert_eq!(guards.channels["gear_unsafe"].capture_sequence, 2);
    let outputs=vec![json!({"id":"kept","channel":"airspeed_kts"})];
    let projected=guards.project(later,&outputs,&[]).0;
    assert_eq!(projected.values["kept"].contributors().unwrap()[0].capture.sequence,2);
    assert_eq!(projected.values["kept"].contributors().unwrap()[0].capture.captured_at,now);
    assert!(!projected.values["kept"].is_current(now+session.freshness));

    // An equal value in a real new packet is new evidence; unrelated values
    // keep their own original acquisition sequence and timestamp.
    let refreshed = later + Duration::from_millis(100);
    session.ingest(vec![(GUARDS.len() + PATH_BYTES, 0.)], refreshed);
    let (fresh, _) = session.native_snapshot(refreshed, Some(refreshed), 4, "producer", false);
    assert_eq!(fresh.channels["airspeed_kts"].value, airspeed.value);
    assert_eq!(fresh.channels["airspeed_kts"].capture_sequence, 4);
    assert_eq!(fresh.channels["airspeed_kts"].captured_at, Some(refreshed));
    assert_eq!(fresh.channels["gear_unsafe"].captured_at, Some(now));
    assert_eq!(fresh.channels["gear_unsafe"].capture_sequence, 2);

    let mut channels = session.channels.clone();
    channels.reverse();
    session.select_shared_channels(channels);
    let (selected, _) = session.native_snapshot(refreshed, Some(refreshed), 4, "producer", false);
    assert_eq!(selected.channels["airspeed_kts"].capture_sequence, 4);
    assert_eq!(selected.channels["airspeed_kts"].captured_at, Some(refreshed));
    assert_eq!(selected.channels["gear_unsafe"].capture_sequence, 2);
}

#[test]
fn context_renewal_keeps_the_original_component_deadline() {
    use crate::native_capture::Validity;

    let now = Instant::now();
    let (mut session, _) = live(now);
    let captured = now + Duration::from_millis(100);
    let renewed = now + Duration::from_millis(200);
    let ttl = session.freshness;
    let inspection = ["airspeed_kts".into()];
    session.ingest(vec![(GUARDS.len() + PATH_BYTES, 32.)], captured);
    let (before, _) = session.native_snapshot(captured, Some(captured), 3, "producer", false);
    session.ingest(evidence(3.), renewed);
    let (after, _) = session.native_snapshot(renewed, Some(renewed), 4, "producer", false);
    let old = before.project(renewed, &[], &inspection).0.values;
    let new = after.project(renewed, &[], &inspection).0.values;
    let old_value = &old["airspeed_kts"];
    let new_value = &new["airspeed_kts"];
    let old = &old_value.contributors().unwrap()[0];
    let new = &new_value.contributors().unwrap()[0];
    assert_eq!(old.capture, new.capture);
    assert_eq!(new.capture.captured_at, captured);
    assert_eq!(new.capture.sequence, 3);
    for (item, expected_observation, expected_sequence) in [(old, now, 2), (new, renewed, 4)] {
        let Validity::GuardedSampled { valid_until, guard } = &item.validity else {
            panic!("Missing separate component and context evidence");
        };
        assert_eq!(*valid_until, captured + ttl);
        assert_eq!(guard.observation.captured_at, expected_observation);
        assert_eq!(guard.observation.sequence, expected_sequence);
        assert_eq!(guard.valid_until, expected_observation + ttl);
    }
    assert!(!old_value.is_current(now + ttl));
    assert!(new_value.is_current(now + ttl));
    assert!(!new_value.is_current(captured + ttl));
    assert!(after.fresh_at(captured + ttl));
}

#[test]
fn empty_packet_and_capture_sequence_exhaustion_cannot_renew_components() {
    let now = Instant::now();
    let (mut session, _) = live(now);
    session.ingest(vec![], now + Duration::from_millis(10));
    assert_eq!(session.capture_sequence, 2);
    session.capture_sequence = u64::MAX;
    session.ingest(evidence(3.), now + Duration::from_millis(20));
    let (snapshot, _) = session.native_snapshot(now, Some(now), u64::MAX, "producer", false);
    assert!(!snapshot.fresh_at(now));
    assert!(snapshot.status.contains("exhausted"));
    assert!(snapshot.channels.values().all(|sample| sample.value.is_none() && sample.capture_sequence == 0));
}
#[test]
fn native_deadlines_expire_individual_values_and_then_leave_idle_state() {
    let now = Instant::now();
    let (mut session, _) = live(now);
    let ttl = session.freshness;
    let refreshed = now + Duration::from_millis(100);
    session.ingest(evidence(3.), refreshed);
    let (snapshot, _) = session.native_snapshot(refreshed, Some(refreshed), 3, "producer", false);
    assert_eq!(session.deadline(refreshed), Some(now + ttl));
    assert!(snapshot.fresh_at(now + ttl));
    assert!(snapshot
        .project(now + ttl, &[], &["airspeed_kts".into()])
        .0
        .values
        .is_empty());
    let (expired, rotate) =
        session.native_snapshot(now + ttl, Some(refreshed), 3, "producer", false);
    assert!(!rotate);
    assert!(expired.fresh_at(now + ttl));
    assert_eq!(session.deadline(now + ttl), Some(refreshed + ttl));
    let (expired, rotate) =
        session.native_snapshot(refreshed + ttl, Some(refreshed), 3, "producer", false);
    assert!(rotate);
    assert!(!expired.fresh_at(refreshed + ttl));
    assert_eq!(session.deadline(refreshed + ttl), None);
}
#[test]
fn frozen_clock_updates_cannot_renew_simulator_progress() {
    let now = Instant::now();
    let (mut session, _) = live(now);
    let later = now + Duration::from_millis(100);
    session.ingest(evidence(2.), later);
    let (snapshot, _) = session.native_snapshot(later, Some(later), 3, "producer", false);
    assert_eq!(snapshot.advanced_at, Some(now));
    assert_eq!(snapshot.guard.unwrap().valid_until, now + session.freshness);
    assert_eq!(session.deadline(later), Some(now + session.freshness));
}
#[test]
fn baseline_initialization_and_epoch_exhaustion_fail_closed() {
    let now = Instant::now();
    let mut session = Session::new(&defaults(12));
    let (empty, _) = session.native_snapshot(now, None, 0, "producer", false);
    assert!(!empty.fresh_at(now));
    assert!(empty.guard.is_none());
    assert_eq!(session.deadline(now), None);
    let (mut session, _) = live(now);
    let (initializing, _) = session.native_snapshot(now, Some(now), 2, "producer", true);
    assert!(!initializing.fresh_at(now));
    assert!(initializing
        .project(now, &[], &["airspeed_kts".into()])
        .0
        .values
        .is_empty());
    session.epoch = u64::MAX;
    session.reset("test");
    session.ingest(evidence(4.), now);
    let (exhausted, _) = session.native_snapshot(now, Some(now), 3, "producer", false);
    assert_eq!(exhausted.epoch, u64::MAX);
    assert!(!exhausted.fresh_at(now));
    assert!(exhausted.status.contains("exhausted"));
}
#[test]
fn native_retention_counts_shared_map_allocation_without_serialization() {
    use osg_streams::RetainedBytes;
    let (_, snapshot) = live(Instant::now());
    let cloned = snapshot.clone();
    assert!(std::sync::Arc::ptr_eq(&snapshot.channels, &cloned.channels));
    assert!(
        snapshot.retained_bytes()
            > std::mem::size_of::<NativeSnapshot>()
                + snapshot.channels.len() * std::mem::size_of::<NativeSample>()
    );
}

#[test]
fn selected_projection_excludes_other_readers_large_union() {
    let now = Instant::now();
    let (_, mut snapshot) = live(now);
    let union = std::sync::Arc::make_mut(&mut snapshot.channels);
    for i in 0..1024 {
        union.insert(
            format!("other_reader_{i}"),
            NativeSample {
                native_input: None,
                value: Some(NativeValue::Number(i as f64)),
                captured_at: Some(now),
                capture_sequence: 2,
            },
        );
    }
    let outputs = vec![
        json!({"id":"zero","channel":"airspeed_kts"}),
        json!({"id":"false","channel":"gear_unsafe"}),
    ];
    let later = now + Duration::from_millis(100);
    let (export, status) = snapshot.project_selected(later, &outputs, &["airspeed_kts".into()]);
    let channels = status["channels"].as_object().unwrap();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels["airspeed_kts"]["value"], 0.);
    assert_eq!(channels["gear_unsafe"]["value"], false);
    assert_eq!(export.values.len(), 3);
    assert_eq!(export.values["zero"].value().clone(), 0.);
    assert_eq!(export.values["false"].value().clone(), false);
    assert_eq!(export.values["airspeed_kts"].value().clone(), 0.);
    assert_eq!(export.age_ms, Some(100));
    assert_eq!(status["ageMs"], 100);
    assert_eq!(snapshot.capture_sequence, 2);
    assert_eq!(snapshot.captured_at, Some(now));
    assert!(snapshot.project_selected(later, &[], &[]).1["channels"]
        .as_object()
        .unwrap()
        .is_empty());
    let (expired, status) = snapshot.project_selected(now + snapshot.freshness, &outputs, &[]);
    assert!(expired.values.is_empty());
    assert_eq!(status["channels"]["airspeed_kts"]["status"], "unavailable");
    assert!(
        snapshot.project(now, &[], &[]).1["channels"]
            .as_object()
            .unwrap()
            .len()
            > 1024
    );
}

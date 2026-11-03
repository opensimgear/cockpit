use super::parser::parse;
use super::*;
use std::time::{Duration, Instant};
pub(super) fn packet(sequence: u32, time: f64) -> Value {
    json!({"schemaVersion":1,"source":"osg-dcs-ownship","session":"fixture-mission","sequence":sequence,"modelTime":time,"planeId":5,"aircraft":"FA-18C_hornet","allowed":true,"paused":false,"activePause":false,"stopped":false,"values":{"flight.airspeed.true":0,"flight.ownship.inAir":false,"flight.engine.left.speedPercent":75.5}})
}
fn feed(session: &mut Session, value: Value, now: u64) -> bool {
    session
        .feed(value.to_string().as_bytes(), Instant::now(), now)
        .unwrap()
}
#[test]
fn local_source_replacement_has_distinct_identity_even_in_the_same_mission() {
    let mut first = Session::new();
    let mut second = Session::new();
    for source in [&mut first, &mut second] {
        feed(source, packet(1, 1.0), 0);
        feed(source, packet(2, 1.1), 50);
    }
    let before = first.snapshot(50);
    let after = second.snapshot(50);
    assert_ne!(before.producer_id, after.producer_id);
    assert_eq!(before.session, after.session);
    assert_eq!(before.epoch, after.epoch);
    assert_eq!(before.sequence, after.sequence);
    assert_eq!(before.producer_id, first.snapshot(100).producer_id);
}
#[test]
fn ownship_requires_advancing_identity_baseline_and_withdraws_pause_denial_and_stale() {
    let mut session = Session::new();
    feed(&mut session, packet(1, 1.0), 0);
    assert!(!session.snapshot(0).fresh);
    feed(&mut session, packet(2, 1.05), 50);
    let live = session.snapshot(50);
    assert!(live.fresh);
    assert_eq!(live.values["flight.airspeed.true"], 0);
    assert_eq!(live.values["flight.ownship.inAir"], false);
    assert!(!session.snapshot(1051).fresh);
    assert!(session.snapshot(1051).values.is_empty());
    let mut paused = packet(3, 1.1);
    paused["activePause"] = json!(true);
    feed(&mut session, paused, 100);
    assert_eq!(session.snapshot(100).status, "paused");
    assert!(session.snapshot(100).values.is_empty());
    let mut denied = packet(4, 1.15);
    denied["allowed"] = json!(false);
    feed(&mut session, denied, 150);
    assert_eq!(session.snapshot(150).status, "export-denied");
    assert!(session.snapshot(150).values.is_empty());
}
#[test]
fn late_duplicate_retired_mission_and_model_time_regression_never_replay_values() {
    let mut session = Session::new();
    feed(&mut session, packet(1, 1.0), 0);
    feed(&mut session, packet(2, 1.1), 100);
    assert!(!feed(&mut session, packet(2, 1.1), 200));
    assert_eq!(session.snapshot(200).age_ms, Some(100));
    feed(&mut session, packet(3, 1.1), 200);
    assert_eq!(session.snapshot(200).status, "paused");
    let before = session.snapshot(200).epoch;
    feed(&mut session, packet(4, 0.0), 250);
    assert!(!session.snapshot(250).fresh);
    assert!(session.snapshot(250).epoch > before);
    let mut new = packet(1, 0.0);
    new["session"] = json!("second-mission");
    feed(&mut session, new, 300);
    assert!(!feed(&mut session, packet(5, 2.0), 350));
    assert_eq!(
        session.snapshot(350).session.as_deref(),
        Some("second-mission")
    );
}

#[test]
fn rejected_duplicate_and_retired_packets_preserve_the_last_capture_instant() {
    let mut session = Session::new();
    let first_capture = Instant::now();
    session
        .feed(packet(1, 1.0).to_string().as_bytes(), first_capture, 0)
        .unwrap();
    let accepted_capture = first_capture + Duration::from_millis(50);
    session
        .feed(packet(2, 1.05).to_string().as_bytes(), accepted_capture, 50)
        .unwrap();
    assert!(session.snapshot(50).fresh);
    assert_eq!(session.snapshot(50).captured_at, Some(accepted_capture));
    assert_eq!(session.snapshot(50).capture_sequence, 2);

    assert!(!session
        .feed(
            packet(2, 1.05).to_string().as_bytes(),
            accepted_capture + Duration::from_millis(150),
            200,
        )
        .unwrap());
    assert_eq!(session.snapshot(200).captured_at, Some(accepted_capture));
    assert_eq!(session.snapshot(200).capture_sequence, 2);

    assert!(session
        .feed(
            b"invalid packet",
            accepted_capture + Duration::from_millis(250),
            300,
        )
        .is_err());
    assert_eq!(session.snapshot(300).captured_at, Some(accepted_capture));
    assert_eq!(session.snapshot(300).capture_sequence, 2);

    let mut next_mission = packet(1, 1.0);
    next_mission["session"] = json!("next-mission");
    let next_capture = accepted_capture + Duration::from_millis(350);
    session
        .feed(next_mission.to_string().as_bytes(), next_capture, 400)
        .unwrap();
    assert_eq!(session.snapshot(400).captured_at, Some(next_capture));
    assert_eq!(session.snapshot(400).capture_sequence, 3);
    assert!(!session
        .feed(
            packet(3, 1.2).to_string().as_bytes(),
            next_capture + Duration::from_millis(50),
            450,
        )
        .unwrap());
    assert_eq!(session.snapshot(450).captured_at, Some(next_capture));
    assert_eq!(session.snapshot(450).capture_sequence, 3);
}

#[test]
fn native_capture_sequence_survives_protocol_sequence_wrap() {
    let mut session = Session::new();
    feed(&mut session, packet(u32::MAX, 1.0), 0);
    assert_eq!(session.snapshot(0).sequence, Some(u32::MAX));
    assert_eq!(session.snapshot(0).capture_sequence, 1);

    feed(&mut session, packet(0, 1.05), 50);
    let wrapped = session.snapshot(50);
    assert_eq!(wrapped.sequence, Some(0));
    assert_eq!(wrapped.capture_sequence, 2);
    assert!(wrapped.fresh);
}

#[test]
fn parser_bounds_frames_drops_invalid_components_and_resets_changed_identity() {
    let mut session = Session::new();
    feed(&mut session, packet(1, 1.0), 0);
    feed(&mut session, packet(2, 1.1), 50);
    let mut wrong = packet(3, 1.2);
    wrong["values"]["flight.airspeed.true"] = json!("fast");
    assert!(session
        .feed(wrong.to_string().as_bytes(), Instant::now(), 100)
        .unwrap());
    assert!(session.snapshot(100).fresh);
    assert!(!session
        .snapshot(100)
        .values
        .contains_key("flight.airspeed.true"));
    assert_eq!(session.snapshot(100).values["flight.ownship.inAir"], false);
    assert!(session
        .feed(&vec![b' '; MAX_PACKET + 1], Instant::now(), 100)
        .is_err());
    let mut unknown = packet(4, 1.3);
    unknown["values"]["target.position"] = json!(1);
    assert!(session
        .feed(unknown.to_string().as_bytes(), Instant::now(), 100)
        .unwrap());
    assert!(!session.snapshot(100).values.contains_key("target.position"));
    let mut changed = packet(5, 1.4);
    changed["planeId"] = json!(9);
    feed(&mut session, changed, 150);
    assert!(!session.snapshot(150).fresh);
}
#[test]
fn catalogue_uses_documented_units_and_distinct_engine_percent_and_bounded_flat_ids() {
    let fields = catalogue();
    assert!(!fields.is_empty() && fields.len() <= 128);
    let ids = fields
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), fields.len());
    let speed = fields
        .iter()
        .find(|v| v["id"] == "flight.engine.left.speedPercent")
        .unwrap();
    assert_eq!(speed["unit"], "%");
    assert!(speed["description"]
        .as_str()
        .unwrap()
        .contains("not absolute"));
    for forbidden in [
        "LoSetCommand",
        "LoGetWorldObjects",
        "LoGetWingTargets",
        "LoGetPayloadInfo",
    ] {
        assert!(!export_source().contains(forbidden));
    }
}
#[test]
fn export_revision_is_a_stable_digest_of_catalogue_and_generated_source() {
    let revision = revision();
    assert_eq!(revision, super::revision());
    assert!(revision.starts_with("sha256:"));
    assert_eq!(revision.len(), 71);
}
#[test]
fn whole_physics_and_canonical_catalogue_produces_complete_typed_profile_ports() {
    let mut catalogue = json!({"channels":catalogue(),"controls":[]});
    crate::game_channels::catalogue("dcs", &mut catalogue);
    let ports = crate::profile_resource_ports::integration(&catalogue).unwrap();
    assert!(ports.len() >= 101);
    assert!(ports
        .iter()
        .any(|port| port.id == "channel:flight.engine.left.speedPercent"));
    osg_profiles::resources_ports::PortUpdate::complete(ports).unwrap();
}
#[test]
fn invalid_components_do_not_discard_valid_guards_or_other_samples() {
    let fields = catalogue()
        .iter()
        .map(|v| {
            (
                v["id"].as_str().unwrap().to_owned(),
                v["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let mut value = packet(1, 1.0);
    value["values"]["flight.airspeed.true"] = json!(1e31);
    value["values"]["flight.ownship.pilotName"] = json!("");
    let parsed = parse(value.to_string().as_bytes(), &fields).unwrap();
    assert!(!parsed.values.contains_key("flight.airspeed.true"));
    assert_eq!(parsed.values["flight.ownship.inAir"], false);
    assert_eq!(parsed.values["flight.ownship.pilotName"], "");
    value["allowed"] = json!("true");
    assert!(parse(value.to_string().as_bytes(), &fields).is_err());
}

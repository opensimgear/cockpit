use super::*;
use crate::game_telemetry::{
    parse_for_game, CaptureOwner, Game, RawSample, SourceIdentity, EVO_CAR_MODEL_OFFSET,
    EVO_GRAPHICS_SIZE, EVO_STATIC_SIZE, EVO_TRACK_OFFSET, PHYSICS_EXTENDED_SIZE,
};
use serde_json::json;

fn sample(game: Game, current_maximum: i32) -> RawSample {
    let mut sample = RawSample {
        source: SourceIdentity {
            id: "projection-test".into(),
            epoch: 1,
        },
        physics: include_bytes!("../../../tests/fixtures/acr/live.physics.bin").to_vec(),
        graphics: include_bytes!("../../../tests/fixtures/acr/live.graphics.bin").to_vec(),
        statics: include_bytes!("../../../tests/fixtures/acr/live.static.bin").to_vec(),
        capture: None,
    };
    sample.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
    sample.physics[588..592].copy_from_slice(&current_maximum.to_le_bytes());
    if game == Game::Evo {
        sample.graphics.resize(EVO_GRAPHICS_SIZE, 0);
        sample.graphics[EVO_CAR_MODEL_OFFSET..EVO_CAR_MODEL_OFFSET + 7].copy_from_slice(b"evo-car");
        sample.statics = vec![0; EVO_STATIC_SIZE];
        sample.statics[..3].copy_from_slice(b"1.0");
        sample.statics[EVO_TRACK_OFFSET..EVO_TRACK_OFFSET + 5].copy_from_slice(b"imola");
    } else {
        let version = if game == Game::Corsa { "1.7" } else { "1.9" };
        sample.statics[..30].fill(0);
        for (index, word) in version.encode_utf16().enumerate() {
            sample.statics[index * 2..index * 2 + 2].copy_from_slice(&word.to_le_bytes());
        }
    }
    sample
}
fn snapshot(game: Game, current_maximum: i32, now: Instant) -> Snapshot {
    let reading = parse_for_game(&sample(game, current_maximum), game).unwrap();
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let physics = owner.capture(now - Duration::from_millis(30)).unwrap();
    let graphics = owner.capture(now - Duration::from_millis(20)).unwrap();
    let statics = owner.capture(now - Duration::from_millis(10)).unwrap();
    Snapshot {
        game,
        epoch: 1,
        fresh: true,
        age_ms: Some(30),
        status: "active".into(),
        source: None,
        context: Some(reading.context),
        packet_id: Some(reading.packet_id),
        capture: Some(CaptureSet::new(&owner, physics, graphics, statics).unwrap()),
        freshness_ms: 100,
        maximum_input: reading.maximum_input,
        channels: reading.channels,
        error: None,
    }
}
fn inputs(value: &CapturedValue) -> Vec<&str> {
    value
        .contributors()
        .unwrap()
        .iter()
        .map(|contribution| match &contribution.input {
            InputId::Channel(name) => name.as_ref(),
            _ => panic!("Assetto requires native field identities"),
        })
        .collect()
}
#[test]
fn all_dialects_keep_exact_native_fields_original_page_stamps_and_deadlines() {
    let now = Instant::now();
    for game in [Game::Corsa, Game::Competizione, Game::Evo, Game::Rally] {
        let snapshot = snapshot(game, 7500, now);
        let values = snapshot.values_at(now);
        let capture = snapshot.capture.as_ref().unwrap();
        let version_input = if game == Game::Evo {
            "static.sm_version"
        } else {
            "static.smVersion"
        };
        for (alias, input, page) in [
            ("rpm", "physics.rpms", capture.physics),
            ("gear", "physics.gear", capture.physics),
            ("speed_kmh", "physics.speedKmh", capture.physics),
            ("graphics.status", "graphics.status", capture.graphics),
            (version_input, version_input, capture.statics),
        ] {
            let value = &values[alias];
            assert_eq!(inputs(value), vec![input], "{} {alias}", game.id());
            let contribution = &value.contributors().unwrap()[0];
            assert_eq!(
                contribution.capture.producer_id.as_ref(),
                capture.producer_id
            );
            assert_eq!(contribution.capture.generation, capture.generation);
            assert_eq!(contribution.capture.sequence, page.sequence);
            assert_eq!(contribution.capture.captured_at, page.captured_at);
            assert_eq!(contribution.capture.byte_offset, None);
            assert_eq!(
                contribution.validity,
                Validity::Sampled {
                    valid_until: page.captured_at + Duration::from_millis(100),
                }
            );
        }
        let maximum = if matches!(game, Game::Evo | Game::Rally) {
            "physics.currentMaxRpm"
        } else {
            "static.maxRpm"
        };
        assert_eq!(inputs(&values["max_rpm"]), vec![maximum]);
        let mut ratio_inputs = vec![maximum, "physics.rpms"];
        ratio_inputs.sort_unstable();
        assert_eq!(inputs(&values["rev_ratio"]), ratio_inputs);
        // Equal dynamic/static values must still name the source actually selected at decode.
        assert_eq!(snapshot.maximum_input, Some(maximum));
        assert_eq!(
            values["engine.rpm"].contributors(),
            values["rpm"].contributors()
        );
        assert_eq!(
            values["vehicle.speed_mps"].contributors(),
            values["speed_kmh"].contributors()
        );
        let speed=values["vehicle.speed_mps"].value().as_f64().unwrap();
        assert!((speed-124.25/3.6).abs()<1e-12);
        assert_eq!(
            values["physics.rpms"].contributors(),
            values["rpm"].contributors()
        );
    }
}
#[test]
fn invalid_current_limits_use_only_the_actual_valid_static_fallback() {
    let now = Instant::now();
    let rally = snapshot(Game::Rally, 0, now);
    let values = rally.values_at(now);
    assert_eq!(rally.maximum_input, Some("static.maxRpm"));
    assert_eq!(inputs(&values["max_rpm"]), vec!["static.maxRpm"]);
    assert_eq!(
        inputs(&values["rev_ratio"]),
        vec!["physics.rpms", "static.maxRpm"]
    );
    let contributions = values["rev_ratio"].contributors().unwrap();
    assert_eq!(
        contributions[0].capture.captured_at,
        rally.capture.as_ref().unwrap().physics.captured_at
    );
    assert_eq!(
        contributions[1].capture.captured_at,
        rally.capture.as_ref().unwrap().statics.captured_at
    );
    assert!(!values["rev_ratio"].is_current(now + Duration::from_millis(70)));
    assert!(values["max_rpm"].is_current(now + Duration::from_millis(70)));
    let evo = snapshot(Game::Evo, 0, now).values_at(now);
    assert!(!evo.contains_key("max_rpm"));
    assert!(!evo.contains_key("rev_ratio"));
    assert!(!evo.contains_key("engine.max_rpm"));
}
#[test]
fn deferred_future_and_inactive_native_projections_fail_closed() {
    let now = Instant::now();
    let mut snapshot = snapshot(Game::Rally, 8500, now);
    assert!(!snapshot.values_at(now).is_empty());
    assert!(snapshot
        .values_at(now + Duration::from_millis(101))
        .is_empty());
    assert!(snapshot
        .values_at(now - Duration::from_millis(31))
        .is_empty());
    // Native scalar validity uses a strict deadline even at the session's age boundary.
    assert!(!snapshot
        .values_at(now + Duration::from_millis(70))
        .contains_key("rpm"));
    snapshot.fresh = false;
    assert!(snapshot.values_at(now).is_empty());
}
#[test]
fn authored_values_are_explicit_fixtures_and_unknown_channels_are_excluded() {
    let now = Instant::now();
    let mut snapshot = snapshot(Game::Rally, 8500, now);
    snapshot.capture = None;
    snapshot.channels.get_mut("rpm").unwrap().status = ChannelStatus::Unknown;
    let values = snapshot.values_at(now);
    assert!(!values.contains_key("rpm"));
    assert!(!values.contains_key("engine.rpm"));
    assert!(!values.is_empty());
    assert!(values.values().all(|value| value.contributors().is_none()));
}

use super::*;
use crate::game_telemetry::{parse_for_game, ChannelStatus, Session, SourceIdentity};
use std::collections::BTreeSet;

fn sample(game: Game) -> RawSample {
    let sizes = game.full_page_lengths();
    let mut sample = RawSample {
        source: SourceIdentity { id: "authored-schema-fixture".into(), epoch: 1 },
        physics: vec![0; sizes.0], graphics: vec![0; sizes.1], statics: vec![0; sizes.2],
    };
    sample.graphics[4..8].copy_from_slice(&2i32.to_le_bytes());
    sample.physics[16..20].copy_from_slice(&3i32.to_le_bytes());
    sample.physics[20..24].copy_from_slice(&4200i32.to_le_bytes());
    sample.physics[28..32].copy_from_slice(&88.5f32.to_le_bytes());
    if game == Game::Evo {
        sample.statics[..3].copy_from_slice(b"1.0");
        sample.statics[15..20].copy_from_slice(b"0.9.1");
        sample.graphics[3086..3089].copy_from_slice(b"car");
        sample.statics[136..141].copy_from_slice(b"track");
    } else {
        let version = if game == Game::Competizione { "1.9" } else { "1.7" };
        for (offset, text) in [(0, version), (30, "fixture"), (68, "car"), (134, "track")] {
            for (index, word) in text.encode_utf16().enumerate() {
                sample.statics[offset+index*2..offset+index*2+2].copy_from_slice(&word.to_le_bytes());
            }
        }
        sample.statics[412..416].copy_from_slice(&8400i32.to_le_bytes());
    }
    if matches!(game, Game::Evo | Game::Rally) {
        sample.physics[588..592].copy_from_slice(&8400i32.to_le_bytes());
    }
    sample
}

#[test]
fn factual_layouts_have_unique_flattened_ids_and_exact_documented_boundaries() {
    for (game, expected) in [(Game::Corsa, (580,296,684)),(Game::Competizione,(800,1588,820)),(Game::Evo,(800,3940,208))] {
        assert_eq!(page_lengths(game), expected);
        let definitions = catalogue(game);
        let unique = definitions.iter().map(|row|row["id"].as_str().unwrap()).collect::<BTreeSet<_>>();
        assert_eq!(definitions.len(), unique.len());
        assert!(definitions.len()>200);
        for row in definitions {
            assert_eq!(row["sourceGame"], game.id());
            assert!(row["nativeField"].is_string() && row["source"].is_string());
            assert!(matches!(row["type"].as_str(),Some("number"|"integer"|"boolean"|"string")));
        }
    }
    let ac = channels(Game::Corsa);
    assert!(!ac.iter().any(|row|row.id=="physics.brakePressure.0"));
    assert_eq!(channels(Game::Evo).iter().find(|row|row.id=="graphics.car_model").unwrap().offset,3086);
}

#[test]
fn finite_vector_components_decode_independently_and_bad_optional_text_does_not_poison_the_frame() {
    let mut source = sample(Game::Competizione);
    for (offset, number) in [(44, 1.25f32),(48,f32::NAN),(52,-2.5)] {
        source.physics[offset..offset+4].copy_from_slice(&number.to_le_bytes());
    }
    let text = channels(Game::Competizione).iter().find(|row|row.id=="graphics.currentTime").unwrap();
    source.graphics[text.offset..text.offset+2].copy_from_slice(&0xd800u16.to_le_bytes());
    let reading = parse_for_game(&source, Game::Competizione).unwrap();
    assert_eq!(reading.channels["physics.accG.0"].value,Some(json!(1.25)));
    assert_eq!(reading.channels["physics.accG.1"].status,ChannelStatus::Unknown);
    assert_eq!(reading.channels["physics.accG.2"].value,Some(json!(-2.5)));
    assert_eq!(reading.channels["graphics.currentTime"].status,ChannelStatus::Unknown);
    assert_eq!(reading.channels["rpm"].value,Some(json!(4200)));
    assert_eq!(reading.channels["static.carModel"].value,Some(json!("car")));
}

#[test]
fn unused_acc_and_unverified_rally_fields_never_become_processing_values_even_when_nonzero() {
    for game in [Game::Competizione,Game::Rally] {
        let mut source = sample(game);
        source.physics.resize(800,0);
        source.physics[592..596].copy_from_slice(&123.0f32.to_le_bytes());
        source.physics[44..48].copy_from_slice(&1.25f32.to_le_bytes());
        let reading = parse_for_game(&source, game).unwrap();
        assert_eq!(reading.channels["physics.mz.0"].status,ChannelStatus::Unknown);
        assert!(reading.channels["physics.mz.0"].value.is_none());
        if game == Game::Rally {
            assert_eq!(reading.channels["physics.accG.0"].status,ChannelStatus::Unknown);
            assert_eq!(reading.channels["physics.rpms"].value,Some(json!(4200)));
        }
    }
}

#[test]
fn evo_nested_fields_boolean_widths_and_uint64_identifiers_use_exact_native_types() {
    let mut source=sample(Game::Evo);
    source.graphics[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    source.graphics[42]=1; // bool immediately following uint16 HUD RPM.
    source.graphics[43]=2; // Invalid bool is unknown, not truthy.
    source.graphics[232..236].copy_from_slice(&91.5f32.to_le_bytes());
    source.physics[592..596].copy_from_slice(&(-15.5f32).to_le_bytes());
    let reading=parse_for_game(&source,Game::Evo).unwrap();
    assert_eq!(reading.channels["graphics.focused_car_id_a"].value,Some(json!(u64::MAX.to_string())));
    assert_eq!(reading.channels["graphics.is_rpm_limiter_on"].value,Some(json!(true)));
    assert_eq!(reading.channels["graphics.is_change_up_rpm"].status,ChannelStatus::Unknown);
    assert_eq!(reading.channels["graphics.tyre_lf.tyre_temperature_c"].value,Some(json!(91.5)));
    assert_eq!(reading.channels["physics.mz.0"].value,Some(json!(-15.5)));
}

#[test]
fn missing_tails_and_unqualified_versions_are_unknown_while_legacy_ports_keep_working() {
    let mut source=sample(Game::Corsa);
    source.physics.truncate(32);source.graphics.truncate(8);source.statics.truncate(416);
    let reading=parse_for_game(&source,Game::Corsa).unwrap();
    assert_eq!(reading.channels["physics.gas"].value,Some(json!(0.0)));
    assert_eq!(reading.channels["physics.accG.0"].status,ChannelStatus::Unknown);
    assert_eq!(reading.channels["rpm"].value,Some(json!(4200)));
    source.statics[..6].copy_from_slice(&[b'9',0,b'.',0,b'9',0]);
    let reading=parse_for_game(&source,Game::Corsa).unwrap();
    assert_eq!(reading.channels["physics.gas"].status,ChannelStatus::Unknown);
    assert_eq!(reading.channels["rpm"].value,Some(json!(4200)));
}

#[test]
fn expanded_channels_obey_original_packet_progression_live_state_and_stale_withdrawal() {
    let mut source=sample(Game::Evo);
    let mut session=Session::new_for_game(100,Game::Evo).unwrap();
    assert!(!session.feed(&source,0).fresh);
    source.physics[..4].copy_from_slice(&1u32.to_le_bytes());
    let fresh=session.feed(&source,1);
    assert_eq!(fresh.values()["physics.accG.0"],json!(0.0));
    assert!(fresh.values().contains_key("graphics.tyre_lf.tyre_temperature_c"));
    assert!(session.tick(102).values().is_empty());
    assert!(!session.feed(&source,103).fresh);
    source.physics[..4].copy_from_slice(&2u32.to_le_bytes());
    assert!(session.feed(&source,104).fresh);
    source.graphics[4..8].copy_from_slice(&3i32.to_le_bytes());
    assert!(session.feed(&source,105).values().is_empty());
}

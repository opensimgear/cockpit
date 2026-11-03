    use super::*;
    fn evo_sample(packet: u32) -> RawSample {
        let mut sample = RawSample {
            source: SourceIdentity { id: Game::Evo.source_id(42), epoch: 1 },
            physics: vec![0; PHYSICS_EXTENDED_SIZE],
            graphics: vec![0; EVO_GRAPHICS_SIZE],
            statics: vec![0; EVO_STATIC_SIZE],
            capture: None,
        };
        advance(&mut sample, packet);
        sample.physics[16..20].copy_from_slice(&3i32.to_le_bytes());
        sample.physics[20..24].copy_from_slice(&4200i32.to_le_bytes());
        sample.physics[28..32].copy_from_slice(&88.5f32.to_le_bytes());
        sample.physics[588..592].copy_from_slice(&8400i32.to_le_bytes());
        sample.graphics[4..8].copy_from_slice(&2i32.to_le_bytes());
        sample.graphics[3086..3094].copy_from_slice(b"evo-car\0");
        sample.statics[0..4].copy_from_slice(b"1.0\0");
        sample.statics[15..21].copy_from_slice(b"0.9.1\0");
        sample.statics[136..142].copy_from_slice(b"imola\0");
        sample
    }
    #[test]
    fn original_corsa_and_competizione_use_static_limits_and_require_live_status() {
        for game in [Game::Corsa, Game::Competizione] {
            let mut s = sample();
            s.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
            s.physics[588..592].copy_from_slice(&9000i32.to_le_bytes());
            assert_eq!(parse_for_game(&s, game).unwrap().channels["max_rpm"].value, Some(json!(7500)));
            let mut session = Session::new_for_game(100, game).unwrap();
            assert!(!session.feed(&s, 0).fresh);
            advance(&mut s, 11);
            assert!(session.feed(&s, 1).fresh);
            assert_eq!(session.snapshot(1).value()["provider"], game.id());
            s.statics[..60].fill(0);
            s.graphics[4..8].fill(0);
            advance(&mut s, 12);
            assert!(!parse_for_game(&s, game).unwrap().sparse_status());
            assert!(!session.feed(&s, 2).fresh);
            s.graphics[4..8].copy_from_slice(&2i32.to_le_bytes());
            assert!(!session.feed(&s, 3).fresh);
            advance(&mut s, 13);
            assert!(session.feed(&s, 4).fresh);
            assert!(!session.tick(105).fresh);
        }
    }
    #[test]
    fn evo_decodes_char_metadata_graphics_car_and_current_physics_integer_limit() {
        let s = evo_sample(20);
        let reading = parse_for_game(&s, Game::Evo).unwrap();
        assert_eq!(reading.context, Context {
            shared_memory_version: "1.0".into(), game_version: "0.9.1".into(),
            car_model: "evo-car".into(), track: "imola".into(),
        });
        assert_eq!(reading.channels["rpm"].value, Some(json!(4200)));
        assert_eq!(reading.channels["gear"].value, Some(json!(2)));
        assert_eq!(reading.channels["speed_kmh"].value, Some(json!(88.5)));
        assert_eq!(reading.channels["max_rpm"].value, Some(json!(8400)));
        assert_eq!(reading.channels["rev_ratio"].value, Some(json!(0.5)));
        assert!(parse(&s).is_err());
        let mut queue = FixtureSource::new_for_game(Game::Evo);
        queue.push(s.clone()).unwrap();
        let mut session = Session::new_for_game(100, Game::Evo).unwrap();
        assert!(!session.poll(&mut queue, 0).fresh);
        queue.push(evo_sample(21)).unwrap();
        assert!(session.poll(&mut queue, 1).fresh);
        assert_eq!(session.snapshot(1).value()["provider"], "assetto-corsa-evo");
        for field in ["graphics", "static"] {
            let mut short = s.clone();
            if field == "graphics" { short.graphics.pop(); } else { short.statics.pop(); }
            assert!(parse_for_game(&short, Game::Evo).is_err());
        }
        let mut invalid = s.clone();
        invalid.graphics[EVO_CAR_MODEL_OFFSET] = 0xff;
        assert!(parse_for_game(&invalid, Game::Evo).is_err());
    }
    #[test]
    fn evo_never_reinterprets_legacy_static_bytes_or_float_bits_as_rpm() {
        let mut s = evo_sample(1);
        s.statics.resize(STATIC_SIZE, 0);
        s.statics[412..416].copy_from_slice(&7500i32.to_le_bytes());
        for value in [0i32, -1, 100001, 8400f32.to_bits() as i32] {
            s.physics[588..592].copy_from_slice(&value.to_le_bytes());
            let reading = parse_for_game(&s, Game::Evo).unwrap();
            assert!(reading.channels["max_rpm"].value.is_none());
            assert!(reading.channels["rev_ratio"].value.is_none());
        }
        s.graphics[4..8].fill(0);
        s.statics[..30].fill(0);
        assert!(!parse_for_game(&s, Game::Evo).unwrap().telemetry_active());
        assert_eq!(Game::Evo.mapping_prefix(), "Local\\acevo_pmf");
        for game in [Game::Corsa, Game::Competizione, Game::Rally] {
            assert_eq!(game.mapping_prefix(), "Local\\acpmf");
            assert_ne!(game.source_id(42), Game::Evo.source_id(42));
            assert_eq!(catalogue_for_game(game.id())["provider"], game.id());
        }
        assert!(Game::from_id("unrelated").is_err());
        assert!(catalogue_for_game("unrelated").is_null());
    }
    fn sample() -> RawSample {
        RawSample {
            source: SourceIdentity {
                id: "fixture".into(),
                epoch: 1,
            },
            physics: include_bytes!("../../../tests/fixtures/acr/live.physics.bin").to_vec(),
            graphics: include_bytes!("../../../tests/fixtures/acr/live.graphics.bin").to_vec(),
            statics: include_bytes!("../../../tests/fixtures/acr/live.static.bin").to_vec(),
            capture: None,
        }
    }
    fn advance(sample: &mut RawSample, packet: u32) {
        sample.physics[..4].copy_from_slice(&packet.to_le_bytes());
    }
    #[test]
    fn observed_acr_sparse_status_qualifies_progression_without_inventing_metadata() {
        let mut s = sample();
        s.graphics[4..8].fill(0);
        s.statics[..60].fill(0);
        s.statics[412..416].fill(0);
        let mut session = Session::new(100).unwrap();
        assert!(parse(&s).unwrap().sparse_status());
        assert!(!session.feed(&s, 0).fresh);
        advance(&mut s, 11);
        let snapshot = session.feed(&s, 1);
        assert!(snapshot.fresh);
        assert_eq!(*snapshot.values()["rpm"].value(), json!(6300));
        assert!(!snapshot.values().contains_key("max_rpm"));
        assert!(!session.feed(&s, 102).fresh);
        for status in [1i32, 3] {
            s.graphics[4..8].copy_from_slice(&status.to_le_bytes());
            advance(&mut s, 12 + status as u32);
            assert!(!session.feed(&s, 103).fresh);
        }
        s.graphics[4..8].fill(0);
        s.statics[68..134].fill(0);
        assert!(!parse(&s).unwrap().telemetry_active());
        s = sample();
        s.graphics[4..8].fill(0);
        assert!(!parse(&s).unwrap().telemetry_active());
    }

    #[test]
    fn authored_prefixes_decode_real_fields_and_typed_catalogue() {
        let reading = parse(&sample()).unwrap();
        assert_eq!(reading.packet_id, 10);
        assert_eq!(reading.status, 2);
        assert_eq!(reading.channels["rpm"].value, Some(json!(6300)));
        assert_eq!(reading.channels["gear"].value, Some(json!(4)));
        assert_eq!(reading.channels["speed_kmh"].value, Some(json!(124.25)));
        assert_eq!(reading.channels["max_rpm"].value, Some(json!(7500)));
        assert_eq!(reading.channels["rev_ratio"].value, Some(json!(0.84)));
        assert_eq!(reading.context.car_model, "research-rally");
        let catalogue = catalogue();
        assert!(catalogue["commands"].as_array().unwrap().is_empty());
        assert_eq!(catalogue["channels"].as_array().unwrap()[..5].iter().map(|row|row["id"].as_str().unwrap()).collect::<Vec<_>>(), CHANNELS);
        let mut s = sample();
        s.physics = include_bytes!("../../../tests/fixtures/acr/reverse.physics.bin").to_vec();
        assert_eq!(parse(&s).unwrap().channels["gear"].value, Some(json!(-1)));
        s.physics = include_bytes!("../../../tests/fixtures/acr/neutral.physics.bin").to_vec();
        assert_eq!(parse(&s).unwrap().channels["gear"].value, Some(json!(0)));
    }
    #[test]
    fn parser_rejects_short_oversized_invalid_identity_status_and_utf16() {
        for field in ["physics", "graphics", "static"] {
            let mut s = sample();
            let page = match field {
                "physics" => &mut s.physics,
                "graphics" => &mut s.graphics,
                _ => &mut s.statics,
            };
            page.clear();
            assert!(parse(&s).is_err());
        }
        let mut s = sample();
        s.physics.resize(MAX_PAGE + 1, 0);
        assert!(parse(&s).is_err());
        for id in ["", "invalid id", "\n", "a".repeat(65).as_str()] {
            let mut s = sample();
            s.source.id = id.into();
            assert!(parse(&s).is_err());
        }
        let mut s = sample();
        s.source.epoch = 0;
        assert!(parse(&s).is_err());
        s = sample();
        s.graphics[4..8].copy_from_slice(&99i32.to_le_bytes());
        assert!(parse(&s).is_err());
        s = sample();
        s.statics[68..70].copy_from_slice(&0xd800u16.to_le_bytes());
        assert!(parse(&s).is_err());
        s = sample();
        s.statics[68..70].copy_from_slice(&10u16.to_le_bytes());
        assert!(parse(&s).is_err());
    }
    #[test]
    fn invalid_numeric_fields_are_independently_unknown_and_maximum_is_never_guessed() {
        let mut s = sample();
        s.physics[20..24].copy_from_slice(&(-1i32).to_le_bytes());
        let reading = parse(&s).unwrap();
        assert_eq!(reading.channels["rpm"].status, ChannelStatus::Unknown);
        assert_eq!(reading.channels["gear"].status, ChannelStatus::Ready);
        assert!(reading.channels["rev_ratio"].value.is_none());
        s = sample();
        s.statics[412..416].fill(0);
        let reading = parse(&s).unwrap();
        assert!(reading.channels["max_rpm"].value.is_none());
        assert!(reading.channels["rev_ratio"].value.is_none());
        assert!(reading.channels["rpm"].value.is_some());
        s = sample();
        s.physics[16..20].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(parse(&s).unwrap().channels["gear"].value.is_none());
        for speed in [f32::NAN, f32::INFINITY, -1.0, 2001.0] {
            s = sample();
            s.physics[28..32].copy_from_slice(&speed.to_le_bytes());
            assert!(parse(&s).unwrap().channels["speed_kmh"].value.is_none());
        }
    }
    #[test]
    fn current_physics_limit_overrides_static_maximum_and_supports_sparse_acr() {
        let mut s = sample();
        s.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
        for maximum in [1i32, 8500, 100000] {
            s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                .copy_from_slice(&maximum.to_le_bytes());
            for static_maximum in [0i32, 7500] {
                s.statics[412..416].copy_from_slice(&static_maximum.to_le_bytes());
                let reading = parse(&s).unwrap();
                assert_eq!(reading.channels["max_rpm"].value, Some(json!(maximum)));
                assert_eq!(
                    reading.channels["rev_ratio"].value,
                    Some(json!(6300.0 / maximum as f64))
                );
            }
        }
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
            .copy_from_slice(&8500i32.to_le_bytes());
        s.statics[..60].fill(0);
        s.graphics[4..8].fill(0);
        let reading = parse(&s).unwrap();
        assert!(reading.sparse_status());
        assert_eq!(reading.channels["max_rpm"].value, Some(json!(8500)));
    }
    #[test]
    fn invalid_current_limits_fall_back_only_to_valid_static_integer_limits() {
        let mut s = sample();
        s.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
        for maximum in [
            0i32,
            -1,
            100001,
            i32::MIN,
            i32::MAX,
            8500f32.to_bits() as i32,
            f32::NAN.to_bits() as i32,
            f32::INFINITY.to_bits() as i32,
        ] {
            s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                .copy_from_slice(&maximum.to_le_bytes());
            s.statics[412..416].copy_from_slice(&7500i32.to_le_bytes());
            let reading = parse(&s).unwrap();
            assert_eq!(reading.channels["max_rpm"].value, Some(json!(7500)));
            assert_eq!(reading.channels["rev_ratio"].value, Some(json!(0.84)));
            for static_maximum in [0i32, -1, 100001] {
                s.statics[412..416].copy_from_slice(&static_maximum.to_le_bytes());
                let reading = parse(&s).unwrap();
                assert_eq!(reading.channels["max_rpm"].status, ChannelStatus::Unknown);
                assert!(reading.channels["max_rpm"].value.is_none());
                assert!(reading.channels["rev_ratio"].value.is_none());
                assert_eq!(reading.channels["rpm"].value, Some(json!(6300)));
            }
        }
    }
    #[test]
    fn incomplete_physics_extensions_use_legacy_static_maximum_without_partial_reads() {
        for length in PHYSICS_SIZE..PHYSICS_EXTENDED_SIZE {
            let mut s = sample();
            s.physics.resize(length, 0xff);
            let reading = parse(&s).unwrap();
            assert_eq!(reading.channels["max_rpm"].value, Some(json!(7500)));
            s.statics[412..416].fill(0);
            assert!(parse(&s).unwrap().channels["max_rpm"].value.is_none());
        }
    }
    #[test]
    fn current_limit_obeys_packet_freshness_and_is_not_retained_when_missing() {
        let mut s = sample();
        s.physics.resize(PHYSICS_EXTENDED_SIZE, 0);
        s.statics[412..416].fill(0);
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
            .copy_from_slice(&8500i32.to_le_bytes());
        let mut session = Session::new(100).unwrap();
        assert!(!session.feed(&s, 0).fresh);
        advance(&mut s, 11);
        assert_eq!(*session.feed(&s, 1).values()["max_rpm"].value(), json!(8500));
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE].fill(0);
        // A changed limit without a new physics packet does not update channels.
        assert_eq!(*session.feed(&s, 50).values()["max_rpm"].value(), json!(8500));
        advance(&mut s, 12);
        let missing = session.feed(&s, 51);
        assert!(missing.fresh);
        assert_eq!(missing.channels["max_rpm"].status, ChannelStatus::Unknown);
        assert!(!missing.values().contains_key("rev_ratio"));
        s.physics[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
            .copy_from_slice(&9000i32.to_le_bytes());
        s.statics[68..70].copy_from_slice(&(b'X' as u16).to_le_bytes());
        assert!(!session.feed(&s, 52).fresh);
        advance(&mut s, 13);
        assert_eq!(*session.feed(&s, 53).values()["max_rpm"].value(), json!(9000));
        let stale = session.feed(&s, 154);
        assert!(!stale.fresh);
        assert_eq!(stale.channels["max_rpm"].status, ChannelStatus::Stale);
        assert!(stale.values().is_empty());
    }
    #[test]
    fn baseline_progression_freeze_timeout_recovery_and_u32_wrap_are_guarded() {
        assert!(Session::new(99).is_err());
        assert!(Session::new(10001).is_err());
        let mut session = Session::new(100).unwrap();
        let mut s = sample();
        assert!(!session.feed(&s, 0).fresh);
        assert!(!session.feed(&s, 1).fresh);
        advance(&mut s, 11);
        let fresh = session.feed(&s, 2);
        assert!(fresh.fresh);
        assert!(CHANNELS.iter().all(|id|fresh.values().contains_key(*id)));
        let epoch = fresh.epoch;
        assert!(!session.tick(103).fresh);
        let stale_epoch = session.snapshot(103).epoch;
        assert!(stale_epoch > epoch);
        for time in [104, 205, 400] {
            s.physics[20..24].copy_from_slice(&9000i32.to_le_bytes());
            let frozen = session.feed(&s, time);
            assert!(!frozen.fresh);
            assert_eq!(frozen.packet_id, Some(11));
            assert_eq!(frozen.epoch, stale_epoch);
            assert!(frozen.values().is_empty());
        }
        advance(&mut s, 12);
        assert!(session.feed(&s, 401).fresh);
        advance(&mut s, 0);
        assert!(!session.feed(&s, 402).fresh);
        assert!(!session.feed(&s, 403).fresh);
        advance(&mut s, 1);
        assert!(session.feed(&s, 404).fresh);
        session.reset();
        advance(&mut s, u32::MAX);
        assert!(!session.feed(&s, 500).fresh);
        advance(&mut s, 0);
        assert!(session.feed(&s, 501).fresh);
        assert!(!session.tick(499).fresh);
    }
    #[test]
    fn pause_replay_off_context_and_process_changes_need_current_baselines() {
        let mut session = Session::new(1000).unwrap();
        let mut s = sample();
        session.feed(&s, 0);
        advance(&mut s, 11);
        session.feed(&s, 1);
        for status in [3i32, 1, 0] {
            s.graphics[4..8].copy_from_slice(&status.to_le_bytes());
            let inactive = session.feed(&s, 2);
            assert!(!inactive.fresh);
            assert_eq!(inactive.status, "inactive");
            let epoch = inactive.epoch;
            assert_eq!(session.feed(&s, 3).epoch, epoch);
        }
        s.graphics[4..8].copy_from_slice(&2i32.to_le_bytes());
        assert!(!session.feed(&s, 4).fresh);
        advance(&mut s, 12);
        assert!(session.feed(&s, 5).fresh);
        s.statics[134..136].copy_from_slice(&(b'X' as u16).to_le_bytes());
        assert!(!session.feed(&s, 6).fresh);
        advance(&mut s, 13);
        assert!(session.feed(&s, 7).fresh);
        s.source.epoch = 2;
        assert!(!session.feed(&s, 8).fresh);
        advance(&mut s, 14);
        let fresh = session.feed(&s, 9);
        assert!(fresh.fresh);
        assert_eq!(fresh.value()["source"]["epoch"], "2");
        assert_eq!(fresh.value()["channels"]["rpm"]["status"], "ready");
        assert_eq!(fresh.value()["context"]["track"], "Xesearch-stage");
    }
    #[test]
    fn raw_byte_arrays_and_optional_hex_use_the_same_bounded_parser_and_fixture_source() {
        let s = sample();
        let frame = json!({"sourceEpoch":"1","physics":s.physics,"graphics":s.graphics,"staticPage":s.statics});
        let parsed = RawSample::parse_json(&frame).unwrap();
        assert_eq!(parsed.source.id, "synthetic");
        let mut source = FixtureSource::new();
        source.push(parsed.clone()).unwrap();
        let mut session = Session::new(100).unwrap();
        assert!(!session.poll(&mut source, 0).fresh);
        let mut next = parsed.clone();
        advance(&mut next, 11);
        source.push(next).unwrap();
        assert!(session.poll(&mut source, 1).fresh);
        assert!(!session.poll(&mut source, 102).fresh);
        for (field, value) in [
            ("sourceEpoch", json!("0")),
            ("sourceEpoch", json!("-1")),
            ("sourceEpoch", json!("18446744073709551616")),
            ("physics", json!([256])),
            ("physics", json!([1.2])),
            ("physics", json!("0G")),
            ("physics", json!("0")),
            ("extra", json!(true)),
        ] {
            let mut v = frame.clone();
            v[field] = value;
            assert!(RawSample::parse_json(&v).is_err(), "{field}");
        }
        let mut hex = frame.clone();
        hex["sourceEpoch"] = json!(1);
        hex["physics"] = json!(s
            .physics
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>());
        assert!(RawSample::parse_json(&hex).is_ok());
        for _ in 0..256 {
            source.push(parsed.clone()).unwrap();
        }
        assert!(source.push(parsed).is_err());
        source.close();
        assert!(source.poll().unwrap().is_none());
    }
    #[test]
    fn acquisition_errors_disarm_without_refreshing_frozen_source_data() {
        struct Broken(bool);
        impl Source for Broken {
            fn poll(&mut self) -> Result<Option<RawSample>, String> {
                Err("source gone".into())
            }
            fn close(&mut self) {
                self.0 = true;
            }
        }
        let mut session = Session::new(100).unwrap();
        let mut s = sample();
        session.feed(&s, 0);
        advance(&mut s, 11);
        session.feed(&s, 1);
        let mut broken = Broken(false);
        let failed = session.poll(&mut broken, 2);
        assert!(!failed.fresh);
        assert_eq!(failed.status, "error");
        assert!(broken.0);
        assert_eq!(failed.error.as_deref(), Some("source gone"));
        assert!(!session.feed(&s, 3).fresh);
        advance(&mut s, 12);
        assert!(session.feed(&s, 4).fresh);
        s.graphics[4..8].copy_from_slice(&99i32.to_le_bytes());
        let invalid = session.feed(&s, 5);
        assert!(!invalid.fresh);
        assert_eq!(invalid.status, "error");
        assert_eq!(session.feed(&s, 6).epoch, invalid.epoch);
    }

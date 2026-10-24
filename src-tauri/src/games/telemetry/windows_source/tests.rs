use super::*;
fn bytes(page: Page) -> Vec<u8> {
    match page {
        Page::Physics => {
            include_bytes!("../../../../../tests/fixtures/acr/live.physics.bin").to_vec()
        }
        Page::Graphics => {
            include_bytes!("../../../../../tests/fixtures/acr/live.graphics.bin").to_vec()
        }
        Page::Statics => {
            include_bytes!("../../../../../tests/fixtures/acr/live.static.bin").to_vec()
        }
    }
}
fn identity() -> SourceIdentity {
    SourceIdentity {
        id: "fixture".into(),
        epoch: 1,
    }
}
#[test]
fn stable_owned_page_copies_match_the_production_parser() {
    let sample = consistent_copy(identity(), &mut |page| Ok(bytes(page))).unwrap();
    assert_eq!(
        parse(&sample).unwrap().channels["rpm"].value,
        Some(serde_json::json!(6300))
    );
    let mut copies = 0;
    let sample = consistent_copy(identity(), &mut |page| {
        copies += 1;
        let mut data = bytes(page);
        data.extend([1, 2, 3]);
        Ok(data)
    })
    .unwrap();
    assert_eq!(sample.source, identity());
    assert_eq!(copies, 6);
    assert_eq!(sample.statics.len(), STATIC_SIZE + 3);
}
#[test]
fn expanded_physics_and_graphics_fields_participate_in_stable_snapshot_checks() {
    for target in [Page::Physics, Page::Graphics] {
        let mut target_reads = 0;
        let result = consistent_copy_for_game(identity(), Game::Competizione, &mut |page| {
            let mut data = bytes(page);
            if matches!(
                (target, page),
                (Page::Physics, Page::Physics) | (Page::Graphics, Page::Graphics)
            ) {
                target_reads += 1;
                let offset = if matches!(page, Page::Physics) {
                    44
                } else {
                    1400
                };
                data.resize(offset + 4, 0);
                data[offset..offset + 4]
                    .copy_from_slice(&(if target_reads == 1 { 1.0f32 } else { 2.0 }).to_le_bytes());
            }
            Ok(data)
        })
        .unwrap();
        assert_eq!(
            target_reads, 4,
            "Changed expanded fields must retry even with unchanged packet IDs"
        );
        assert_eq!(result.source, identity());
    }
    let mut reads = 0;
    assert!(
        consistent_copy_for_game(identity(), Game::Competizione, &mut |page| {
            let mut data = bytes(page);
            if matches!(page, Page::Physics) {
                reads += 1;
                data.resize(48, 0);
                data[44..48].copy_from_slice(&(reads as f32).to_le_bytes());
            }
            Ok(data)
        })
        .is_err()
    );
    assert_eq!(
        reads, 6,
        "Persistent expanded-field tearing must fail with bounded work"
    );
}

#[test]
fn a_torn_read_retries_and_unstable_or_missing_pages_fail_with_bounded_work() {
    let mut physics_reads = 0;
    let sample = consistent_copy(identity(), &mut |page| {
        let mut data = bytes(page);
        if matches!(page, Page::Physics) {
            physics_reads += 1;
            data[..4].copy_from_slice(&(if physics_reads == 1 { 10u32 } else { 11 }).to_le_bytes());
        }
        Ok(data)
    })
    .unwrap();
    assert_eq!(parse(&sample).unwrap().packet_id, 11);
    assert_eq!(physics_reads, 4);
    physics_reads = 0;
    assert!(consistent_copy(identity(), &mut |page| {
        let mut data = bytes(page);
        if matches!(page, Page::Physics) {
            physics_reads += 1;
            data[..4].copy_from_slice(&(physics_reads as u32).to_le_bytes());
        }
        Ok(data)
    })
    .is_err());
    assert_eq!(physics_reads, 6);
    assert!(consistent_copy(identity(), &mut |_| Err("mapping unavailable".into())).is_err());
    assert!(consistent_copy(identity(), &mut |_| Ok(vec![0])).is_err());
}
#[test]
fn current_rpm_limit_is_part_of_the_stable_copy_even_when_the_packet_is_unchanged() {
    let mut physics_reads = 0;
    let sample = consistent_copy(identity(), &mut |page| {
        let mut data = bytes(page);
        if matches!(page, Page::Physics) {
            physics_reads += 1;
            data.resize(PHYSICS_EXTENDED_SIZE, 0);
            let maximum: i32 = if physics_reads == 1 { 8500 } else { 9000 };
            data[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                .copy_from_slice(&maximum.to_le_bytes());
        }
        Ok(data)
    })
    .unwrap();
    assert_eq!(physics_reads, 4);
    assert_eq!(
        parse(&sample).unwrap().channels["max_rpm"].value,
        Some(serde_json::json!(9000))
    );

    physics_reads = 0;
    assert!(consistent_copy(identity(), &mut |page| {
        let mut data = bytes(page);
        if matches!(page, Page::Physics) {
            physics_reads += 1;
            data.resize(PHYSICS_EXTENDED_SIZE, 0);
            let maximum: i32 = 8500 + physics_reads;
            data[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                .copy_from_slice(&maximum.to_le_bytes());
        }
        Ok(data)
    })
    .is_err());
    assert_eq!(physics_reads, 6);
}
#[test]
fn mapping_reset_policy_distinguishes_wrap_duplicates_and_backward_counters() {
    assert!(!counter_reset(None, 0));
    assert!(!counter_reset(Some(10), 10));
    assert!(!counter_reset(Some(10), 11));
    assert!(!counter_reset(Some(u32::MAX), 0));
    assert!(counter_reset(Some(10), 0));
    assert!(counter_reset(Some(10), 9));
}
#[test]
fn selected_pid_requires_an_explicit_matching_game_executable_basename() {
    assert!(selected_image_matches("C:\\Games\\ACR\\ACR.exe", "acr.exe").is_ok());
    assert!(selected_image_matches(
        "C:\\Games\\ACR\\ACR-Win64-Shipping.exe",
        "ACR-Win64-Shipping.exe"
    )
    .is_ok());
    assert!(selected_image_matches("C:\\Games\\ACC\\acc.exe", "acr.exe").is_err());
    for expected in ["", "../acr.exe", "acr", "a.exe\0", &"a".repeat(129)] {
        assert!(selected_image_matches("acr.exe", expected).is_err());
    }
}
#[test]
fn native_producer_identity_cannot_be_relabelled_as_a_different_assetto_game() {
    let producers = [
        (Game::Corsa, "acs.exe"),
        (Game::Corsa, "acs_x86.exe"),
        (Game::Competizione, "AC2-Win64-Shipping.exe"),
        (Game::Evo, "AssettoCorsaEVO.exe"),
        (Game::Rally, "acr.exe"),
    ];
    for (owner, image) in producers {
        assert!(selected_game_image_matches(&format!("C:\\Games\\{image}"), image, owner).is_ok());
        for game in [Game::Corsa, Game::Competizione, Game::Evo, Game::Rally] {
            assert_eq!(
                selected_game_image_matches(image, image, game).is_ok(),
                owner == game
            );
        }
    }
    assert!(
        selected_game_image_matches("AssettoCorsa.exe", "AssettoCorsa.exe", Game::Corsa).is_err()
    );
    assert!(selected_game_image_matches("AC2.exe", "AC2.exe", Game::Competizione).is_err());
}
#[test]
fn evo_stability_check_uses_its_graphics_car_identity_and_narrow_static_metadata() {
    let mut car_reads = 0;
    let result = consistent_copy_for_game(identity(), Game::Evo, &mut |page| {
        let mut data = match page {
            Page::Physics => vec![0; PHYSICS_EXTENDED_SIZE],
            Page::Graphics => vec![0; EVO_GRAPHICS_SIZE],
            Page::Statics => vec![0; Game::Evo.page_lengths().2],
        };
        if matches!(page, Page::Graphics) {
            car_reads += 1;
            data[EVO_CAR_MODEL_OFFSET] = if car_reads == 1 { b'A' } else { b'B' };
        }
        Ok(data)
    })
    .unwrap();
    assert_eq!(car_reads, 4);
    assert_eq!(
        parse_for_game(&result, Game::Evo)
            .unwrap()
            .context
            .car_model,
        "B"
    );
}
#[cfg(not(windows))]
#[test]
fn unsupported_live_platform_is_explicit_and_does_not_manufacture_game_data() {
    assert!(WindowsSource::new(0).is_err());
    assert!(WindowsSource::new(123).is_err());
    let mut source = WindowsSource;
    assert!(source.poll().is_err());
    assert_eq!(source.process_image(), None);
    source.close();
}

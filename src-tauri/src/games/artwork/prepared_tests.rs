use super::*;
use serde_json::json;

fn sample(root: &Path, name: &str, color: u8) -> PathBuf {
    let path = root.join(name);
    image::RgbImage::from_pixel(100, 60, image::Rgb([color, 80, 120])).save(&path).unwrap();
    path
}

fn managed(root: &Path, name: &str, color: u8) -> Background {
    import(root, sample(root, name, color).to_str().unwrap(), 20.0, 70.0).unwrap()
}

fn platform_image(root: &Path, name: &str, color: u8) -> String {
    discovery_preview(&sample(root, name, color)).unwrap()
}

#[test]
fn prepared_snapshot_is_memory_only_and_ignores_managed_global_cache_on_refresh() {
    let root = crate::test_directory();
    let background = managed(root.path(), "manual.png", 10);
    let catalogue = vec![json!({"background":background})];
    let prepared = prepare_local(root.path(), &catalogue).unwrap();
    let clone = prepared.clone();
    assert!(Arc::ptr_eq(&prepared.0, &clone.0));
    let image = prepared.0.managed[&background.image].clone();
    assert_eq!(Arc::strong_count(&image), 2);
    // Import populated the old global path cache. Deleting the underlying file
    // must still affect a new preparation, while current readers retain bytes.
    let path = root.path().join("game-artwork").join(asset_name(&background.image).unwrap());
    fs::remove_file(&path).unwrap();
    for _ in 0..4 {
        let mut output = catalogue.clone();
        clone.decorate(&mut output);
        assert_eq!(output[0]["background"]["image"], image.as_ref());
    }
    let mut refreshed = catalogue.clone();
    prepare_local(root.path(), &catalogue).unwrap().decorate(&mut refreshed);
    assert!(refreshed[0].get("background").is_none());
    fs::write(&path, b"tampered bytes").unwrap();
    let mut tampered = catalogue.clone();
    prepare_local(root.path(), &catalogue).unwrap().decorate(&mut tampered);
    assert!(tampered[0].get("background").is_none());
    assert_eq!(Arc::strong_count(&image), 2);
    drop(prepared);
    drop(clone);
    assert_eq!(Arc::strong_count(&image), 1);
}

#[test]
fn projection_preserves_crops_and_existing_manual_default_platform_priority() {
    let root = crate::test_directory();
    let mut manual = managed(root.path(), "manual.png", 10);
    manual.crop = Some(Crop { x: 0.1, y: 0.2, width: 0.7, height: 0.6 });
    let default = managed(root.path(), "default.png", 220);
    let platform = platform_image(root.path(), "platform.png", 100);
    cache_installation_preview(root.path(), "C:/Games/A", &platform).unwrap();
    let catalogue = vec![
        json!({"installPath":"C:/Games/A","background":manual,"defaultBackground":default}),
        json!({"installPath":"C:/Games/A","defaultBackground":default}),
        json!({"installPath":"C:/Games/B","defaultBackground":default}),
        json!({"installPath":"C:/Games/A","background":{"image":"invalid"}}),
        json!({"background":null,"defaultBackground":{"positionX":12}}),
    ];
    let mut actual = catalogue.clone();
    prepare_local(root.path(), &catalogue).unwrap().decorate(&mut actual);
    let mut expected = catalogue.clone();
    super::super::decorate(root.path(), &mut expected);
    assert_eq!(actual, expected);
    assert_eq!(actual[0]["background"]["crop"], json!(manual.crop));
    assert_eq!(actual[0]["background"]["positionX"], 20.0);
    assert_eq!(actual[0]["background"]["positionY"], 70.0);
    assert_eq!(actual[1]["defaultBackground"]["image"], platform);
    assert_eq!(actual[3]["defaultBackground"]["image"], platform);
}

#[test]
fn snapshots_capture_exact_references_installations_and_application_root() {
    let root = crate::test_directory();
    let other = crate::test_directory();
    let background = managed(root.path(), "manual.png", 10);
    let different = managed(root.path(), "other.png", 200);
    let image = platform_image(root.path(), "platform.png", 100);
    cache_installation_preview(root.path(), "C:/Games/A", &image).unwrap();
    let catalogue = vec![json!({"background":background,"installPath":"C:/Games/A"})];
    let snapshot = prepare_local(root.path(), &catalogue).unwrap();
    let mut unrelated = vec![json!({"background":different,"installPath":"C:/Games/B"})];
    snapshot.decorate(&mut unrelated);
    assert!(unrelated[0].get("background").is_none());
    assert!(unrelated[0].get("defaultBackground").is_none());
    let mut normalized = vec![json!({"installPath":r"c:\games\A\"})];
    snapshot.decorate(&mut normalized);
    assert_eq!(normalized[0]["defaultBackground"]["image"], image);
    let mut separate = catalogue.clone();
    prepare_local(other.path(), &catalogue).unwrap().decorate(&mut separate);
    assert!(separate[0].get("background").is_none());
    assert!(separate[0].get("defaultBackground").is_none());
    let mut invalid_root = catalogue.clone();
    prepare_local(Path::new("relative"), &catalogue).unwrap().decorate(&mut invalid_root);
    assert!(invalid_root[0].get("background").is_none());
}

#[test]
fn installation_snapshot_retains_bytes_and_source_refresh_sees_changes_or_deletion() {
    let root = crate::test_directory();
    let first = platform_image(root.path(), "first.png", 20);
    let second = platform_image(root.path(), "second.png", 220);
    cache_installation_preview(root.path(), "C:/Games/A", &first).unwrap();
    let catalogue = vec![json!({"installPath":"C:/Games/A"})];
    let old = prepare_local(root.path(), &catalogue).unwrap();
    cache_installation_preview(root.path(), "C:/Games/A", &second).unwrap();
    let current = prepare_local(root.path(), &catalogue).unwrap();
    let path = root.path().join("game-platform-artwork")
        .join(installation_filename(&installation_identity("C:/Games/A").unwrap()));
    fs::remove_file(path).unwrap();
    let absent = prepare_local(root.path(), &catalogue).unwrap();
    for (snapshot, image) in [(&old, Some(first)), (&current, Some(second)), (&absent, None)] {
        let mut output = catalogue.clone();
        snapshot.decorate(&mut output);
        assert_eq!(output[0]["defaultBackground"]["image"].as_str(), image.as_deref());
    }
}

#[test]
fn missing_invalid_oversized_nonregular_and_changed_managed_files_are_absent() {
    let root = crate::test_directory();
    let background = managed(root.path(), "manual.png", 100);
    let path = root.path().join("game-artwork").join(asset_name(&background.image).unwrap());
    let catalogue = vec![json!({"background":background})];
    fs::write(&path, vec![0; DISCOVERY_PREVIEW_LIMIT + 1]).unwrap();
    let mut output = catalogue.clone();
    prepare_local(root.path(), &catalogue).unwrap().decorate(&mut output);
    assert!(output[0].get("background").is_none());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let mut output = catalogue.clone();
    prepare_local(root.path(), &catalogue).unwrap().decorate(&mut output);
    assert!(output[0].get("background").is_none());
    let invalid = vec![json!({"background":{"image":"artwork:../invalid"},"installPath":""}),
        json!({"installPath":"bad\npath"}), json!({"installPath":"x".repeat(4097)})];
    let snapshot = prepare_local(root.path(), &invalid).unwrap();
    assert!(snapshot.0.managed.is_empty());
    assert!(snapshot.0.installations.is_empty());
}

#[test]
fn source_paths_are_pure_complete_deduplicated_and_include_missing_file_parents() {
    let root = crate::test_directory();
    let missing = root.path().join("not-created");
    let reference = format!("artwork:{}.jpg", "a".repeat(64));
    let catalogue = vec![json!({"background":{"image":reference},"installPath":"C:/Games/A"}),
        json!({"background":{"image":reference},"installPath":r"c:\games\a\"})];
    let paths = local_source_paths(&missing, &catalogue).unwrap();
    assert!(!missing.exists());
    assert_eq!(paths.len(), 5);
    assert!(paths.contains(&missing));
    assert!(paths.contains(&missing.join("game-artwork")));
    assert!(paths.contains(&missing.join("game-artwork").join(asset_name(&reference).unwrap())));
    assert!(paths.contains(&missing.join("game-platform-artwork")));
    assert!(paths.contains(&missing.join("game-platform-artwork")
        .join(format!("{:x}.jpg", Sha256::digest(br"c:\games\a")))));
    assert_eq!(local_source_paths(&missing, &[]).unwrap(), vec![missing]);
}

#[test]
fn a_full_valid_library_keeps_every_game_and_deduplicates_image_buffers() {
    let root = crate::test_directory();
    let background = managed(root.path(), "manual.png", 100);
    let catalogue = (0..MAX_INSTALLATIONS).map(|id| json!({"id":id,"background":background,
        "defaultBackground":background,"installPath":format!("C:/Games/{id}")})).collect::<Vec<_>>();
    let snapshot = prepare_local(root.path(), &catalogue).unwrap();
    assert_eq!(snapshot.0.managed.len(), 1);
    assert!(snapshot.0.installations.is_empty());
    let mut output = catalogue.clone();
    snapshot.decorate(&mut output);
    assert_eq!(output.len(), MAX_INSTALLATIONS);
    assert!(output.iter().all(|game| game["background"]["image"].as_str()
        .is_some_and(|image| image.starts_with("data:image/jpeg;base64,"))));
    let paths = local_source_paths(root.path(), &catalogue).unwrap();
    assert_eq!(paths.len(), MAX_INSTALLATIONS + 4);
    let distinct = (0..MAX_INSTALLATIONS).map(|id| json!({
        "background":{"image":format!("artwork:{id:064x}.jpg")},
        "defaultBackground":{"image":format!("artwork:{:064x}.jpg", id + MAX_INSTALLATIONS)},
        "installPath":format!("C:/Games/{id}")})).collect::<Vec<_>>();
    assert_eq!(local_source_paths(root.path(), &distinct).unwrap().len(), MAX_INSTALLATIONS * 3 + 3);
    let mut excessive = catalogue;
    excessive.push(json!({"id":"excessive"}));
    assert!(prepare_local(root.path(), &excessive).is_err());
    assert!(local_source_paths(root.path(), &excessive).is_err());
}

#[test]
fn matching_platform_images_share_one_buffer_across_installations() {
    let root = crate::test_directory();
    let image = platform_image(root.path(), "platform.png", 100);
    for path in ["C:/Games/A", "C:/Games/B"] {
        cache_installation_preview(root.path(), path, &image).unwrap();
    }
    let catalogue = vec![json!({"installPath":"C:/Games/A"}), json!({"installPath":"C:/Games/B"})];
    let snapshot = prepare_local(root.path(), &catalogue).unwrap();
    let values = snapshot.0.installations.values().collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert!(Arc::ptr_eq(values[0], values[1]));
}

use super::*;
use serde_json::json;

const BACKGROUND: &str = "abcdef0123456789abcdef0123456789";
const COVER: &str = "123456789abcdef0123456789abcdef0";
const LOGO: &str = "fedcba9876543210fedcba9876543210";

fn cached(root: &Path, hash: &str) -> PathBuf {
    let path = root.join(&hash[..2]).join(&hash[2..4]).join(hash);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
}
fn image(root: &Path, hash: &str) -> PathBuf {
    let path = cached(root, hash);
    image::RgbImage::from_pixel(640, 360, image::Rgb([80, 120, 160]))
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap();
    path
}
fn catalogue(root: &Path, hash: &str, version: u64, product_id: &str, uid: &str) -> PathBuf {
    let path = cached(root, hash);
    fs::write(&path, serde_json::to_vec(&json!({
        "version":version,
        "products":[{"id":product_id,"base":{"types":{"retail":{"uid":uid}},
            "background":"game#BACKGROUND","key_art":"game#KEY_ART","logo":"game#LOGO"}}],
        "files":{"default":{"game#BACKGROUND":{"hash":BACKGROUND},"game#KEY_ART":{"hash":COVER},"game#LOGO":{"hash":LOGO}}}
    })).unwrap()).unwrap();
    path
}
fn codes() -> BTreeSet<String> {
    BTreeSet::from(["osi".into()])
}

#[test]
fn catalogue_prefers_game_art_over_decorative_backgrounds_and_prepares_a_preview() {
    let temp = crate::test_directory();
    let root = temp.path();
    catalogue(root, "00000000000000000000000000000001", 1, "OSI", "osi");
    image(root, BACKGROUND);
    let cover = image(root, COVER);
    let found = lookup(&[root.into()], &codes());
    assert_eq!(found["osi"], cover);
    let preview = crate::game_artwork::discovery_preview(&found["osi"]).unwrap();
    assert!(preview.starts_with("data:image/jpeg;base64,"));
    assert!(lookup(&[root.into()], &BTreeSet::from(["wow".into()])).is_empty());
}

#[test]
fn logos_are_never_substituted_for_game_pictures() {
    let temp = crate::test_directory();
    catalogue(
        temp.path(),
        "00000000000000000000000000000001",
        1,
        "OSI",
        "osi",
    );
    image(temp.path(), BACKGROUND);
    image(temp.path(), COVER);
    image(temp.path(), LOGO);
    assert_eq!(
        lookup(&[temp.path().into()], &codes())["osi"],
        cached(temp.path(), COVER)
    );
    fs::remove_file(cached(temp.path(), COVER)).unwrap();
    assert!(lookup(&[temp.path().into()], &codes()).is_empty());
}

#[test]
fn scanner_assigns_cached_artwork_without_overriding_an_existing_image() {
    let temp = crate::test_directory();
    catalogue(
        temp.path(),
        "00000000000000000000000000000001",
        1,
        "OSI",
        "osi",
    );
    let background = image(temp.path(), COVER);
    let mut candidates = [Candidate {
        id: "battlenet:OSI".into(),
        client: "battlenet".into(),
        name: "Diablo II".into(),
        install_path: temp.path().to_string_lossy().into_owned(),
        executable_candidates: vec![],
        preferred_executable: None,
        game_id: None,
        kind: "game".into(),
        app_id: "OSI".into(),
        launcher: None,
        artwork: None,
    }];
    assign(&mut candidates, &[temp.path().into()]);
    assert_eq!(candidates[0].artwork.as_ref(), Some(&background));
    let previous = temp.path().join("existing.jpg");
    candidates[0].artwork = Some(previous.clone());
    assign(&mut candidates, &[temp.path().into()]);
    assert_eq!(candidates[0].artwork, Some(previous));
    assert!(lookup(&[temp.path().into()], &BTreeSet::new()).is_empty());
}

#[test]
fn unrelated_product_cannot_match_through_an_arbitrary_type_alias() {
    let temp = crate::test_directory();
    catalogue(
        temp.path(),
        "00000000000000000000000000000001",
        1,
        "DIABLO2",
        "OSI",
    );
    image(temp.path(), COVER);
    assert!(lookup(&[temp.path().into()], &codes()).is_empty());
}

#[test]
fn localized_only_catalogues_do_not_override_verified_default_artwork() {
    let temp = crate::test_directory();
    let old = catalogue(
        temp.path(),
        "00000000000000000000000000000001",
        1,
        "OSI",
        "osi",
    );
    let newer = catalogue(
        temp.path(),
        "00000000000000000000000000000002",
        2,
        "OSI",
        "osi",
    );
    image(temp.path(), BACKGROUND);
    let cover = image(temp.path(), COVER);
    let mut data: Value = serde_json::from_slice(&fs::read(&newer).unwrap()).unwrap();
    data["files"] = json!({"deDE":{"game#KEY_ART":{"hash":BACKGROUND}}});
    fs::write(&newer, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(lookup(&[temp.path().into()], &codes())["osi"], cover);
    fs::remove_file(old).unwrap();
    assert!(lookup(&[temp.path().into()], &codes()).is_empty());
    data["files"]["default"] = json!({"game#KEY_ART":{"hash":BACKGROUND}});
    fs::write(newer, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(
        lookup(&[temp.path().into()], &codes())["osi"],
        cached(temp.path(), BACKGROUND)
    );
}

#[test]
fn malformed_fragments_unsafe_hashes_and_non_raster_assets_do_not_provide_artwork() {
    let temp = crate::test_directory();
    let root = temp.path();
    fs::write(cached(root, "00000000000000000000000000000001"), b"{broken").unwrap();
    let path = catalogue(root, "00000000000000000000000000000002", 1, "OSI", "osi");
    let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    data["files"]["default"]["game#BACKGROUND"]["hash"] = json!("../../outside.png");
    fs::write(path, serde_json::to_vec(&data).unwrap()).unwrap();
    fs::write(cached(root, COVER), b"<svg>not a raster image</svg>").unwrap();
    assert!(lookup(&[root.into()], &codes()).is_empty());
    assert!(lookup(&[root.join("missing")], &codes()).is_empty());
    let mut budget = 8;
    let large = cached(root, "00000000000000000000000000000003");
    fs::write(&large, b"{\"large\":123456789}").unwrap();
    assert!(fragment(&large, &mut budget).is_none());
    assert_eq!(budget, 0);
}

#[test]
fn chinese_default_overrides_expansions_and_low_resolution_assets_fail_closed() {
    let temp = crate::test_directory();
    let root = temp.path();
    let path = catalogue(root, "00000000000000000000000000000001", 30, "OSI", "osi");
    let source = image(root, COVER);
    let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    data["products"][0]["base"]["install_background"] = json!("osi_cn#OSI_INSTALL_BACKGROUND_CN");
    data["products"][0]["base"]["key_art"] = json!("osi#OSI_X2_KEY_ART");
    data["files"] = json!({"default":{
        "osi_cn#OSI_INSTALL_BACKGROUND_CN":{"hash":COVER,"name":"resources/osi/install_background_osi_cn_zhCN.png"},
        "osi#OSI_X2_KEY_ART":{"hash":COVER,"name":"resources/osi/phx_game_card_garden_enus.jpg"}
    }});
    fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(lookup(&[root.into()], &codes()).is_empty());
    data["products"][0]["base"]["install_background"] = json!("osi#OSI_INSTALL_BACKGROUND");
    data["files"]["default"]["osi#OSI_INSTALL_BACKGROUND"] =
        json!({"hash":COVER,"name":"resources/osi/install_background_osi_v2_2x_zhCN.jpg"});
    fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(lookup(&[root.into()], &codes()).is_empty());
    data["files"]["default"]["osi#OSI_INSTALL_BACKGROUND"] =
        json!({"hash":COVER,"name":"resources/osi/install_background_osi_v2_2x.jpg"});
    fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(lookup(&[root.into()], &codes())["osi"], source);
    image::RgbImage::new(128, 64)
        .save_with_format(&source, image::ImageFormat::Png)
        .unwrap();
    assert!(lookup(&[root.into()], &codes()).is_empty());
}

#[test]
fn english_resource_priority_precedes_catalogue_version() {
    let temp = crate::test_directory();
    let root = temp.path();
    let path = catalogue(root, "00000000000000000000000000000001", 1, "OSI", "osi");
    let english = image(root, COVER);
    image(root, BACKGROUND);
    let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    data["files"] = json!({"enUS":{"game#KEY_ART":{"hash":COVER}}});
    fs::write(path, serde_json::to_vec(&data).unwrap()).unwrap();
    let newer = catalogue(root, "00000000000000000000000000000002", 99, "OSI", "osi");
    data["version"] = json!(99);
    data["files"] = json!({"default":{"game#KEY_ART":{"hash":BACKGROUND}}});
    fs::write(newer, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(lookup(&[root.into()], &codes())["osi"], english);
}

#[test]
#[ignore = "Read-only installed Battle.net artwork check; requires the launcher's populated cache."]
fn installed_diablo_artwork_decodes_from_the_real_cache() {
    let found = lookup(&cache_roots(), &codes());
    let path = found
        .get("osi")
        .expect("Diablo II artwork exists in the Battle.net cache");
    assert_eq!(
        path.file_name().unwrap(),
        "41a0afd0de68f5518310072cff97053a"
    );
    assert_eq!(
        image::ImageReader::open(path)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap(),
        (544, 1120)
    );
    assert!(crate::game_artwork::discovery_preview(path).is_ok());
    let mut scan = super::super::Scan::default();
    super::super::scan(&mut scan);
    let game = scan
        .candidates
        .iter()
        .find(|game| game.app_id.eq_ignore_ascii_case("osi"))
        .expect("The installed Diablo II game is discovered");
    assert_eq!(game.artwork.as_ref(), Some(path));
}

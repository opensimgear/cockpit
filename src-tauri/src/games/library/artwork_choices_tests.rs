use super::*;
use serde_json::json;
use std::fs;

fn source(client: &str, app_id: &str, install: &Path, launcher: Option<&Path>) -> LibrarySource {
    LibrarySource {
        client: client.into(),
        app_id: app_id.into(),
        install_path: install.to_string_lossy().into_owned(),
        launcher: launcher.map(|path| path.to_string_lossy().into_owned()),
    }
}

fn save_image(path: &Path, width: u32, height: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(width, height, image::Rgb([30, 80, 130]))
        .save_with_format(path, image::ImageFormat::Png)
        .unwrap();
}

#[test]
fn steam_enumerates_only_the_exact_apps_cached_art_and_valid_local_images() {
    let temp = crate::test_directory();
    let root = temp.path().join("Steam");
    let install = temp.path().join("Steam/steamapps/common/Game");
    fs::create_dir_all(&install).unwrap();
    fs::write(root.join("steam.exe"), b"fixture").unwrap();
    let cache = root.join("appcache/librarycache");
    save_image(&cache.join("42/header.jpg"), 460, 215);
    save_image(
        &cache.join("42/abcdef0123456789abcdef0123456789/logo.png"),
        256,
        256,
    );
    save_image(&cache.join("43/header.jpg"), 460, 215);
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("cover.png"), b"not an image").unwrap();
    save_image(&install.join("header.jpg"), 800, 400);
    let choices = artwork_choices_from_roots(
        &[source(
            "steam",
            "42",
            &install,
            Some(&root.join("steam.exe")),
        )],
        &[root],
        &[],
    );
    assert_eq!(choices.len(), 3);
    assert!(choices.iter().any(|choice| choice.path.ends_with("header.jpg") && choice.label.starts_with("Steam:")));
    assert!(
        choices
            .iter()
            .any(|choice| choice.path.ends_with("logo.png"))
    );
    assert!(
        choices
            .iter()
            .any(|choice| choice.path == install.join("header.jpg"))
    );
    assert!(
        !choices
            .iter()
            .any(|choice| choice.path.starts_with(cache.join("43")))
    );
}

#[test]
fn battlenet_enumerates_every_exact_product_art_field_and_rejects_other_products() {
    let temp = crate::test_directory();
    let root = temp.path();
    let install = root.join("game");
    fs::create_dir_all(&install).unwrap();
    let hashes = [
        "abcdef0123456789abcdef0123456789",
        "123456789abcdef0123456789abcdef0",
        "fedcba9876543210fedcba9876543210",
    ];
    for hash in hashes {
        save_image(
            &root.join(&hash[..2]).join(&hash[2..4]).join(hash),
            640,
            360,
        );
    }
    let catalogue = root.join("aa/bb/00000000000000000000000000000001");
    fs::create_dir_all(catalogue.parent().unwrap()).unwrap();
    fs::write(&catalogue, serde_json::to_vec(&json!({
        "products":[
            {"id":"OSI","base":{"install_background":"product#BACKGROUND","key_art":"product#KEY_ART","logo":"product#LOGO"}},
            {"id":"DIABLO2","base":{"key_art":"other#KEY_ART"}}
        ],
        "files":{"enUS":{
            "product#BACKGROUND":{"hash":hashes[0]},
            "product#KEY_ART":{"hash":hashes[1]},
            "product#LOGO":{"hash":hashes[2]},
            "other#KEY_ART":{"hash":hashes[0]}
        }}
    })).unwrap()).unwrap();
    let choices = artwork_choices_from_roots(
        &[source("battlenet", "osi", &install, None)],
        &[],
        &[root.into()],
    );
    assert_eq!(choices.len(), 3);
    assert!(
        choices
            .iter()
            .any(|choice| choice.label.contains("Install Background"))
    );
    assert!(
        choices
            .iter()
            .any(|choice| choice.label.contains("Key Art"))
    );
    assert!(choices.iter().any(|choice| choice.label.contains("Logo")));
    assert!(
        choices
            .iter()
            .all(|choice| !choice.path.ends_with(hashes[0])
                || choice.label.contains("Install Background"))
    );
    assert!(super::super::battlenet::artwork_choices("unknown", &[root.into()]).is_empty());
}

#[test]
fn result_count_is_capped_and_invalid_sources_are_ignored() {
    let temp = crate::test_directory();
    let install = temp.path().join("game");
    fs::create_dir_all(&install).unwrap();
    let mut invalid = source("steam", "42", &install, None);
    invalid.app_id = "../43".into();
    assert!(artwork_choices_from_roots(&[invalid], &[], &[]).is_empty());
    let root = temp.path().join("Steam");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("steam.exe"), b"fixture").unwrap();
    let hash_dir = root.join("appcache/librarycache/42/abcdef0123456789abcdef0123456789");
    for index in 0..80 {
        save_image(&hash_dir.join(format!("asset-{index}_header.png")), 64, 64);
    }
    let choices = artwork_choices_from_roots(
        &[source(
            "steam",
            "42",
            &install,
            Some(&root.join("steam.exe")),
        )],
        &[root],
        &[],
    );
    assert_eq!(choices.len(), MAX_CHOICES);
}

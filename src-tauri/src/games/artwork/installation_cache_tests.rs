use super::*;
use serde_json::json;

fn image() -> String {
    let mut source = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(920, 430, image::Rgb([60, 100, 140]))
        .write_to(&mut source, image::ImageFormat::Png)
        .unwrap();
    discovery_bytes(source.get_ref()).unwrap()
}

#[test]
fn platform_artwork_persists_offline_by_normalized_installation_identity() {
    let root = crate::test_directory();
    let image = image();
    assert!(installation_preview(root.path(), "C:/Games/Diablo").is_none());
    cache_installation_preview(root.path(), "C:/Games/Diablo", &image).unwrap();
    assert_eq!(
        installation_preview(root.path(), r"c:\games\DIABLO\"),
        Some(image.clone())
    );
    assert_eq!(
        filename("C:/Games/Diablo").unwrap(),
        filename(r"\\?\C:\Games\Diablo").unwrap()
    );
    assert!(installation_preview(root.path(), "C:/Games/Other").is_none());
    let path = directory(root.path())
        .unwrap()
        .join(filename("C:/Games/Diablo").unwrap());
    assert_eq!(path.file_name().unwrap().to_str().unwrap().len(), 68);
    let first_modified = fs::metadata(&path).unwrap().modified().unwrap();
    cache_installation_preview(root.path(), "C:/Games/Diablo", &image).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        first_modified
    );
    // A separate application root cannot reuse the in-memory path cache.
    let restarted = crate::test_directory();
    let new_directory = directory(restarted.path()).unwrap();
    fs::create_dir_all(&new_directory).unwrap();
    fs::copy(
        path,
        new_directory.join(filename("C:/Games/Diablo").unwrap()),
    )
    .unwrap();
    assert_eq!(
        installation_preview(restarted.path(), "C:/Games/Diablo"),
        Some(image)
    );
}

#[test]
fn corrupt_oversized_and_nonregular_cache_entries_are_rejected() {
    let root = crate::test_directory();
    let directory = directory(root.path()).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(filename("C:/Games/Diablo").unwrap());
    fs::write(&path, b"not JPEG").unwrap();
    assert!(installation_preview(root.path(), "C:/Games/Diablo").is_none());
    fs::write(&path, vec![0; DISCOVERY_PREVIEW_LIMIT + 1]).unwrap();
    assert!(installation_preview(root.path(), "C:/Games/Diablo").is_none());
    assert!(cache_installation_preview(root.path(), "C:/Games/Diablo", "invalid").is_err());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(cache_installation_preview(root.path(), "C:/Games/Diablo", &image()).is_err());
    assert!(installation_preview(root.path(), "C:/Games/Diablo").is_none());
    assert!(filename("").is_err());
    assert!(filename("C:/bad\npath").is_err());
    assert!(filename(&"x".repeat(4097)).is_err());
    assert!(installation_preview(Path::new("relative"), "C:/Games/Diablo").is_none());
    let invalid = crate::test_directory();
    fs::write(invalid.path().join("game-platform-artwork"), b"file").unwrap();
    assert!(cache_installation_preview(invalid.path(), "C:/Games/Diablo", &image()).is_err());
}

#[test]
fn cache_is_bounded_and_existing_installations_can_refresh_at_capacity() {
    let root = crate::test_directory();
    let directory = directory(root.path()).unwrap();
    fs::create_dir_all(&directory).unwrap();
    for index in 0..MAX_FILES {
        fs::write(directory.join(format!("{index}.jpg")), b"x").unwrap();
    }
    let image = image();
    assert!(cache_installation_preview(root.path(), "C:/Games/Diablo", &image).is_err());
    fs::rename(
        directory.join("0.jpg"),
        directory.join(filename("C:/Games/Diablo").unwrap()),
    )
    .unwrap();
    cache_installation_preview(root.path(), "C:/Games/Diablo", &image).unwrap();
    assert_eq!(
        installation_preview(root.path(), "C:/Games/Diablo"),
        Some(image)
    );
}

#[test]
fn automatic_artwork_refreshes_default_without_changing_manual_backgrounds() {
    let root = crate::test_directory();
    let image = image();
    cache_installation_preview(root.path(), "C:/Games/Diablo", &image).unwrap();
    let mut source = Cursor::new(Vec::new());
    image::RgbImage::new(100, 60)
        .write_to(&mut source, image::ImageFormat::Png)
        .unwrap();
    let manual = store_thumbnail(
        root.path(),
        thumbnail(source.get_ref()).unwrap(),
        50.0,
        50.0,
        None,
    )
    .unwrap();
    let mut games = [
        json!({"installPath":"C:/Games/Diablo"}),
        json!({"installPath":"C:/Games/Diablo","background":manual}),
    ];
    decorate(root.path(), &mut games);
    assert_eq!(games[0]["defaultBackground"]["image"], image);
    assert!(games[1].get("defaultBackground").is_none());
    assert!(games[1]["background"]["image"]
        .as_str()
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));
}

#[cfg(windows)]
#[test]
fn redirected_cache_directory_is_rejected() {
    let root = crate::test_directory();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();
    let link = root.path().join("game-platform-artwork");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(installation_preview(root.path(), "C:/Games/Diablo").is_none());
    assert!(cache_installation_preview(root.path(), "C:/Games/Diablo", &image()).is_err());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    fs::remove_dir(link).unwrap();
}

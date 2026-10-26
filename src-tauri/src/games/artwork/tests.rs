use super::*;
#[test]
fn discovery_images_are_small_transient_jpegs_and_invalid_artwork_is_rejected() {
    let directory=crate::test_directory();
    let source=directory.path().join("landscape.png");
    image::RgbImage::from_fn(640,360,|x,y|image::Rgb([(x/3) as u8,(y/2) as u8,120])).save(&source).unwrap();
    let preview=discovery_preview(&source).unwrap();
    let bytes=preview_bytes(&preview).unwrap();
    assert!(bytes.len()<=THUMBNAIL_LIMIT);
    let image=image::load_from_memory(&bytes).unwrap();
    assert_eq!((image.width(),image.height()),(384,216));
    assert!(!directory.path().join("game-artwork").exists());
    fs::write(&source,b"not an image").unwrap();
    assert!(discovery_preview(&source).is_err());
    assert!(discovery_preview(&directory.path().join("missing.png")).is_err());
}
#[test]
fn transient_previews_and_crops_preserve_bounds_shape_and_exact_managed_bytes() {
    let directory = crate::test_directory();
    let source = sample(directory.path());
    let data = preview(Some(source.to_str().unwrap()), None).unwrap();
    assert!(!directory.path().join("game-artwork").exists());
    let decoded = image::load_from_memory(&preview_bytes(&data).unwrap()).unwrap();
    let crop = Crop {
        x: 0.0,
        y: 0.1,
        width: 1.0,
        height: decoded.width() as f64 / decoded.height() as f64 / (16.0 / 9.0),
    };
    let background = import_preview(directory.path(), &data, crop).unwrap();
    assert_eq!(background.crop, Some(crop));
    assert_eq!(resolved(directory.path(), &background.image).unwrap(), data);
    for invalid in [
        Crop { x: 0.1, ..crop },
        Crop {
            height: 1.0,
            ..crop
        },
        Crop {
            x: f64::NAN,
            ..crop
        },
        Crop { width: 0.0, ..crop },
    ] {
        assert!(import_preview(directory.path(), &data, invalid).is_err());
    }
    assert!(import_preview(directory.path(), "data:image/jpeg;base64,AAAA", crop).is_err());
    assert!(preview(
        Some(source.to_str().unwrap()),
        Some("https://example.com/image.jpg")
    )
    .is_err());
    assert!(preview(None, Some("file:///C:/secret.jpg")).is_err());
    assert!(preview(None, Some("https://user:secret@example.com/image.jpg")).is_err());
}
#[test]
fn image_urls_follow_bounded_http_redirects_and_reject_large_or_non_image_responses() {
    use std::net::TcpListener;
    let directory = crate::test_directory();
    let source = sample(directory.path());
    let bytes = fs::read(source).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut image_response = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).into_bytes();
        image_response.extend(bytes);
        let responses = [b"HTTP/1.1 302 Found\r\nLocation: /image\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),image_response,
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",SOURCE_LIMIT+1).into_bytes(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntext".to_vec(),
            b"HTTP/1.1 302 Found\r\nLocation: file:///C:/secret.jpg\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()];
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            stream.write_all(&response).unwrap();
        }
    });
    assert!(preview(None, Some(&format!("http://{address}/redirect")))
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));
    assert!(preview(None, Some(&format!("http://{address}/large")))
        .unwrap_err()
        .contains("16 MiB"));
    assert!(preview(None, Some(&format!("http://{address}/text"))).is_err());
    assert!(preview(None, Some(&format!("http://{address}/file-redirect"))).is_err());
    server.join().unwrap();
}
fn sample(root: &Path) -> PathBuf {
    let source = root.join("source.png");
    image::RgbImage::from_pixel(110, 80, image::Rgb([35, 70, 120]))
        .save(&source)
        .unwrap();
    source
}
#[test]
fn imported_image_is_managed_deduplicated_and_survives_source_removal() {
    let temporary = crate::test_directory();
    let root = temporary.path();
    let source = sample(root);
    let background = import(root, source.to_str().unwrap(), 20.0, 80.0).unwrap();
    assert_eq!(
        background,
        import(root, source.to_str().unwrap(), 20.0, 80.0).unwrap()
    );
    fs::remove_file(&source).unwrap();
    let mut catalogue = vec![serde_json::json!({"background":background})];
    decorate(root, &mut catalogue);
    assert!(catalogue[0]["background"]["image"]
        .as_str()
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));
    assert_eq!(catalogue[0]["background"]["positionX"], 20.0);
    assert_eq!(fs::read_dir(root.join("game-artwork")).unwrap().count(), 1);
}
#[test]
fn invalid_images_positions_and_managed_paths_are_rejected() {
    let temporary = crate::test_directory();
    let root = temporary.path();
    let source = sample(root);
    assert!(import(root, source.to_str().unwrap(), f64::NAN, 50.0).is_err());
    assert!(import(root, source.to_str().unwrap(), 50.0, 101.0).is_err());
    fs::write(&source, b"not an image").unwrap();
    assert!(import(root, source.to_str().unwrap(), 50.0, 50.0).is_err());
    assert!(asset_name("artwork:../../file.jpg").is_err());
    let mut catalogue = vec![serde_json::json!({"background":{"image":"artwork:../../file.jpg"}})];
    decorate(root, &mut catalogue);
    assert!(catalogue[0].get("background").is_none());
}
#[test]
fn dimensions_and_encoded_thumbnail_are_bounded() {
    let mut source = Vec::new();
    image::RgbImage::from_fn(1200, 900, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 251) as u8, ((x * y) % 251) as u8])
    })
    .write_to(&mut Cursor::new(&mut source), image::ImageFormat::Png)
    .unwrap();
    let encoded = thumbnail(&source).unwrap();
    assert!(encoded.len() <= THUMBNAIL_LIMIT);
    assert!(image::load_from_memory(&encoded).unwrap().width() <= 640);
}
#[test]
fn collection_preserves_shared_assets_and_unrelated_files() {
    let temporary = crate::test_directory();
    let root = temporary.path();
    let source = sample(root);
    let background = import(root, source.to_str().unwrap(), 50.0, 50.0).unwrap();
    fs::write(root.join("game-artwork/notes.txt"), b"unrelated").unwrap();
    retain(root, [&background, &background].into_iter());
    assert!(resolved(root, &background.image).is_ok());
    retain(root, std::iter::empty());
    assert!(!root
        .join("game-artwork")
        .join(asset_name(&background.image).unwrap())
        .exists());
    assert!(root.join("game-artwork/notes.txt").exists());
}
#[test]
fn collection_waits_for_uncommitted_image_proposals() {
    let temporary = crate::test_directory();
    let root = temporary.path();
    let first = staging(root).unwrap();
    let second = staging(root).unwrap();
    let source = sample(root);
    let background = import(root, source.to_str().unwrap(), 50.0, 50.0).unwrap();
    retain(root, std::iter::empty());
    assert!(resolved(root, &background.image).is_ok());
    drop(first);
    retain(root, std::iter::empty());
    assert!(resolved(root, &background.image).is_ok());
    drop(second);
    retain(root, std::iter::empty());
    assert!(resolved(root, &background.image).is_err());
}
#[test]
fn collection_uses_current_sqlite_references_and_honours_another_process_lease() {
    let directory = crate::test_directory();
    let root = directory.path();
    let background = import(root, sample(root).to_str().unwrap(), 50.0, 50.0).unwrap();
    let store = crate::game_installations::Store::new(root.join("game-library.json"));
    let mut settings = crate::game_installations::Settings::default();
    settings.games.insert("dcs".into(),crate::game_installations::GameRecord{name:"DCS".into(),installations:Default::default(),selected_installation:None,integration:None,default_background:None});
    settings
        .backgrounds
        .insert("dcs".into(), background.clone());
    store.save(&settings).unwrap();
    retain(root, std::iter::empty());
    assert!(root
        .join("game-artwork")
        .join(asset_name(&background.image).unwrap())
        .exists());
    settings.backgrounds.clear();
    store.save(&settings).unwrap();
    let foreign = asset_lock(root).unwrap();
    foreign.lock_shared().unwrap();
    retain(root, std::iter::empty());
    assert!(root
        .join("game-artwork")
        .join(asset_name(&background.image).unwrap())
        .exists());
    drop(foreign);
    retain(root, std::iter::empty());
    assert!(!root
        .join("game-artwork")
        .join(asset_name(&background.image).unwrap())
        .exists());
}

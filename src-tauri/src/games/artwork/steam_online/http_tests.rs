use super::*;
use serde_json::json;
fn image() -> String {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbImage::from_fn(640, 360, |x, y| {
        image::Rgb([(x / 3) as u8, (y / 2) as u8, 120])
    })
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    encode(bytes.get_ref()).unwrap()
}
#[test]
fn metadata_resolves_hashed_official_artwork_and_rejects_wrong_sources() {
    for host in [
        "shared.fastly.steamstatic.com",
        "shared.akamai.steamstatic.com",
        "cdn.akamai.steamstatic.com",
        "cdn.cloudflare.steamstatic.com",
        "cdn.fastly.steamstatic.com",
        "steamcdn-a.akamaihd.net",
    ] {
        let url=format!("https://{host}/store_item_assets/steam/apps/42/content-hash/header_alt_assets.jpg?t=123");
        let metadata = json!({"42":{"success":true,"data":{"header_image":url}}});
        assert_eq!(
            image_url(42, &serde_json::to_vec(&metadata).unwrap()).unwrap(),
            url
        );
    }
    for url in [
        "http://shared.fastly.steamstatic.com/steam/apps/42/header.jpg",
        "https://evil.example/steam/apps/42/header.jpg",
        "https://shared.fastly.steamstatic.com.evil.example/steam/apps/42/header.jpg",
        "https://user:secret@shared.fastly.steamstatic.com/steam/apps/42/header.jpg",
        "https://shared.fastly.steamstatic.com:8443/steam/apps/42/header.jpg",
        "https://shared.fastly.steamstatic.com/steam/apps/43/header.jpg",
        "https://shared.fastly.steamstatic.com/steam/apps/42/../../43/header.jpg",
    ] {
        assert!(
            image_url(
                42,
                &serde_json::to_vec(&json!({"42":{"success":true,"data":{"header_image":url}}}))
                    .unwrap()
            )
            .is_err(),
            "{url}"
        );
    }
    for metadata in [b"invalid".as_slice(), b"{}", br#"{"42":{"success":false}}"#] {
        assert!(image_url(42, metadata).is_err());
    }
}

#[test]
fn downloaded_images_are_bounded_and_prepared_at_display_resolution() {
    assert_eq!(read_bounded(Cursor::new(b"abc"), 3).unwrap(), b"abc");
    assert!(read_bounded(Cursor::new(b"abcd"), 3).is_err());
    assert!(encode(b"not an image").is_err());
    let image = image();
    let bytes = discovery_preview_bytes(&image).unwrap();
    assert!(bytes.len() <= DISCOVERY_PREVIEW_LIMIT);
    let decoded = decoded(&bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (640, 360));
}

#[test]
fn high_resolution_header_is_probed_only_beside_verified_header_and_failure_keeps_original() {
    let url = "https://shared.fastly.steamstatic.com/steam/apps/42/hash/header.jpg?t=123";
    let metadata =
        serde_json::to_vec(&json!({"42":{"success":true,"data":{"header_image":url}}})).unwrap();
    let make = |width, height| {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(width, height, image::Rgb([40, 80, 120]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    let small = make(460, 215);
    let large = make(920, 430);
    for available in [true, false] {
        let mut calls = Vec::new();
        let preview = download_with(42, |requested, _| {
            calls.push(requested.to_owned());
            match calls.len() {
                1 => Ok(metadata.clone()),
                2 => Ok(small.clone()),
                _ if available => Ok(large.clone()),
                _ => Err("missing 2x header".into()),
            }
        })
        .unwrap();
        assert_eq!(calls[1], url);
        assert_eq!(calls[2], url.replace("header.jpg", "header_2x.jpg"));
        let bytes = discovery_preview_bytes(&preview).unwrap();
        let decoded = decoded(&bytes).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            if available { (920, 430) } else { (460, 215) }
        );
    }
    assert!(larger_header_url(
        "https://shared.fastly.steamstatic.com/steam/apps/42/hash/header_alt_assets.jpg"
    )
    .is_none());
}

#[test]
fn metadata_and_download_follow_the_confirmed_store_url_and_preserve_failures() {
    let url = "https://shared.fastly.steamstatic.com/steam/apps/42/hash/header.jpg";
    let metadata =
        serde_json::to_vec(&json!({"42":{"success":true,"data":{"header_image":url}}})).unwrap();
    let png = super::super::discovery_preview_bytes(&image()).unwrap();
    let mut calls = Vec::new();
    let result = download_with(42, |requested, limit| {
        calls.push((requested.to_owned(), limit));
        Ok(if calls.len() == 1 {
            metadata.clone()
        } else {
            png.clone()
        })
    })
    .unwrap();
    assert!(result.starts_with("data:image/jpeg;base64,"));
    assert!(calls[0].0.contains("appids=42"));
    assert_eq!(calls[1].0, url);
    assert_eq!(calls[0].1, METADATA_LIMIT);
    assert_eq!(calls[1].1, DOWNLOAD_LIMIT);
    assert!(download_with(42, |_, _| Err("offline".into()))
        .unwrap_err()
        .contains("offline"));
    assert!(download_with(42, |_, _| Ok(b"invalid JSON".to_vec())).is_err());
    let mut first = true;
    assert!(download_with(42, |_, _| if std::mem::take(&mut first) {
        Ok(metadata.clone())
    } else {
        Err("image unavailable".into())
    })
    .unwrap_err()
    .contains("unavailable"));
}

use super::*;
use serde_json::json;

#[test]
fn asset_projection_shares_image_handles_and_preserves_custom_precedence() {
    let custom: Arc<str> = "custom-image".into();
    let installed: Arc<str> = "installation-image".into();
    let path = "C:\\game";
    let prepared = PreparedLocalArtwork(Arc::new(Images {
        managed: BTreeMap::from([("custom".into(), custom.clone())]),
        installations: BTreeMap::from([(installation_identity(path).unwrap(), installed)]),
    }));
    let mut catalogue = vec![json!({"background":{"image":"custom"},"installPath":path})];
    let mut calls = 0;
    prepared.decorate_with(&mut catalogue, &mut |image| {
        assert!(Arc::ptr_eq(image, &custom)); calls += 1;
        Ok(json!({"$gamePage":{"kind":"asset","id":"custom-token"}}))
    }).unwrap();
    assert_eq!(calls, 1);
    assert!(catalogue[0]["defaultBackground"].is_null());
    assert!(prepared.same_images(&prepared.clone()));
    let error = prepared.decorate_with(&mut [json!({"background":{"image":"custom"}})], &mut |_| Err("budget".into()));
    assert_eq!(error, Err("budget".into()));
}

use super::*;
use serde_json::json;

#[test]
fn tokens_preserve_local_priority_and_online_resolution_shares_cached_bytes() {
    let root = Path::new("unused");
    let image: Arc<str> = "x".repeat(128 * 1024).into();
    let pool = Mutex::new(Service::default());
    pool.lock().unwrap().cache_ready(Key { root: root.into(), app_id: 42 }, image.clone());
    let mut state = json!({"catalogue":[{"id":"game","installations":[{"sources":[{"client":"steam","appId":"42"}]}]}],
        "discovery":{"status":"ready","entries":[]}});
    let token = json!({"$gamePage":{"kind":"asset","id":"token"}});
    decorate_assets_with(&pool, root, &mut state, &mut |shared| {
        assert!(Arc::ptr_eq(shared, &image)); Ok(token.clone())
    }).unwrap();
    assert_eq!(state["catalogue"][0]["defaultBackground"]["image"], token);
    decorate_assets_with(&pool, root, &mut state, &mut |_| panic!("Resolved local tokens retain precedence")).unwrap();
    assert_eq!(state["artworkPending"], false);
}

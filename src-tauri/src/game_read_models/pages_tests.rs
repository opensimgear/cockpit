use super::*;

fn id(node: &Value) -> &str { node["$gamePage"]["id"].as_str().unwrap() }

fn small_pages(bytes: usize, blobs: usize, scopes: usize) -> Pages {
    Pages::with_limits(Limits { bytes, blobs, blob_bytes: bytes, scopes })
}

#[test]
fn pinned_baselines_survive_capacity_failure_and_retirement_allows_eviction() {
    let pages = small_pages(6, 2, 2);
    let mut projection = pages.begin();
    let first = projection.asset(Arc::from("abc")).unwrap();
    projection.commit("state").unwrap();
    let mut projection = pages.begin();
    let second = projection.asset(Arc::from("def")).unwrap();
    projection.commit("detail").unwrap();
    {
        let mut projection = pages.begin();
        assert!(projection.asset(Arc::from("ghi")).is_err());
        assert_eq!(pages.get(id(&first), 0).unwrap()["chunk"], "abc");
        assert_eq!(pages.get(id(&second), 0).unwrap()["chunk"], "def");
    }
    pages.release("detail");
    let mut projection = pages.begin();
    let third = projection.asset(Arc::from("ghi")).unwrap();
    projection.commit("detail").unwrap();
    assert!(pages.get(id(&second), 0).is_err());
    assert_eq!(pages.get(id(&third), 0).unwrap()["chunk"], "ghi");
    assert!(pages.get(id(&first), 0).is_ok());
}

#[test]
fn replacement_unpins_previous_ids_and_drop_discards_provisional_blobs() {
    let pages = small_pages(6, 2, 1);
    let mut projection = pages.begin();
    let old = projection.asset(Arc::from("old")).unwrap();
    projection.commit("state").unwrap();
    let mut projection = pages.begin();
    let replacement = projection.asset(Arc::from("new")).unwrap();
    projection.commit("state").unwrap();
    {
        let mut projection = pages.begin();
        let temporary = projection.asset(Arc::from("tmp")).unwrap();
        assert!(pages.get(id(&temporary), 0).is_ok());
        assert!(pages.get(id(&old), 0).is_err());
    }
    assert_eq!(pages.state().blobs.len(), 1);
    assert!(pages.get(id(&replacement), 0).is_ok());
    pages.clear();
    assert!(pages.get(id(&replacement), 0).is_err());
    assert!(pages.state().scopes.is_empty());
}

#[test]
fn scope_limit_failure_keeps_baseline_and_releases_provisionals() {
    let pages = small_pages(100, 4, 1);
    let mut projection = pages.begin();
    let baseline = projection.asset(Arc::from("baseline")).unwrap();
    projection.commit("state").unwrap();
    let mut projection = pages.begin();
    let rejected = projection.asset(Arc::from("rejected")).unwrap();
    assert!(projection.commit("extra").is_err());
    assert!(pages.get(id(&baseline), 0).is_ok());
    assert!(pages.get(id(&rejected), 0).is_err());
    pages.release("missing");
    pages.release("state");
    assert_eq!(pages.state().blobs[id(&baseline)].pins, 0);
}

#[test]
fn asset_hash_dedup_keeps_the_original_arc_and_counts_bytes_once() {
    let pages = Pages::default();
    let text: Arc<str> = Arc::from("abc");
    let mut projection = pages.begin();
    let first = projection.asset(Arc::clone(&text)).unwrap();
    let second = projection.asset(Arc::from("abc")).unwrap();
    assert_eq!(first, second);
    assert_eq!(id(&first), "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    {
        let state = pages.state();
        assert_eq!(state.bytes, 3);
        assert_eq!(state.blobs.len(), 1);
        assert!(Arc::ptr_eq(&state.blobs[id(&first)].text, &text));
    }
    projection.commit("state").unwrap();
}

#[test]
fn chunk_reads_are_utf8_safe_bounded_and_independent_of_writer() {
    let pages = Pages::default();
    let text = "🛩".repeat(6000);
    let mut projection = pages.begin();
    let node = projection.asset(Arc::from(text.as_str())).unwrap();
    let mut offset = 0;
    let mut restored = String::new();
    loop {
        let result = pages.get(id(&node), offset).unwrap();
        let chunk = result["chunk"].as_str().unwrap();
        assert!(chunk.len() <= CHUNK_BYTES);
        assert_eq!(result["offset"], offset);
        assert_eq!(result["total"], text.len());
        restored.push_str(chunk);
        match result["nextOffset"].as_u64() {
            Some(next) => { assert!(next > offset); offset = next; },
            None => break,
        }
    }
    assert_eq!(restored, text);
    assert!(pages.get(id(&node), 1).is_err());
    assert!(pages.get(id(&node), u64::MAX).is_err());
    assert_eq!(pages.get(id(&node), text.len() as u64).unwrap()["chunk"], "");
    projection.commit("state").unwrap();
}

#[test]
fn forged_identifiers_and_oversized_blobs_are_rejected() {
    let pages = small_pages(8, 2, 1);
    assert!(pages.get("../secret", 0).is_err());
    assert!(pages.get(&format!("sha256:{}", "a".repeat(64)), 0).is_err());
    assert!(pages.get(&format!("sha256:{}", "A".repeat(64)), 0).is_err());
    let mut projection = pages.begin();
    assert!(projection.asset(Arc::from("123456789")).is_err());
    assert!(pages.state().blobs.is_empty());
}

#[test]
fn zero_cache_capacity_is_an_explicit_error() {
    let pages = small_pages(100, 0, 1);
    assert!(pages.begin().asset(Arc::from("")).is_err());
}

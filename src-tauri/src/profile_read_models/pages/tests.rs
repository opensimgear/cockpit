use super::*;

#[test]
fn chunks_are_exact_utf8_bounded_and_unknown_or_expired_ids_are_explicit() {
    let pages = Pages::default();
    let original = format!("[\"{}\"]", "é".repeat(6000));
    let manifest = pages.install(original.clone(), 1).unwrap();
    let id = manifest["pages"]["id"].as_str().unwrap();
    pages.pin(Some(id));
    let mut offset = 0;
    let mut reconstructed = String::new();
    loop {
        let page = pages.get(id, offset).unwrap();
        let chunk = page["chunk"].as_str().unwrap();
        assert!(chunk.len() <= CHUNK_BYTES);
        reconstructed.push_str(chunk);
        offset = page["nextOffset"].as_u64().unwrap();
        if offset == page["total"].as_u64().unwrap() { break; }
    }
    assert_eq!(original, reconstructed);
    assert_eq!(id, format!("sha256:{:x}", Sha256::digest(reconstructed.as_bytes())));
    assert!(pages.get(id, 3).unwrap_err().contains("UTF-8"));
    assert!(pages.get("../../anything", 0).is_err());
    assert!(pages.get(&format!("sha256:{}", "0".repeat(64)), 0).unwrap_err().contains("expired"));
    for index in 0..6 { pages.install(format!("[\"version-{index}\"]"), 1).unwrap(); }
    assert_eq!(pages.get(id, 0).unwrap()["chunk"], &original[..8192]);
    assert!(pages.state.lock().unwrap().blobs.len() <= MAX_BLOBS);
    pages.pin(None);
    for index in 6..12 { pages.install(format!("[\"version-{index}\"]"), 1).unwrap(); }
    assert!(pages.get(id, 0).unwrap_err().contains("expired"));
}

#[test]
fn cache_bytes_and_single_blob_are_bounded_and_current_blob_is_retained() {
    let pages = Pages::default();
    assert!(pages.install("x".repeat(MAX_BLOB_BYTES + 1), 0).is_err());
    let first = pages.install("a".repeat(MAX_BLOB_BYTES), 0).unwrap();
    let id = first["pages"]["id"].as_str().unwrap();
    pages.pin(Some(id));
    for value in ['b', 'c', 'd', 'e'] {
        pages.install(value.to_string().repeat(MAX_BLOB_BYTES), 0).unwrap();
        let state = pages.state.lock().unwrap();
        assert!(state.bytes <= MAX_BYTES);
        assert!(state.blobs.len() <= MAX_BLOBS);
        assert!(state.blobs.contains_key(id));
    }
    pages.clear();
    assert!(pages.get(id, 0).is_err());
}

#[test]
fn prepared_equal_rules_reuse_the_blob_and_buffer() {
    let pages = Pages::default();
    let mut rules = vec![Rule { identity: osg_profiles::activation::GameIdentity {
        game_id: "dcs".into(), executable: format!("C:\\{}", "x".repeat(32760)) },
        profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), priority: 0, enabled: true }];
    let mut prepared = PreparedRules::default();
    prepared.prepare(&rules, &pages).unwrap();
    let wire = prepared.wire().clone();
    let buffer = prepared.previous.as_ref().unwrap().as_ptr();
    prepared.pin(&pages);
    for _ in 0..10 { prepared.prepare(&rules, &pages).unwrap(); }
    assert_eq!(prepared.wire(), &wire);
    assert_eq!(prepared.previous.as_ref().unwrap().as_ptr(), buffer);
    assert_eq!(pages.state.lock().unwrap().blobs.len(), 1);
    let old = wire["pages"]["id"].as_str().unwrap();
    let original = pages.get(old, 0).unwrap();
    rules[0].priority = 4;
    prepared.prepare(&rules, &pages).unwrap();
    assert_ne!(prepared.wire(), &wire);
    assert_eq!(pages.get(old, 0).unwrap(), original);
}

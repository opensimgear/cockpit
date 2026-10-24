use super::*;
use serde_json::json;
fn content()->Value{
    let upload:Value=serde_json::from_str(include_str!("../../tests/fixtures/c1-flight-publication.json")).unwrap();
    let document:Value=serde_json::from_str(upload["canonicalDocument"].as_str().unwrap()).unwrap();
    json!({"entries":[{"hostId":"host","instanceId":"panel","name":"Offline panel","key":"[\"host\",\"panel\"]","publication":{"document":document,"assets":upload["assets"],"publicationHash":upload["publicationHash"],"capabilityFingerprint":upload["capabilityFingerprint"]},"savedAtMs":123,"pageId":"flight-page-0"}],"selectedKey":"[\"host\",\"panel\"]"})
}
fn empty()->String{json!({"entries":[],"selectedKey":""}).to_string()}
#[test]
fn native_cache_reopens_exact_public_content_and_preserves_selected_panel_and_page(){
    let dir=tempfile::tempdir().unwrap();let cache=ClientCache::open(dir.path()).unwrap();let initial=cache.execute(CacheRequest::Read{}).unwrap();assert_eq!(initial.revision,0);assert!(initial.payload.is_none());
    let payload=content().to_string();let written=cache.execute(CacheRequest::Write{expected_revision:0,payload:payload.clone()}).unwrap();assert_eq!(written.revision,1);
    let reopened=ClientCache::open(dir.path()).unwrap().read().unwrap();assert_eq!(reopened.payload.as_deref(),Some(payload.as_str()));assert!(reopened.error.is_none());
    assert!(!dir.path().join("companion-publications").exists());assert!(dir.path().join("cockpit.sqlite3").exists());
}
#[test]
fn stale_writers_and_failed_commits_cannot_resurrect_a_cleared_library(){
    let dir=tempfile::tempdir().unwrap();let cache=ClientCache::open(dir.path()).unwrap();cache.write(0,&content().to_string()).unwrap();let stale=ClientCache::open(dir.path()).unwrap();assert_eq!(stale.read().unwrap().revision,1);
    let lock=osg_persistence::test_support::hold_writer(&cache.database).unwrap();assert!(cache.write(1,&empty()).is_err());drop(lock);assert_eq!(cache.read().unwrap().revision,1);
    cache.write(1,&empty()).unwrap();assert!(stale.write(1,&content().to_string()).is_err());assert_eq!(cache.read().unwrap().payload.unwrap(),empty());
}
#[test]
fn invalid_records_stay_visible_and_an_explicit_clear_uses_the_observed_revision(){
    let dir=tempfile::tempdir().unwrap();let cache=ClientCache::open(dir.path()).unwrap();
    cache.database.commit(&[Change{domain:"companion-client",key:"publications",expected_revision:0,payload:Some(b"corrupt cache")}]).unwrap();
    let snapshot=cache.read().unwrap();assert_eq!(snapshot.revision,1);assert!(snapshot.payload.is_none());assert!(snapshot.error.is_some());
    assert_eq!(cache.database.read("companion-client","publications").unwrap().unwrap().payload,b"corrupt cache");cache.write(1,&empty()).unwrap();assert!(cache.read().unwrap().error.is_none());
}
#[test]
fn native_validation_rejects_corruption_unknown_metadata_and_unbounded_content(){
    let source=content();validate(source.to_string().as_bytes()).unwrap();
    let mutations:Vec<Box<dyn Fn(&mut Value)>>=vec![
        Box::new(|v|v["version"]=json!(1)),
        Box::new(|v|v["selectedKey"]=json!("missing")),
        Box::new(|v|v["entries"][0]["hostId"]=json!("")),
        Box::new(|v|v["entries"][0]["name"]=json!("bad\nname")),
        Box::new(|v|v["entries"][0]["key"]=json!("incorrect")),
        Box::new(|v|v["entries"][0]["savedAtMs"]=json!(9_007_199_254_740_992u64)),
        Box::new(|v|v["entries"][0]["pageId"]=json!("missing")),
        Box::new(|v|v["entries"][0]["extra"]=json!(true)),
        Box::new(|v|v["entries"][0]["publication"]["publicationHash"]=json!("sha256:wrong")),
        Box::new(|v|v["entries"][0]["publication"]["capabilityFingerprint"]=json!("wrong")),
        Box::new(|v|v["entries"][0]["publication"]["token"]=json!("forbidden extra field")),
        Box::new(|v|v["entries"][0]["publication"]["document"]["ui"]["title"]=json!("tampered")),
        Box::new(|v|{let entry=v["entries"][0].clone();v["entries"].as_array_mut().unwrap().push(entry);}),
        Box::new(|v|{let entry=v["entries"][0].clone();v["entries"]=json!(vec![entry;5]);}),
    ];
    for mutate in mutations {let mut value=source.clone();mutate(&mut value);assert!(validate(value.to_string().as_bytes()).is_err());}
    assert!(validate(&vec![b' ';CACHE_BYTES+1]).is_err());
    let mut oversized=source.clone();oversized["entries"][0]["publication"]["document"]["padding"]=json!("x".repeat(ENTRY_BYTES));
    assert!(validate(oversized.to_string().as_bytes()).unwrap_err().contains("13 MiB"));
    assert!(serde_json::from_value::<CacheRequest>(json!({"op":"write","payload":empty()})).is_err());
    assert!(serde_json::from_value::<CacheRequest>(json!({"op":"read","path":"elsewhere"})).is_err());
}

#[cfg(windows)]
#[test]
fn clearing_public_cache_never_forgets_or_exposes_the_same_namespaces_sealed_credential(){
    use crate::persistence::{SealedSqliteStore,StateStorage};
    let dir=tempfile::tempdir().unwrap();let secret=SealedSqliteStore::new(dir.path().join("companion-client.dpapi"));
    let private=b"native installation token remains sealed";secret.write(private).unwrap();
    let cache=ClientCache::open(dir.path()).unwrap();cache.write(0,&content().to_string()).unwrap();cache.write(1,&empty()).unwrap();
    assert_eq!(secret.read().unwrap().unwrap(),private);let bytes=std::fs::read(cache.database.path()).unwrap();assert!(!bytes.windows(private.len()).any(|chunk|chunk==private));
    assert!(cache.database.read("secrets","companion-client").unwrap().is_some());
}

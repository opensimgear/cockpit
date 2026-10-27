use super::*;

fn query()->MetadataQuery {MetadataQuery{key:"local".into(),title:"Example Game".into(),external_ids:vec![]}}
fn matched(at:u64)->MetadataLookup {
    MetadataLookup{status:LookupStatus::Matched,metadata:Some(GameMetadata{
        source:"wikidata".into(),entity_id:"Q42".into(),url:"https://www.wikidata.org/wiki/Q42".into(),
        title:"Example Game".into(),description:Some("A video game".into()),aliases:vec![],genres:vec![],genre_ancestors:vec![],
        platforms:vec![],developers:vec![],publishers:vec![],release_dates:vec![],external_ids:vec![],fetched_at:at,matched_by:MatchMethod::ExactTitle,
    }),checked_at:at,stale:false}
}

#[test]
fn fingerprint_is_independent_of_local_key_and_external_id_order() {
    let mut first=query();
    first.external_ids=vec![ExternalId{platform:"steam".into(),id:"42".into()},ExternalId{platform:"gog".into(),id:"4".into()}];
    let mut second=first.clone();second.key="another".into();second.title="EXAMPLE™ game".into();second.external_ids.reverse();
    assert_eq!(fingerprint(&first),fingerprint(&second));
    second.external_ids[0].id="different".into();assert_ne!(fingerprint(&first),fingerprint(&second));
}

#[test]
fn positive_ttl_allows_explicit_stale_fallback_but_rejects_future_records() {
    let directory=tempfile::tempdir().unwrap();let query=query();let value=matched(100);
    assert!(write(directory.path(),&query,&value));
    assert_eq!(read(directory.path(),&query,100+POSITIVE_TTL-1,false),Some(value));
    assert!(read(directory.path(),&query,100+POSITIVE_TTL,false).is_none());
    assert!(read(directory.path(),&query,100+POSITIVE_TTL,true).unwrap().stale);
    assert!(read(directory.path(),&query,99,true).is_none());
}

#[test]
fn negative_and_ambiguous_results_expire_without_stale_reuse() {
    let directory=tempfile::tempdir().unwrap();let query=query();
    for status in [LookupStatus::NotFound,LookupStatus::Ambiguous] {
        let lookup=MetadataLookup::empty(status,100);
        assert!(write(directory.path(),&query,&lookup));
        assert_eq!(read(directory.path(),&query,100+NEGATIVE_TTL-1,false),Some(lookup));
        assert!(read(directory.path(),&query,100+NEGATIVE_TTL,true).is_none());
    }
    assert!(!write(directory.path(),&query,&MetadataLookup::empty(LookupStatus::Unavailable,100)));
}

#[test]
fn cache_rejects_corruption_wrong_fingerprint_invalid_model_and_oversize() {
    let directory=tempfile::tempdir().unwrap();let query=query();
    assert!(write(directory.path(),&query,&matched(100)));
    let path=directory.path().join("game-metadata").join(format!("{}.json",fingerprint(&query)));
    fs::write(&path,b"broken").unwrap();assert!(read(directory.path(),&query,100,true).is_none());
    fs::write(&path,serde_json::to_vec(&Cached{fingerprint:"wrong".into(),lookup:matched(100)}).unwrap()).unwrap();
    assert!(read(directory.path(),&query,100,true).is_none());
    let mut invalid=matched(100);invalid.metadata.as_mut().unwrap().entity_id="Q0".into();assert!(!write(directory.path(),&query,&invalid));
    fs::write(&path,vec![b' ';MAX_BYTES as usize+1]).unwrap();assert!(read(directory.path(),&query,100,true).is_none());
}

#[test]
fn cache_capacity_and_unavailable_root_are_bounded() {
    let directory=tempfile::tempdir().unwrap();let query=query();
    assert!(!write(Path::new("relative"),&query,&matched(100)));
    assert!(read(Path::new("relative"),&query,100,false).is_none());
    let cache=super::directory(directory.path(),true).unwrap();
    for index in 0..MAX_FILES {fs::write(cache.join(format!("{index}.json")),b"{}").unwrap();}
    assert!(!write(directory.path(),&query,&matched(100)));
}

#[test]
fn cooldown_is_persisted_without_truncating_long_retry_after() {
    let directory=tempfile::tempdir().unwrap();save_cooldown(directory.path(),130);
    assert_eq!(cooldown(directory.path(),100),130);assert_eq!(cooldown(directory.path(),131),0);
    save_cooldown(directory.path(),100+3*24*60*60);
    assert_eq!(cooldown(directory.path(),100),100+3*24*60*60);
    save_cooldown(directory.path(),u64::MAX);assert_eq!(cooldown(directory.path(),100),u64::MAX);
}

#[test]
fn read_only_source_lookup_survives_rename_but_requires_same_key_and_external_ids() {
    let directory=tempfile::tempdir().unwrap();let original=query();
    assert!(write(directory.path(),&original,&matched(100)));
    let mut renamed=original.clone();renamed.title="My custom game name".into();
    assert!(read(directory.path(),&renamed,101,true).is_none());
    assert_eq!(read_source(directory.path(),&renamed,101),Some(matched(100)));
    assert!(read_source(directory.path(),&renamed,100+POSITIVE_TTL).unwrap().stale);
    renamed.key="another-source".into();assert!(read_source(directory.path(),&renamed,101).is_none());
    renamed.key=original.key.clone();renamed.external_ids.push(ExternalId{platform:"steam".into(),id:"42".into()});
    assert!(read_source(directory.path(),&renamed,101).is_none());
    assert!(read_source(directory.path(),&original,99).is_none());
}

#[test]
fn source_index_rejects_corruption_and_negative_replacement() {
    let directory=tempfile::tempdir().unwrap();let query=query();assert!(write(directory.path(),&query,&matched(100)));
    let path=directory.path().join("game-metadata").join(format!("source-{}.json",key_hash(&query)));
    fs::write(&path,b"broken").unwrap();assert!(read_source(directory.path(),&query,101).is_none());
    let index=SourceIndex{key_hash:"wrong".into(),external_ids:vec![],fingerprint:fingerprint(&query),lookup:matched(100)};
    fs::write(&path,serde_json::to_vec(&index).unwrap()).unwrap();assert!(read_source(directory.path(),&query,101).is_none());
    assert!(write(directory.path(),&query,&matched(100)));
    assert!(write(directory.path(),&query,&MetadataLookup::empty(LookupStatus::NotFound,101)));
    assert_eq!(read_source(directory.path(),&query,101),Some(matched(100)));
}

#[cfg(unix)]
#[test]
fn redirected_cache_paths_are_rejected() {
    use std::os::unix::fs::symlink;
    let directory=tempfile::tempdir().unwrap();let target=tempfile::tempdir().unwrap();
    symlink(target.path(),directory.path().join("game-metadata")).unwrap();
    assert!(!write(directory.path(),&query(),&matched(100)));
    assert!(read(directory.path(),&query(),100,true).is_none());
}

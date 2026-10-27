use super::*;
use std::sync::atomic::{AtomicUsize,Ordering};

fn query(key:&str)->MetadataQuery {MetadataQuery{key:key.into(),title:"Example Game".into(),external_ids:vec![]}}
fn empty_response()->Vec<u8> {br#"{"head":{"vars":[]},"results":{"bindings":[]}}"#.to_vec()}

#[test]
fn explicit_refresh_bypasses_fresh_metadata_cache_for_only_the_requested_game() {
    let root=tempfile::tempdir().unwrap();let target=query("selected");
    load_with(root.path(),&[target.clone()],&||false,||100,|_,_|Ok(empty_response()));
    let calls=AtomicUsize::new(0);
    let result=load_using(root.path(),&[target],&||false,true,||101,|batch,_|{
        calls.fetch_add(1,Ordering::Relaxed);assert_eq!(batch.len(),1);assert_eq!(batch[0].key,"selected");Ok(empty_response())
    });
    assert_eq!(calls.load(Ordering::Relaxed),1);assert_eq!(result["selected"].status,LookupStatus::NotFound);
}

#[test]
fn identical_queries_share_fetch_and_negative_cache_survives_new_local_keys() {
    let root=tempfile::tempdir().unwrap();let calls=AtomicUsize::new(0);
    let result=load_with(root.path(),&[query("first"),query("second")],&||false,||100,|batch,_| {
        calls.fetch_add(1,Ordering::Relaxed);assert_eq!(batch.len(),1);Ok(empty_response())
    });
    assert_eq!(calls.load(Ordering::Relaxed),1);
    assert_eq!(result["first"].status,LookupStatus::NotFound);assert_eq!(result["first"],result["second"]);
    let cached=load_with(root.path(),&[query("new-key")],&||false,||101,|_,_|panic!("fresh negative cache must skip transport"));
    assert_eq!(cached["new-key"].status,LookupStatus::NotFound);
}

#[test]
fn failures_are_unavailable_and_are_not_negative_cached() {
    let root=tempfile::tempdir().unwrap();
    let result=load_with(root.path(),&[query("game")],&||false,||100,|_,_|Err("offline".into()));
    assert_eq!(result["game"].status,LookupStatus::Unavailable);
    let result=load_with(root.path(),&[query("game")],&||false,||101,|_,_|Ok(empty_response()));
    assert_eq!(result["game"].status,LookupStatus::NotFound);
}

#[test]
fn stale_positive_is_returned_only_when_refresh_fails() {
    let root=tempfile::tempdir().unwrap();let query=query("game");
    let lookup=MetadataLookup{status:LookupStatus::Matched,metadata:Some(GameMetadata {
        source:"wikidata".into(),entity_id:"Q42".into(),url:"https://www.wikidata.org/wiki/Q42".into(),
        title:"Example Game".into(),description:None,aliases:vec![],genres:vec![],genre_ancestors:vec![],
        platforms:vec![],developers:vec![],publishers:vec![],release_dates:vec![],external_ids:vec![],
        fetched_at:100,matched_by:MatchMethod::ExactTitle,
    }),checked_at:100,stale:false};
    assert!(cache::write(root.path(),&query,&lookup));
    let mut alias=query.clone();alias.key="another-copy".into();
    let copied=load_with(root.path(),&[alias.clone()],&||false,||101,|_,_|panic!("fresh cache"));
    assert_eq!(copied["another-copy"],lookup);
    alias.title="Manually renamed game".into();
    assert_eq!(cached(root.path(),&alias).unwrap().status,LookupStatus::Matched);
    let renamed=load_with(root.path(),&[alias],&||false,||101,|_,_|Err("offline".into()));
    assert_eq!(renamed["another-copy"].status,LookupStatus::Unavailable);
    let at=100+31*24*60*60;
    let result=load_with(root.path(),&[query.clone()],&||false,||at,|_,_|Err("offline".into()));
    assert_eq!(result["game"].status,LookupStatus::Matched);assert!(result["game"].stale);
    let refreshed=load_with(root.path(),&[query],&||false,||at,|_,_|Ok(empty_response()));
    assert_eq!(refreshed["game"].status,LookupStatus::NotFound);assert!(!refreshed["game"].stale);
}

#[test]
fn invalid_or_oversized_payload_is_unavailable_without_negative_cache() {
    let root=tempfile::tempdir().unwrap();
    for bytes in [b"not json".to_vec(),vec![b' ';network::MAX_RESPONSE+1]] {
        let result=load_with(root.path(),&[query("game")],&||false,||100,|_,_|Ok(bytes.clone()));
        assert_eq!(result["game"].status,LookupStatus::Unavailable);
        assert!(cache::read(root.path(),&query("game"),100,true).is_none());
    }
}

#[test]
fn cancellation_and_invalid_queries_do_not_contact_provider() {
    let root=tempfile::tempdir().unwrap();
    assert!(load_with(root.path(),&[query("game")],&||true,||100,|_,_|panic!("cancelled")).is_empty());
    let mut invalid=query("game");invalid.title="".into();
    assert!(load_with(root.path(),&[invalid],&||false,||100,|_,_|panic!("invalid")).is_empty());
}

#[test]
fn batches_are_bounded_and_optional_cache_failure_does_not_block() {
    let root=tempfile::tempdir().unwrap();let queries=(0..45).map(|index|MetadataQuery {
        key:format!("game-{index}"),title:format!("Game {index}"),external_ids:vec![]
    }).collect::<Vec<_>>();
    let calls=AtomicUsize::new(0);
    let result=load_with(&root.path().join("absent"),&queries,&||false,||100,|batch,_| {
        calls.fetch_add(1,Ordering::Relaxed);assert!(batch.len()<=5);Ok(empty_response())
    });
    assert_eq!(calls.load(Ordering::Relaxed),9);assert_eq!(result.len(),45);
    assert!(result.values().all(|value|value.status==LookupStatus::NotFound));
}

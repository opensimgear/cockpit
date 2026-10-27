use super::*;
use std::collections::BTreeMap;

#[test]
fn real_wikidata_response_preserves_rich_metadata_and_neutral_titles() {
    let bytes=include_bytes!("../../../../tests/fixtures/wikidata/portal-2.json");
    let mut document:serde_json::Value=serde_json::from_slice(bytes).unwrap();
    let mut alias=document["results"]["bindings"][0].clone();
    alias["field"]=serde_json::json!({"type":"literal","value":"alias"});
    alias["value"]=serde_json::json!({"type":"literal","xml:lang":"en","value":"Portal Two"});
    document["results"]["bindings"].as_array_mut().unwrap().push(alias);
    let query=MetadataQuery{key:"fixture".into(),title:"Portal 2".into(),external_ids:vec![]};
    let values=wikidata::parse(&[query],&serde_json::to_vec(&document).unwrap(),1234).unwrap();
    let record=values["fixture"].metadata.as_ref().unwrap();
    assert_eq!(record.entity_id,"Q279446");assert_eq!(record.title,"Portal 2");assert!(record.valid());
    assert!(!record.genres.is_empty());assert!(!record.genre_ancestors.is_empty());
    assert!(!record.platforms.is_empty());assert!(!record.developers.is_empty());assert!(!record.publishers.is_empty());
    assert!(!record.release_dates.is_empty());assert!(!record.external_ids.is_empty());assert_eq!(record.aliases,["Portal Two"]);
    let genres=record.genres.iter().chain(&record.genre_ancestors).map(|entity|entity.label.clone()).collect::<Vec<_>>();
    assert!(!crate::game_installations::tags::from_metadata(&crate::game_installations::tags::defaults(),&genres).is_empty());
}

#[test]
fn records_keep_provenance_and_fields_without_owning_local_settings() {
    let record=test_record("Fixture","Racing");assert!(record.valid());
    let json=serde_json::to_value(&record).unwrap();
    for key in ["source","entityId","url","title","description","genres","genreAncestors","platforms","developers","publishers","releaseDates","externalIds","fetchedAt","matchedBy"] {
        assert!(json.get(key).is_some(),"{key}");
    }
    assert!(json.get("executable").is_none());assert!(json.get("tagIds").is_none());assert!(json.get("artwork").is_none());
    let mut invalid=record.clone();invalid.url="https://evil.example/Q123".into();assert!(!invalid.valid());
    invalid=record.clone();invalid.entity_id="Q0".into();assert!(!invalid.valid());
    invalid=record.clone();invalid.genres[0].label="bad\nlabel".into();assert!(!invalid.valid());
    invalid=record.clone();invalid.description=Some("x".repeat(2049));assert!(!invalid.valid());
    let lookup=MetadataLookup{status:LookupStatus::Matched,metadata:Some(record),checked_at:1,stale:false};assert!(lookup.valid());
    let mut invalid=lookup.clone();invalid.checked_at=2;assert!(!invalid.valid());
    invalid=lookup;invalid.status=LookupStatus::NotFound;assert!(!invalid.valid());
}

#[test]
fn source_queries_bound_untrusted_titles_and_ids() {
    let query=MetadataQuery{key:"disk:opaque".into(),title:"Portal 2™".into(),external_ids:vec![]};
    assert!(query.valid());assert_eq!(normalized_title(&query.title),"portal 2");
    assert_eq!(normalized_title("Diablo® II: Resurrected"),"diablo ii resurrected");
    let mut bad=query.clone();bad.title="A\nB".into();assert!(!bad.valid());
    bad=query;bad.external_ids=vec![ExternalId{platform:"steam".into(),id:"x".repeat(257)}];assert!(!bad.valid());
}

#[test]
#[ignore="Calls the live Wikidata endpoint and records a bounded metadata cache"]
fn live_wikidata_metadata_smoke() {
    let root=std::path::PathBuf::from(std::env::var("OSG_WIKIDATA_SMOKE_ROOT").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let queries=[MetadataQuery{key:"disk:portal2".into(),title:"Portal 2".into(),external_ids:vec![]},
        MetadataQuery{key:"steam:620".into(),title:"Portal 2".into(),external_ids:vec![ExternalId{platform:"steam".into(),id:"620".into()}]}];
    std::fs::write(root.join("smoke-query.rq"),wikidata::query(&queries)).unwrap();
    let results=load(&root,&queries,||false);
    let evidence=results.iter().map(|(key,result)|(key.clone(),serde_json::json!({"status":result.status,"metadata":result.metadata,"stale":result.stale}))).collect::<BTreeMap<_,_>>();
    std::fs::write(root.join("smoke-results.json"),serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    assert!(results.values().all(|lookup|lookup.status==LookupStatus::Matched),"Inspect smoke-results.json for endpoint availability.");
    assert!(results.values().all(|lookup|lookup.metadata.as_ref().unwrap().valid()));
}

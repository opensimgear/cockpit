use super::*;
use std::fs;

fn candidate(root:&Path,id:&str,title:Option<&str>)->Candidate {
    fs::create_dir_all(root).unwrap();
    let executable=root.join("Game.exe");fs::write(&executable,b"fixture").unwrap();
    Candidate{id:format!("steam:{id}"),client:"steam".into(),name:"Fixture".into(),
        install_path:root.to_string_lossy().into_owned(),executable_candidates:vec![executable.to_string_lossy().into_owned()],
        preferred_executable:None,game_id:title.map(str::to_owned),kind:"game".into(),app_id:id.into(),launcher:None,artwork:None}
}
fn review(saved:&crate::game_installations::Settings,candidates:Vec<Candidate>,metadata:Metadata)->Review {
    Review{scan_id:"tags".into(),status:"ready".into(),revision:saved.revision,
        entries:super::super::reconcile::with_metadata(saved,candidates,&BTreeSet::from(["steam".into()]),&metadata),..Default::default()}
}
fn accept(saved:&crate::game_installations::Settings,review:&Review,actions:Value)->crate::game_installations::Settings {
    super::super::apply(saved,review,&json!({"scanId":"tags","actions":actions})).unwrap()
}
fn platform(id:&str,labels:&[&str])->(String,SourceMetadata) {
    (format!("steam:{id}"),SourceMetadata{platform_genres:labels.iter().map(|label|(*label).into()).collect(),..Default::default()})
}
fn metadata()->Metadata {BTreeMap::from([platform("42",&["Racing","Simulation"])])}

#[test]
fn new_imports_seed_reviewed_tags_and_unchanged_games_can_gain_metadata_later() {
    let directory=crate::test_directory();let found=candidate(&directory.path().join("game"),"42",None);
    let saved=crate::game_installations::Settings::default();
    let pending=review(&saved,vec![found.clone()],metadata());
    assert_eq!(pending.entries[0].tag_ids,["racing","simulation"]);
    assert_eq!(serde_json::to_value(&pending.entries[0]).unwrap()["tagIds"],json!(["racing","simulation"]));
    assert!(pending.entries[0].message.as_ref().unwrap().contains("Platform tags:"));
    let imported=accept(&saved,&pending,json!([]));let id=imported.games.keys().next().unwrap();
    assert_eq!(imported.game_tags[id],["racing","simulation"]);
    assert!(review(&imported,vec![found.clone()],metadata()).entries.is_empty());

    // Offline discovery remains useful; later metadata becomes an explicit update.
    let offline=accept(&saved,&review(&saved,vec![found.clone()],BTreeMap::new()),json!([]));
    assert!(offline.game_tags.is_empty());
    let later=review(&offline,vec![found],metadata());
    assert_eq!(later.entries.len(),1);assert_eq!(later.entries[0].action,"update");
    let skipped=accept(&offline,&later,json!([{"id":later.entries[0].id,"selected":false}]));
    assert!(skipped.game_tags.is_empty());
    assert_eq!(accept(&offline,&later,json!([])).game_tags[id],["racing","simulation"]);
}

#[test]
fn edited_cleared_and_deleted_taxonomy_choices_survive_more_installations() {
    let directory=crate::test_directory();let found=candidate(&directory.path().join("game"),"42",Some("dcs"));
    let saved=crate::game_installations::Settings::default();
    let saved=accept(&saved,&review(&saved,vec![found.clone()],metadata()),json!([]));
    for ids in [vec!["flight".to_owned()],vec![]] {
        let mut edited=saved.clone();edited.game_tags.insert("dcs".into(),ids.clone());
        let second=candidate(&directory.path().join(if ids.is_empty(){"empty"}else{"edited"}),"42",Some("dcs"));
        let pending=review(&edited,vec![found.clone(),second],metadata());
        assert_eq!(pending.entries.len(),1);assert!(pending.entries[0].tag_ids.is_empty());
        assert_eq!(accept(&edited,&pending,json!([])).game_tags["dcs"],ids);
    }
    let mut renamed=crate::game_installations::Settings::default();renamed.tags.insert("racing".into(),"Motorsport".into());renamed.tags.remove("simulation");
    let pending=review(&renamed,vec![found],metadata());
    assert_eq!(pending.entries[0].tag_ids,["racing"]);
    assert!(pending.entries[0].message.as_ref().unwrap().contains("Motorsport"));
    assert!(!accept(&renamed,&pending,json!([])).tags.contains_key("simulation"));
}

#[test]
fn selected_copies_merge_evidence_for_resolved_identity_without_overwriting_manual_tags() {
    let directory=crate::test_directory();let a=candidate(&directory.path().join("a"),"42",None);
    let b=candidate(&directory.path().join("b"),"43",None);
    let saved=crate::game_installations::Settings::default();
    let tags=BTreeMap::from([platform("42",&["Racing"]),platform("43",&["Simulation"])]);
    let pending=review(&saved,vec![a.clone(),b.clone()],tags.clone());
    let first=&pending.entries[0];let second=&pending.entries[1];
    let actions=json!([{"id":second.id,"selected":true,"gameId":first.game_id}]);
    let merged=accept(&saved,&pending,actions);
    assert_eq!(merged.games.len(),1);assert_eq!(merged.game_tags[&first.game_id],["racing","simulation"]);
    let single=accept(&saved,&pending,json!([{"id":second.id,"selected":false}]));
    assert_eq!(single.game_tags[&first.game_id],first.tag_ids);
    let mut edited=single;edited.game_tags.insert(first.game_id.clone(),vec![]);
    let other=if same_path(&first.install_path,&a.install_path){b}else{a};
    let pending=review(&edited,vec![other],tags);
    let actions=json!([{"id":pending.entries[0].id,"selected":true,"gameId":first.game_id}]);
    assert!(accept(&edited,&pending,actions).game_tags[&first.game_id].is_empty());
}

#[test]
fn cancelled_and_empty_enrichment_does_not_block_local_discovery() {
    let directory=crate::test_directory();let mut found=candidate(&directory.path().join("game"),"42",None);
    let saved=crate::game_installations::Settings::default();
    assert!(load(&saved,&[found.clone()],directory.path(),||true).is_empty());
    found.client="disk".into();
    let mut tool=found.clone();tool.kind="tool".into();
    assert!(load(&saved,&[tool],directory.path(),||false).is_empty());
    let pending=review(&saved,vec![found],BTreeMap::new());
    assert!(pending.entries[0].tag_ids.is_empty());assert_eq!(accept(&saved,&pending,json!([])).games.len(),1);
}

#[test]
fn wikidata_fallback_applies_to_every_library_without_replacing_platform_or_manual_tags() {
    let directory=crate::test_directory();
    for (client,_) in crate::game_library::CLIENTS {
        let mut found=candidate(&directory.path().join(client),"42",None);found.client=(*client).into();
        let record=crate::game_metadata::test_record("Remote display title","role-playing video game");
        let data=BTreeMap::from([(format!("{client}:42"),SourceMetadata{wikidata:Some(record),..Default::default()})]);
        let saved=crate::game_installations::Settings::default();
        let pending=review(&saved,vec![found.clone()],data.clone());
        assert_eq!(pending.entries[0].tag_ids,["rpg"],"{client}");
        assert_eq!(pending.entries[0].tag_source.as_deref(),Some("wikidata"));
        assert!(pending.entries[0].message.as_ref().unwrap().contains("Wikidata tags:"));
        assert_eq!(pending.entries[0].name,"Fixture","Remote facts cannot rename games");
        let imported=accept(&saved,&pending,json!([]));let id=imported.games.keys().next().unwrap();
        for manual in [vec!["flight".into()],vec![]] {
            let mut edited=imported.clone();edited.game_tags.insert(id.clone(),manual.clone());
            assert!(review(&edited,vec![found.clone()],data.clone()).entries.is_empty());
            assert_eq!(edited.game_tags[id],manual);
        }
        let mut primary=data;primary.get_mut(&format!("{client}:42")).unwrap().platform_genres=vec!["Racing".into()];
        assert_eq!(review(&saved,vec![found],primary).entries[0].tag_ids,["racing"]);
    }
}

#[test]
fn cached_wikidata_enriches_local_titles_even_when_tags_were_manually_cleared() {
    let directory=crate::test_directory();let mut found=candidate(&directory.path().join("local"),"42",None);found.client="disk".into();
    let query=query("disk","42","Fixture");
    let mut record=crate::game_metadata::test_record("Fixture","racing video game");
    record.fetched_at=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let lookup=crate::game_metadata::MetadataLookup{checked_at:record.fetched_at,status:crate::game_metadata::LookupStatus::Matched,metadata:Some(record),stale:false};
    assert!(crate::game_metadata::test_store(directory.path(),&query,&lookup));
    let saved=crate::game_installations::Settings::default();
    let metadata=load(&saved,&[found.clone()],directory.path(),||false);
    assert!(metadata["disk:42"].wikidata.is_some());
    let pending=review(&saved,vec![found.clone()],metadata.clone());let mut imported=accept(&saved,&pending,json!([]));
    let id=imported.games.keys().next().unwrap().clone();imported.game_tags.insert(id,vec![]);
    assert!(load(&imported,&[found.clone()],directory.path(),||false)["disk:42"].wikidata.is_some());
    assert!(review(&imported,vec![found],metadata).entries.is_empty());
}

#[test]
fn wiki_sources_prefer_store_identity_then_platform_title_and_reject_equal_conflicts() {
    let saved=crate::game_installations::Settings::default();
    let sources=[LibrarySource{client:"battlenet".into(),app_id:"D2R".into(),install_path:"C:/Fixture".into(),launcher:None},
        LibrarySource{client:"disk".into(),app_id:"folder".into(),install_path:"C:/Fixture".into(),launcher:None}];
    let first=crate::game_metadata::test_record("Installed game","role-playing video game");
    let mut other=crate::game_metadata::test_record("Generic folder title","racing video game");
    other.entity_id="Q456".into();other.url="https://www.wikidata.org/wiki/Q456".into();
    let mut data=BTreeMap::from([("battlenet:D2R".into(),SourceMetadata{wikidata:Some(first),..Default::default()}),
        ("disk:folder".into(),SourceMetadata{wikidata:Some(other),..Default::default()})]);
    assert_eq!(proposed(&saved,"fixture",&sources,&data).ids,["rpg"]);
    data.get_mut("disk:folder").unwrap().wikidata.as_mut().unwrap().matched_by=MatchMethod::ExternalId;
    assert_eq!(proposed(&saved,"fixture",&sources,&data).ids,["racing"]);
    let mut equal=sources.clone();equal[1].client="gog".into();
    let other=data.remove("disk:folder").unwrap();data.insert("gog:folder".into(),other);
    data.get_mut("gog:folder").unwrap().wikidata.as_mut().unwrap().matched_by=MatchMethod::ExactTitle;
    assert!(proposed(&saved,"fixture",&equal,&data).ids.is_empty());
}

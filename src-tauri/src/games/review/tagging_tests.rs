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
fn metadata()->Metadata {BTreeMap::from([(42,vec!["Racing".into(),"Simulation".into()])])}

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
    let tags=BTreeMap::from([(42,vec!["Racing".into()]),(43,vec!["Simulation".into()])]);
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
    assert!(load(&saved,&[found.clone()],directory.path(),||false).is_empty());
    let pending=review(&saved,vec![found],BTreeMap::new());
    assert!(pending.entries[0].tag_ids.is_empty());assert_eq!(accept(&saved,&pending,json!([])).games.len(),1);
}

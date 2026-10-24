use super::*;
fn empty()->Value {json!({"sources":builtins()["definitions"],"drafts":[],"publications":{}})}
fn read(r:&Runtime)->Value {r.request(&json!({"command":"designer_workspace_read"})).unwrap()}
fn write(r:&Runtime,revision:u64,value:Value)->Result<Value,String> {r.request(&json!({"command":"designer_workspace_write","expectedRevision":revision,"expectedLibraryRevision":read(r)["libraryRevision"],"value":value}))}
#[test]
fn workspace_sources_and_drafts_survive_native_restart_without_file_stores() {
    let dir=crate::test_directory();let path=dir.path().join("widgets.json");
    std::fs::write(&path,b"retired invalid file").unwrap();
    let r=Runtime::new(Some(path.clone()));assert_eq!(read(&r)["value"],empty());
    let mut value=empty(); value["drafts"]=json!([{"id":"draft","title":"Unsaved compilation","pages":[{"id":"page","title":"Main","width":1280,"height":800}],"widgets":[]}]);
    assert_eq!(write(&r,0,value.clone()).unwrap()["revision"],1);
    let reopened=Runtime::new(Some(path.clone()));assert_eq!(read(&reopened)["value"],value);
    assert_eq!(std::fs::read(path).unwrap(),b"retired invalid file");
    assert!(write(&r,0,empty()).unwrap_err().contains("conflict"));
}
#[test]
fn locked_writer_commits_neither_workspace_nor_widget_library() {
    let dir=crate::test_directory();let path=dir.path().join("widgets.json");let r=Runtime::new(Some(path.clone()));
    write(&r,0,empty()).unwrap();let db=osg_persistence::Database::for_namespace(&path).unwrap();
    let mut value=empty();let mut source=value["sources"][0].clone(); source["id"]=json!("new-source");value["sources"].as_array_mut().unwrap().push(source);
    let lock=osg_persistence::test_support::hold_writer(&db).unwrap();
    assert!(write(&r,1,value.clone()).unwrap_err().contains("busy"));drop(lock);
    assert_eq!(read(&r)["revision"],1);
    assert_eq!(r.request(&json!({"command":"widgets_list"})).unwrap()["total"],1);
    assert_eq!(write(&r,1,value).unwrap()["revision"],2);
    assert_eq!(r.request(&json!({"command":"widgets_list"})).unwrap()["total"],2);
}
#[test]
fn workspace_keeps_external_sources_and_refreshes_shared_runtime() {
    let dir=crate::test_directory();let path=dir.path().join("widgets.json");let r=Runtime::new(Some(path.clone()));write(&r,0,empty()).unwrap();
    let external=Runtime::new(Some(path));let mut value=empty();value["sources"][0]["id"]=json!("mcp-source");
    external.request(&json!({"command":"widget_library_merge","expectedRevision":1,"bundle":{"definitions":value["sources"]}})).unwrap();
    assert!(write(&r,1,empty()).is_err());let refreshed=read(&r);let saved=write(&r,refreshed["revision"].as_u64().unwrap(),refreshed["value"].clone()).unwrap();assert_eq!(saved["value"]["sources"].as_array().unwrap().len(),2);
    assert_eq!(r.request(&json!({"command":"widgets_list"})).unwrap()["total"],2);
    assert_eq!(read(&r)["value"]["sources"].as_array().unwrap().len(),2);
}
#[test]
fn invalid_workspace_preserves_current_snapshot_and_sources() {
    let r=Runtime::new(None);write(&r,0,empty()).unwrap();
    let mut invalids=vec![];let mut v=empty();v["extra"]=json!(1);invalids.push(v);
    let mut v=empty();v["sources"][0]["formatVersion"]=json!(2);invalids.push(v);
    let mut v=empty();v["drafts"]=json!([{"id":"draft","title":"Bad","pages":[],"widgets":[]}]);invalids.push(v);
    let mut v=empty();v["publications"]=json!({"bad":[{}]});invalids.push(v);
    for v in invalids {assert!(write(&r,1,v).is_err());assert_eq!(read(&r)["value"],empty());}
    assert!(r.request(&json!({"command":"designer_workspace_read","path":"secret"})).is_err());
}

fn compiled()->Value {serde_json::from_str(include_str!("../../../../crates/companion-service/tests/fixtures/compiled-workspace.json")).unwrap()}

#[test]
fn compiled_publication_assets_and_source_parameters_validate_together() {
    let dir=crate::test_directory();let path=dir.path().join("workspace.json");let r=Runtime::new(Some(path.clone()));
    let value=compiled();write(&r,0,value.clone()).unwrap();assert_eq!(read(&Runtime::new(Some(path)))["value"],value);
    let draft=&value["drafts"][0];let key=draft["id"].as_str().unwrap();
    let mutations:Vec<Box<dyn Fn(&mut Value)>>=vec![
        Box::new(|v|v["drafts"][0]["title"]=json!("New name without recompiling")),
        Box::new(|v|v["drafts"][0]["widgets"][0]["label"]=json!("Different label")),
        Box::new(|v|v["sources"][0]["title"]=json!("Source hash changed")),
        Box::new(|v|v["drafts"][0]["widgets"][0]["parameters"]=json!({"label":"Changed"})),
        Box::new(|v|v["publications"]["toggle-native-fixture"][0]["assets"][0]["base64"]=json!("AAAA")),
    ];
    for mutate in mutations {let mut bad=value.clone();mutate(&mut bad);assert!(validation::validate(&bad).is_err());}
    let mut bad=value.clone();bad["publications"][key][0]["publicationHash"]=json!(format!("sha256:{}","0".repeat(64)));assert!(validation::validate(&bad).is_err());
    let mut wrong=value.clone();let archive=wrong["publications"].as_object_mut().unwrap().remove(key).unwrap();wrong["publications"]["other-panel"]=archive;assert!(validation::validate(&wrong).is_err());
}
#[test]
fn native_draft_admission_rejects_invalid_shapes_bounds_and_source_references() {
    let value=compiled();
    let changes=vec![
        ("/drafts/0/id",json!("bad id")),("/drafts/0/title",json!("")),("/drafts/0/pages/0/width",json!(319)),
        ("/drafts/0/pages/0/height",json!(8193)),("/drafts/0/widgets/0/id",json!("main-page")),
        ("/drafts/0/widgets/0/pageId",json!("missing")),("/drafts/0/widgets/0/sourceId",json!("missing")),
        ("/drafts/0/widgets/0/parameters",json!({"unknown":1})),("/drafts/0/widgets/0/parameters",json!([])),
        ("/drafts/0/widgets/0/bounds/x",json!(1.1)),("/drafts/0/widgets/0/bounds/width",json!(0)),
        ("/drafts/0/widgets/0/bounds/y",json!(0.99)),("/drafts/0/widgets/0/bounds/height",json!("invalid")),
        ("/sources",json!({})),("/drafts",json!({})),("/publications",json!([])),("/sources",json!([])),
    ];
    for (pointer,new) in changes {let mut bad=value.clone();*bad.pointer_mut(pointer).unwrap()=new;assert!(validation::validate(&bad).is_err(),"{pointer}");}
    let mut duplicate=value.clone();duplicate["drafts"].as_array_mut().unwrap().push(value["drafts"][0].clone());assert!(validation::validate(&duplicate).is_err());
    let mut bad=empty();bad["drafts"]=json!(vec![value["drafts"][0].clone();21]);assert!(validation::validate(&bad).is_err());
    let mut bad=value.clone();let key=bad["drafts"][0]["id"].as_str().unwrap().to_owned();let p=bad["publications"][&key][0].clone();bad["publications"][key]=json!(vec![p;101]);assert!(validation::validate(&bad).is_err());
}

#[test]
fn native_parameters_enforce_domains_and_corrupt_sqlite_records_never_seed_defaults() {
    let mut v=compiled();v["publications"]=json!({});
    v["sources"][0]["parameters"].as_array_mut().unwrap().extend([
        json!({"id":"amount","label":"Amount","type":"number","min":0,"max":10,"default":5}),
        json!({"id":"visible","label":"Visible","type":"boolean","default":true}),
    ]);
    validation::validate(&v).unwrap();
    for parameters in [json!({"amount":11}),json!({"visible":1}),json!({"accent":"bad"}),json!({"label":"bad\n"})] {
        let mut bad=v.clone();bad["drafts"][0]["widgets"][0]["parameters"]=parameters;assert!(validation::validate(&bad).is_err());
    }
    let dir=crate::test_directory();let path=dir.path().join("workspace.json");let db=osg_persistence::Database::for_namespace(&path).unwrap();
    db.commit(&[osg_persistence::Change{domain:"companion",key:"designer-workspace",expected_revision:0,payload:Some(b"invalid JSON")}]).unwrap();
    let r=Runtime::new(Some(path));assert!(r.request(&json!({"command":"designer_workspace_read"})).unwrap_err().contains("Invalid stored"));
    assert!(r.request(&json!({"command":"designer_workspace_write","expectedRevision":1,"expectedLibraryRevision":0,"value":empty()})).is_err());assert_eq!(db.read("companion","designer-workspace").unwrap().unwrap().payload,b"invalid JSON");
}

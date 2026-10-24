use super::*;
fn setup()->(tempfile::TempDir,Database,Preferences){let root=crate::test_directory();let db=Database::for_namespace(&root.path().join("config.json")).unwrap();let service=Preferences::from_database(Ok(db.clone()));(root,db,service)}
#[test]
fn preferences_use_single_sqlite_authority_and_defaults_never_write_records(){
    let(_root,db,service)=setup();let snapshot=service.snapshot(&FRONTEND_KEYS).unwrap();assert_eq!(snapshot["entries"]["theme"]["value"],false);assert!(snapshot["entries"]["remembered"]["value"].is_null());assert_eq!(db.revision("inventory","remembered").unwrap(),0);assert_eq!(db.revision("ui","theme").unwrap(),0);
    let saved=service.write("theme",0,json!(true)).unwrap();assert_eq!(saved.revision,1);assert_eq!(service.cached("theme"),true);
    let reopened=Preferences::from_database(Ok(db.clone()));assert_eq!(reopened.read("theme").unwrap().value,true);assert!(reopened.write("theme",0,json!(false)).is_err());
    assert_eq!(reopened.write("devices-mode",0,json!("topology")).unwrap().value,"topology");assert_eq!(db.read("ui","devices-mode").unwrap().unwrap().revision,1);
    let memory=json!({"components":[],"forgotten":{"offline":{"signature":"known","sawAbsent":false,"type":"controller","provider":"controllers"}}});service.write("remembered",0,memory.clone()).unwrap();assert_eq!(db.read("inventory","remembered").unwrap().unwrap().payload,serde_json::to_vec(&memory).unwrap());assert_eq!(reopened.read("remembered").unwrap().value,memory);
}
#[test]
fn invalid_busy_and_corrupt_records_preserve_authority_and_do_not_publish_success(){
    let(_root,db,service)=setup();let observed=Arc::new(Mutex::new(vec![]));service.observe({let observed=observed.clone();Arc::new(move|key,revision|observed.lock().unwrap().push((key.to_owned(),revision)))});
    service.write("theme",0,json!(true)).unwrap();assert_eq!(observed.lock().unwrap().len(),1);
    assert!(service.write("theme",1,json!("invalid")).is_err());assert!(service.write("unknown",0,Value::Null).is_err());assert!(service.write("theme",u64::MAX,json!(false)).is_err());
    let lock=osg_persistence::test_support::hold_writer(&db).unwrap();assert!(service.write("theme",1,json!(false)).is_err());drop(lock);assert_eq!(service.cached("theme"),true);assert_eq!(observed.lock().unwrap().len(),1);
    db.commit(&[Change{domain:"ui",key:"theme",expected_revision:1,payload:Some(b"invalid")}]).unwrap();assert!(service.read("theme").is_err());assert_eq!(service.cached("theme"),false);assert_eq!(db.read("ui","theme").unwrap().unwrap().payload,b"invalid");
    assert!(Preferences::from_database(Err("read-only".into())).read("theme").is_err());
}
#[test]
fn typed_dispatch_rejects_extra_fields_and_tray_intent_requires_observed_availability(){
    let(_root,_db,service)=setup();assert!(service.write("tray",0,json!({"closeToTray":true,"minimizeToTray":false})).is_err());service.set_tray_available(true);service.write("tray",0,json!({"closeToTray":true,"minimizeToTray":false})).unwrap();
    assert_eq!(service.request(&json!({"command":"ui_preferences_get","clientId":"typed","key":"tray"})).unwrap()["revision"],1);
    assert_eq!(service.request(&json!({"command":"ui_preferences_save","clientId":"typed","key":"progress","expectedRevision":0,"value":{"delivery":"window","position":"top-left"}})).unwrap()["value"]["delivery"],"window");
    for request in [json!({}),json!({"command":"sql"}),json!({"command":"ui_preferences_get","key":1}),json!({"command":"ui_preferences_get","sql":"SELECT"}),json!({"command":"ui_preferences_save","key":"theme","value":false}),json!({"command":"ui_preferences_save","expectedRevision":0,"value":false}),json!({"command":"ui_preferences_save","key":"theme","expectedRevision":0})]{assert!(service.request(&request).is_err());}
}
#[test]
fn inventory_contract_preserves_history_references_and_tombstones_without_importing_old_envelopes(){
    let(_root,db,service)=setup();let component=json!({"id":"offline","name":"Offline controller","type":"controller","role":"","presence":"offline","hidden":false,"enabled":true,"owner":null,"capture":"unknown","status":"Disconnected","diagnostics":["Provider unavailable"],"connections":[{"id":"usb:offline","from":"host","to":"offline","transport":"usb","current":false,"observed":true,"endpoint":"COM9"}],"targets":[{"id":"offline","kind":"controller"}],"evidence":["local:offline"],"references":[{"kind":"binding","id":"steering","name":"Steering"}],"firstSeen":10,"lastSeen":20,"observedAt":30,"provider":"controllers","stale":true,"forget":{"allowed":false,"reason":"Refresh first","effect":"Local metadata"}});
    let value=json!({"components":[component],"forgotten":{"forgotten":{"signature":"known","sawAbsent":false,"type":"controller","provider":"controllers"}}});service.write("remembered",0,value.clone()).unwrap();let restored=Preferences::from_database(Ok(db)).read("remembered").unwrap().value;assert_eq!(restored["components"][0]["presence"],"unknown");assert_eq!(restored["components"][0]["firstSeen"],10);assert_eq!(restored["components"][0]["lastSeen"],20);assert_eq!(restored["components"][0]["references"],value["components"][0]["references"]);assert_eq!(restored["forgotten"],value["forgotten"]);
    for (field,bad) in [("id",json!("")),("type",json!("other")),("firstSeen",json!(-1)),("capture",json!("active")),("owner",json!(1)),("enabled",json!("yes")),("status",Value::Null),("diagnostics",json!([{}])),("connections",json!([{"id":"unsafe"}])),("targets",json!([{"id":"x","kind":"shell"}])),("references",json!([{"kind":"binding","id":"x"}])),("forget",json!({"allowed":"yes","reason":"","effect":""}))] {
        let mut invalid=value.clone();invalid["components"][0][field]=bad;assert!(service.write("remembered",1,invalid).is_err(),"{field}");
    }
    let mut duplicate=value.clone();let repeated=duplicate["components"][0].clone();duplicate["components"].as_array_mut().unwrap().push(repeated);assert!(validate("remembered",&duplicate).is_err());
    for invalid in [json!({"version":1,"components":[],"forgotten":{}}),json!({"components":[],"forgotten":{"bad":{"signature":"known","sawAbsent":true,"type":"unknown"}}}),json!({"components":[],"forgotten":[]})]{assert!(service.write("remembered",1,invalid).is_err());}
    assert_eq!(service.write("remembered",1,Value::Null).unwrap().revision,2);
}
#[test]
fn each_preference_domain_rejects_invalid_values_and_roundtrips_without_schema_envelopes(){
    let(_root,db,service)=setup();for (key,value) in [("theme",json!(true)),("devices-mode",json!("topology")),("canvas-navigation",json!({"wheelAction":"zoom"})),("canvas-styling",json!({"arrowStyle":"orthogonal","cornerRadius":100,"collapseParallelWires":false})),("tray",json!({"closeToTray":false,"minimizeToTray":false})),("progress",json!({"delivery":"window","position":"top-left"})),("windows",json!({"x":-1200,"y":10,"width":1000,"height":800,"scaleFactor":1.5,"maximized":true}))]{
        service.write(key,0,value.clone()).unwrap();assert_eq!(Preferences::from_database(Ok(db.clone())).read(key).unwrap().value,value);assert!(service.write(key,1,json!({"schemaVersion":1})).is_err());
    }
    for key in ["progress","canvas-navigation","canvas-styling","tray"]{assert!(service.write(key,1,json!(true)).is_err());}
    assert!(service.write("canvas-styling",1,json!({"arrowStyle":"smooth","cornerRadius":101,"collapseParallelWires":true})).is_err());assert!(service.write("windows",1,json!({"x":0,"y":0,"width":0,"height":800,"scaleFactor":0.0,"maximized":false})).is_err());
}

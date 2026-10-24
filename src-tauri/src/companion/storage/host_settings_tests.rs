use super::*;
fn config()->ServerConfig{ServerConfig{bind:"127.0.0.1:19443".parse().unwrap(),advertised_host:"127.0.0.1".into(),discovery:false}}
#[test]
fn listener_sqlite_restart_disable_and_stale_writers_preserve_acknowledged_settings(){
    let dir=crate::test_directory();std::fs::write(dir.path().join("companion-listener.json"),b"invalid retired file").unwrap();
    let first=Listener::open(dir.path()).unwrap();let stale=Listener::open(dir.path()).unwrap();assert!(first.snapshot().1.is_none());
    first.save(Some(config()),Some(0)).unwrap();assert_eq!(first.snapshot().0,1);
    assert!(stale.save(None,None).unwrap_err().contains("conflict"));assert!(first.save(None,Some(0)).is_err());
    assert_eq!(stale.refresh().unwrap().0,1);assert!(stale.save(None,Some(0)).is_err());
    let reopened=Listener::open(dir.path()).unwrap();assert_eq!(reopened.snapshot().1.unwrap().bind,config().bind);
    let mut effect=false;reopened.save_with_effect(None,Some(1),||effect=true).unwrap();assert!(effect);
    assert!(Listener::open(dir.path()).unwrap().snapshot().1.is_none());
    assert_eq!(std::fs::read(dir.path().join("companion-listener.json")).unwrap(),b"invalid retired file");
}
#[test]
fn listener_failed_commits_never_apply_effect_or_advance_memory(){
    let dir=crate::test_directory();let listener=Listener::open(dir.path()).unwrap();listener.save(Some(config()),None).unwrap();
    let lock=osg_persistence::test_support::hold_writer(&listener.database).unwrap();let mut applied=false;
    assert!(listener.save_with_effect(None,Some(1),||applied=true).unwrap_err().contains("busy"));drop(lock);
    assert!(!applied);assert_eq!(listener.snapshot().0,1);assert!(listener.snapshot().1.is_some());
    assert_eq!(listener.with_config(|c|c.unwrap().bind),config().bind);
    let mut bad=config();bad.advertised_host="https://bad".into();assert!(listener.save(Some(bad),None).is_err());
    listener.database.commit(&[Change{domain:"companion",key:"listener",expected_revision:1,payload:Some(b"invalid")}]).unwrap();assert!(Listener::open(dir.path()).is_err());
}
#[test]
fn firmware_observations_are_revisioned_and_invalid_records_remain_errors(){
    let dir=crate::test_directory();std::fs::write(dir.path().join("companion-firmware.json"),b"retired").unwrap();
    let(first,empty)=firmware(dir.path()).unwrap();assert_eq!(empty,json!({}));let(stale,_)=firmware(dir.path()).unwrap();
    let value=json!({"screen":{"profile":"sunton-3248s035r","version":"1.2.3","connectionMode":"serial"}});first.save(&value).unwrap();
    assert!(stale.save(&json!({})).unwrap_err().contains("conflict"));assert_eq!(firmware(dir.path()).unwrap().1,value);
    for bad in [json!([]),json!({"screen":[]}),json!({"bad id":{}}),json!({"screen":{"extra":true}}),json!({"screen":{"version":1}}),json!({"screen":{"connectionMode":"other"}}),json!({"screen":{"renderProfile":{}}})] {assert!(first.save(&bad).is_err());}
    assert_eq!(firmware(dir.path()).unwrap().1,value);
    let db=database(dir.path()).unwrap();let lock=osg_persistence::test_support::hold_writer(&db).unwrap();assert!(first.save(&json!({})).is_err());drop(lock);
    db.commit(&[Change{domain:"companion",key:"firmware",expected_revision:1,payload:Some(b"invalid")}]).unwrap();assert!(firmware(dir.path()).is_err());
}

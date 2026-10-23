fn tamper(path:&std::path::Path,payload:&[u8]) {
    let db=osg_persistence::Database::for_source(path).unwrap();
    db.commit(&[osg_persistence::Change{domain:"controllers",key:"visibility",format_version:1,
        expected_revision:db.revision("controllers","visibility").unwrap(),payload:Some(payload)}]).unwrap();
}
use super::*;
#[test]
fn explicit_intent_roundtrip_stays_disabled_and_external_edits_are_preserved() {
    let temp = crate::test_directory();
    let path = temp.path().join("settings.json");
    let mut store = Settings::new(Some(path.clone()));
    assert!(!path.exists());
    assert!(!store.value.enabled);
    let mut value = store.value.clone();
    let id = uuid::Uuid::new_v4().to_string();
    value.devices.insert(
        id.clone(),
        Intent::HideFromProcess {
            executable: r"C:\Games\game.exe".into(),
        },
    );
    store.save(value.clone()).unwrap();
    assert!(!store.value.enabled);
    assert_eq!(Settings::new(Some(path.clone())).value, value);
    value.enabled = true;
    store.save(value).unwrap();
    tamper(&path,b"external");
    assert!(store.save(Preferences::default()).is_err());
    assert_eq!(osg_persistence::Database::for_source(&path).unwrap().read("controllers","visibility").unwrap().unwrap().payload,b"external");
    let mut broken = Settings::new(Some(path));
    assert!(broken.error.is_some());
    assert!(broken.save(Preferences::default()).is_err());
}
#[test]
fn limits_identity_schema_paths_and_competing_editors_are_checked() {
    for path in [
        "",
        "game.exe",
        "1234",
        r"C:\Games\..\game.exe",
        r"C:\Games\file.txt",
    ] {
        assert!(Intent::HideFromProcess {
            executable: path.into()
        }
        .validate()
        .is_err());
    }
    let mut value = Preferences::default();
    value.schema = 2;
    assert!(value.validate().is_err());
    value.schema = 1;
    value.devices.insert("bad".into(), Intent::HideAll);
    assert!(value.validate().is_err());
    value.devices.clear();
    for _ in 0..257 {
        value
            .devices
            .insert(uuid::Uuid::new_v4().to_string(), Intent::Visible);
    }
    assert!(value.validate().is_err());
    let temp = crate::test_directory();
    let path = temp.path().join("s.json");
    let mut first = Settings::new(Some(path.clone()));
    let mut second = Settings::new(Some(path.clone()));
    first.save(Preferences::default()).unwrap();
    assert!(second.save(Preferences::default()).is_err());
    tamper(&path,&vec![0;1_048_577]);
    assert!(Settings::new(Some(path)).error.is_some());
    assert!(Settings::new(Some(temp.path().into())).error.is_some());
    let mut memory = Settings::new(None);
    memory.save(Preferences::default()).unwrap();
}

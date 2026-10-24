    use super::*;
    #[test]
    fn secrets_are_hashed_and_corrupt_configuration_is_preserved() {
        let root = crate::test_directory();
        let path = root.path().join("mcp.json");
        let mut store = Store::open(Some(path.clone()));
        assert!(!store.settings.enabled);
        let mut next = store.settings.clone();
        next.credentials.push(Credential {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Assistant".into(),
            scope: "read".into(),
            created_at: 1,
            hash: digest("secret-token"),
        });
        store.save(next).unwrap();
        let database=osg_persistence::Database::for_namespace(&path).unwrap();
        assert!(!std::fs::read(database.path()).unwrap().windows(b"secret-token".len()).any(|value|value==b"secret-token"));
        assert_eq!(store.revision,1);
        assert!(matches("secret-token", &store.settings.credentials[0].hash));
        assert!(!matches("different", &store.settings.credentials[0].hash));
        assert_eq!(
            Store::open(Some(path.clone())).settings.credentials.len(),
            1
        );
        crate::persistence::test_support::record(&path,"security","mcp",b"invalid");
        let mut damaged = Store::open(Some(path.clone()));
        assert!(damaged.error.is_some());
        assert!(damaged.save(Settings::default()).is_err());
        assert_eq!(database.read("security","mcp").unwrap().unwrap().payload,b"invalid");
    }
    #[test]
    fn rejects_duplicate_credentials_and_invalid_metadata() {
        let mut store = Store::open(None);
        let credential = Credential {
            id: uuid::Uuid::new_v4().to_string(),
            name: "x".into(),
            scope: "write".into(),
            created_at: 0,
            hash: digest("x"),
        };
        let mut next = store.settings.clone();
        next.credentials = vec![credential.clone(), credential];
        assert!(store.save(next).is_err());
        assert!(store.settings.credentials.is_empty());
    }

#[test]
fn sqlite_revisions_reject_stale_metadata_and_preserve_settings_after_writer_failure(){
 let root=crate::test_directory();let path=root.path().join("mcp-v1.json");std::fs::write(&path,b"retired invalid file").unwrap();
 let mut first=Store::open(Some(path.clone()));let mut stale=Store::open(Some(path.clone()));assert!(first.error.is_none());
 let mut next=first.settings.clone();next.port=55440;first.save(next.clone()).unwrap();assert_eq!(first.revision,1);
 assert!(stale.save(Settings::default()).unwrap_err().contains("conflict"));assert_eq!(stale.settings.port,DEFAULT_PORT);stale.refresh().unwrap();assert_eq!(stale.revision,1);assert_eq!(stale.settings.port,55440);assert!(stale.check_revision(0).is_err());
 let db=osg_persistence::Database::for_namespace(&path).unwrap();let lock=osg_persistence::test_support::hold_writer(&db).unwrap();next.enabled=true;assert!(first.save(next).unwrap_err().contains("busy"));drop(lock);
 assert!(!first.settings.enabled);assert_eq!(first.revision,1);assert!(!Store::open(Some(path.clone())).settings.enabled);assert_eq!(std::fs::read(path).unwrap(),b"retired invalid file");
}
#[test]
fn corrupt_sqlite_settings_require_a_valid_read_before_any_new_write(){
 let root=crate::test_directory();let path=root.path().join("mcp-v1.json");let mut store=Store::open(Some(path.clone()));
 crate::persistence::test_support::record(&path,"security","mcp",b"invalid");assert!(store.refresh().is_err());assert!(store.save(Settings::default()).is_err());
 let valid=serde_json::to_vec(&Settings::default()).unwrap();crate::persistence::test_support::record(&path,"security","mcp",&valid);store.refresh().unwrap();assert_eq!(store.revision,2);assert!(store.error.is_none());
 let mut next=store.settings.clone();next.port=55441;store.save(next).unwrap();assert_eq!(store.revision,3);
}

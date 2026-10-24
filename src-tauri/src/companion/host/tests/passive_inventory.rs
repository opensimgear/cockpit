use super::*;
#[test]
fn passive_inventory_never_creates_a_first_time_namespace_host_or_tls_service(){
    let root=crate::test_directory();let path=root.path().join("absent");
    for path in [None,Some(path.clone())] {
        let runtime=Runtime::new(path.clone(),Arc::new(CompanionProvider::new(Arc::new(||1))));
        let status=call(&runtime,"companion_inventory",json!({})).unwrap();assert_eq!(status["host"]["clients"],json!([]));assert_eq!(status["listener"]["settingsRevision"],0);assert_eq!(status["listener"]["enabled"],false);
        assert!(runtime.metadata.get().is_none());assert!(runtime.services.get().is_none());assert!(path.is_none_or(|path|!path.exists()));runtime.shutdown();assert!(call(&runtime,"companion_inventory",json!({})).is_err());
    }
    let retired=root.path().join("companion-host-state.dpapi");std::fs::write(&retired,b"retired invalid trust").unwrap();
    let runtime=Runtime::new(Some(root.path().into()),Arc::new(CompanionProvider::new(Arc::new(||1))));assert!(call(&runtime,"companion_inventory",json!({})).is_ok());assert!(!root.path().join("cockpit.sqlite3").exists());assert_eq!(std::fs::read(retired).unwrap(),b"retired invalid trust");
}
#[test]
fn passive_inventory_preserves_unavailable_or_orphaned_sql_metadata_as_an_error(){
    for (domain,key,payload) in [("secrets","companion-host",b"bad cipher".as_slice()),("companion","publications",b"orphan archive".as_slice())] {
        let root=crate::test_directory();let anchor=root.path().join("companion-host-state.dpapi");crate::persistence::test_support::record(&anchor,domain,key,payload);
        let runtime=Runtime::new(Some(root.path().into()),Arc::new(CompanionProvider::new(Arc::new(||1))));assert!(call(&runtime,"companion_inventory",json!({})).is_err());assert!(runtime.profile_authoring_catalogue().is_err());assert!(runtime.saved_panel_updates().is_err());assert!(runtime.resource_instance_ids().is_err());assert!(runtime.services.get().is_none());assert!(runtime.metadata.get().is_none());
        let db=osg_persistence::Database::for_namespace(&anchor).unwrap();assert_eq!(db.read(domain,key).unwrap().unwrap().payload,payload);assert!(db.read("secrets","companion-identity").unwrap().is_none());
    }
}
#[test]
fn passive_cached_metadata_does_not_create_a_directory_and_retains_tls_failure_diagnostics(){
    let root=crate::test_directory();let directory=root.path().join("absent");let runtime=Runtime::new(None,Arc::new(CompanionProvider::new(Arc::new(||1))));
    let hub=Hub::new("passive-test".into(),Arc::new(osg_companion_service::NullSourceSink));assert!(runtime.metadata.set(Metadata{hub,directory:directory.clone()}).is_ok());assert!(runtime.services.set(Err("TLS key unavailable".into())).is_ok());
    let status=call(&runtime,"companion_inventory",json!({})).unwrap();assert_eq!(status["host"]["hostId"],"passive-test");assert_eq!(status["listener"]["lastError"],"TLS key unavailable");assert!(!directory.exists());
}

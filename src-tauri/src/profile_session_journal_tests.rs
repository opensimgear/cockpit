use super::*;
#[test]
fn repeated_stopped_receipt_is_idempotent_but_foreign_revisions_still_fail(){
 let directory=crate::test_directory();let path=directory.path().join("profile-session-v1.json");
 let (mut store,manifest,error)=Store::open(Some(path.clone()));assert!(error.is_none());store.save(&manifest).unwrap();
 let database=osg_persistence::Database::for_namespace(&path).unwrap();let before=database.recovery_fingerprint().unwrap();store.save(&manifest).unwrap();assert_eq!(database.recovery_fingerprint().unwrap(),before);
 let bytes=osg_persistence::test_support::read_evidence(&path,"recovery-profile").unwrap();osg_persistence::test_support::write_evidence(&path,"recovery-profile",Some(&bytes)).unwrap();assert!(store.save(&manifest).is_err());
}

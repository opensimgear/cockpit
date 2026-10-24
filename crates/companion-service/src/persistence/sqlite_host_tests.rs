use super::*;
use serde_json::json;
struct Sealer;
impl NativeSealer for Sealer {
    fn available(&self)->Result<(),String>{Ok(())}
    fn seal(&self,bytes:&[u8])->Result<Vec<u8>,String>{
        let mut nonce=[0;12];getrandom::fill(&mut nonce).map_err(|_|"Randomness unavailable.")?;
        crate::persistence::encrypt_with_key(bytes,&[9;32],nonce)
    }
    fn unseal(&self,bytes:&[u8])->Result<Vec<u8>,String>{crate::persistence::decrypt_with_key(bytes,&[9;32])}
}
fn store(directory:&PathBuf)->SealedSqliteStore{SealedSqliteStore::with_sealer(directory.join("companion-host-state.dpapi"),Arc::new(Sealer))}
fn hash(letter:&str)->String{format!("sha256:{}",letter.repeat(64))}
fn content(hashes:&[String])->Vec<u8>{serde_json::to_vec(&json!({"publications":hashes.iter().map(|hash|json!({"publicationHash":hash})).collect::<Vec<_>>(),"savedHeads":{}})).unwrap()}
#[test]
fn host_snapshot_reopens_atomic_ciphertext_content_and_rejects_stale_or_failed_commits(){
    let dir=tempfile::tempdir().unwrap();let root=dir.path().to_path_buf();let first=store(&root);assert!(first.host_snapshot_store().is_some());assert!(first.read_host_snapshot().unwrap().state.is_none());
    let archive=content(&[hash("a")]);first.write_host_snapshot(b"first encrypted assignments",&archive).unwrap();let stale=store(&root);let loaded=stale.read_host_snapshot().unwrap();assert_eq!(loaded.state.unwrap().as_slice(),b"first encrypted assignments");assert_eq!(loaded.content.unwrap(),archive);
    let newer=content(&[hash("a"),hash("b")]);first.write_host_snapshot(b"newer encrypted assignments",&newer).unwrap();assert!(stale.write_host_snapshot(b"stale assignments",&archive).unwrap_err().contains("revision conflict"));
    let database=first.database(false).unwrap().unwrap();let lock=osg_persistence::test_support::hold_writer(database).unwrap();assert!(first.write_host_snapshot(b"uncommitted assignments",&archive).is_err());drop(lock);
    let snapshot=store(&root).read_host_snapshot().unwrap();assert_eq!(snapshot.state.unwrap().as_slice(),b"newer encrypted assignments");assert_eq!(snapshot.content.unwrap(),newer);
    assert_eq!(database.revision("secrets","companion-host").unwrap(),2);assert_eq!(database.revision("companion","publications").unwrap(),2);
    let bytes=std::fs::read(database.path()).unwrap();assert!(!bytes.windows(b"encrypted assignments".len()).any(|bytes|bytes==b"encrypted assignments"));
    assert!(!root.join("companion-host-state.dpapi").exists());
}
#[test]
fn pruning_consults_current_sql_profile_references_and_never_rewrites_the_profile_revision(){
    let dir=tempfile::tempdir().unwrap();let owner=store(&dir.path().to_path_buf());let a=hash("a");let b=hash("b");owner.write_host_snapshot(b"authority",&content(&[a.clone(),b.clone()])).unwrap();
    let db=owner.database(false).unwrap().unwrap();let profile=serde_json::to_vec(&json!({"profiles":[{"id":"00000000-0000-4000-8000-000000000001","name":"Retained reference","revision":1,"companionAssignments":[{"resourceId":"panel","panelId":"stable-panel","publicationHash":a}]}],"localAssignments":{}})).unwrap();
    db.commit(&[Change{domain:"profiles",key:"library",expected_revision:0,payload:Some(&profile)}]).unwrap();
    assert!(owner.write_host_snapshot(b"must not commit",&content(&[b.clone()])).unwrap_err().contains("still reference"));assert_eq!(db.revision("secrets","companion-host").unwrap(),1);
    db.commit(&[Change{domain:"profiles",key:"library",expected_revision:1,payload:Some(b"invalid profile references")}]).unwrap();
    assert!(owner.write_host_snapshot(b"must not commit",&content(&[b.clone()])).unwrap_err().contains("references are invalid"));
    db.commit(&[Change{domain:"profiles",key:"library",expected_revision:2,payload:Some(b"{\"profiles\":[],\"localAssignments\":{}}")}]).unwrap();
    owner.write_host_snapshot(b"safe removal",&content(&[b])).unwrap();assert_eq!(db.revision("profiles","library").unwrap(),3);assert_eq!(db.revision("companion","publications").unwrap(),2);
}
#[test]
fn corrupt_host_ciphertext_and_orphaned_content_are_preserved_without_creating_replacement_authority(){
    let dir=tempfile::tempdir().unwrap();let owner=store(&dir.path().to_path_buf());let db=Database::for_namespace(&dir.path().join("companion-host-state.dpapi")).unwrap();
    db.commit(&[Change{domain:"companion",key:"publications",expected_revision:0,payload:Some(&content(&[hash("a")]))}]).unwrap();assert!(owner.read_host_snapshot().err().unwrap().contains("no protected host"));
    db.commit(&[Change{domain:"secrets",key:"companion-host",expected_revision:0,payload:Some(b"invalid cipher")}]).unwrap();assert!(owner.read_host_snapshot().is_err());assert!(owner.write_host_snapshot(b"replacement",&content(&[])).is_err());
    assert_eq!(db.read("secrets","companion-host").unwrap().unwrap().payload,b"invalid cipher");assert_eq!(db.revision("companion","publications").unwrap(),1);
    assert!(owner.write_host_snapshot(&[],&content(&[])).is_err());assert!(owner.write_host_snapshot(b"state",b"invalid archive").is_err());assert!(owner.write_host_snapshot(b"state",b"{}").is_err());
    assert!(owner.write_host_snapshot(b"state",&content(&["wrong".into()])).is_err());assert!(owner.write_host_snapshot(b"state",&vec![b' ';CONTENT_BYTES+1]).is_err());
    assert!(owner.write_host_snapshot(&vec![0;STATE_BYTES+1],&content(&[])).is_err());
}

#[test]
fn routine_assignment_and_credential_edits_guard_but_never_rewrite_the_unchanged_archive(){
    let dir=tempfile::tempdir().unwrap();let owner=store(&dir.path().to_path_buf());assert!(owner.write_host_record(b"unloaded").is_err());let archive=content(&[hash("a")]);owner.write_host_snapshot(b"first state",&archive).unwrap();
    let db=owner.database(false).unwrap().unwrap();let before=db.read("companion","publications").unwrap().unwrap();owner.write_host_record(b"new assignments").unwrap();assert_eq!(db.read("companion","publications").unwrap().unwrap(),before);assert_eq!(db.revision("secrets","companion-host").unwrap(),2);
    let reopened=store(&dir.path().to_path_buf());assert_eq!(reopened.read_host_snapshot().unwrap().state.unwrap().as_slice(),b"new assignments");
    // An independently changed archive invalidates the state edit even while
    // its protected-record revision remains unchanged.
    db.commit(&[Change{domain:"companion",key:"publications",expected_revision:1,payload:Some(&content(&[hash("b")]))}]).unwrap();assert!(owner.write_host_record(b"stale reference").is_err());assert_eq!(db.revision("secrets","companion-host").unwrap(),2);
    assert!(owner.write_host_record(&[]).is_err());assert!(owner.write_host_record(&vec![0;STATE_BYTES+1]).is_err());
}

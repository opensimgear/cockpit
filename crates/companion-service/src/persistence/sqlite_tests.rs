use super::*;
fn encrypt(bytes:&[u8])->Result<Vec<u8>,String>{let mut nonce=[0;12];getrandom::fill(&mut nonce).map_err(|_|"Test randomness unavailable.")?;super::super::encrypt_with_key(bytes,&[7;32],nonce)}
fn decrypt(bytes:&[u8])->Result<Vec<u8>,String>{super::super::decrypt_with_key(bytes,&[7;32])}
struct TestSealer;
impl NativeSealer for TestSealer {
    fn available(&self)->Result<(),String>{Ok(())}
    fn seal(&self,plain:&[u8])->Result<Vec<u8>,String>{encrypt(plain)}
    fn unseal(&self,cipher:&[u8])->Result<Vec<u8>,String>{decrypt(cipher)}
}
fn store(anchor:impl Into<PathBuf>)->SealedSqliteStore {SealedSqliteStore::with_sealer(anchor,Arc::new(TestSealer))}
#[test]
fn passive_state_discovery_ignores_retired_files_and_never_creates_a_namespace(){
    let dir=tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();let anchor=dir.path().join("missing/state");let s=store(anchor);
    assert!(!s.exists().unwrap());assert!(s.read().unwrap().is_none());s.delete().unwrap();assert!(!dir.path().join("missing").exists());
    let anchor=dir.path().join("companion-host-state.dpapi");std::fs::write(&anchor,b"retired invalid file").unwrap();let s=store(anchor.clone());
    assert!(s.read().unwrap().is_none());s.write(b"current encrypted authority").unwrap();assert_eq!(s.read().unwrap().unwrap(),b"current encrypted authority");
    assert_eq!(std::fs::read(anchor).unwrap(),b"retired invalid file");assert_eq!(s.key().unwrap(),"companion-host");
    assert_eq!(store(dir.path().join("companion-identity.dpapi")).key().unwrap(),"companion-identity");
    assert_eq!(store(dir.path().join("companion-client.dpapi")).key().unwrap(),"companion-client");
}
#[test]
fn ciphertext_only_sqlite_and_backup_reopen_preserve_native_plaintext_boundary(){
    let dir=tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();let anchor=dir.path().join("state");let s=store(anchor.clone());let private=b"installation-private-token-and-ca-private-key";
    s.write(private).unwrap();assert!(s.exists().unwrap());let database=s.database(false).unwrap().unwrap();let record=database.read("secrets","state").unwrap().unwrap();
    assert!(!record.payload.windows(private.len()).any(|w|w==private));assert_eq!(store(anchor).read().unwrap().unwrap(),private);
    let backup=dir.path().join("online-backup.sqlite3");database.backup(&backup).unwrap();
    for file in [database.path(),backup.as_path()] {let bytes=std::fs::read(file).unwrap();assert!(!bytes.windows(private.len()).any(|w|w==private));}
    assert_eq!(Database::open(backup).unwrap().read("secrets","state").unwrap().unwrap().payload,record.payload);
}
#[test]
fn stale_secret_writers_and_forget_cannot_replace_newer_credentials(){
    let dir=tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();let anchor=dir.path().join("state");let first=store(anchor.clone());first.write(b"first").unwrap();let stale=store(anchor.clone());assert_eq!(stale.read().unwrap().unwrap(),b"first");
    first.write(b"newer").unwrap();stale.available().unwrap();assert!(stale.exists().unwrap());assert!(stale.write(b"stale").unwrap_err().contains("conflict"));assert!(stale.delete().unwrap_err().contains("conflict"));
    assert_eq!(store(anchor.clone()).read().unwrap().unwrap(),b"newer");assert_eq!(stale.read().unwrap().unwrap(),b"newer");stale.delete().unwrap();assert!(!stale.exists().unwrap());
    let reopened=store(anchor);reopened.write(b"new installation").unwrap();assert!(stale.write(b"old authority").is_err());
}
#[test]
fn failed_commits_and_corrupt_records_never_advance_or_overwrite_authority(){
    let dir=tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();let anchor=dir.path().join("state");let s=store(anchor.clone());s.write(b"first").unwrap();let db=s.database(false).unwrap().unwrap();
    let lock=osg_persistence::test_support::hold_writer(db).unwrap();assert!(s.write(b"failed").unwrap_err().contains("busy"));assert!(s.delete().is_err());drop(lock);
    assert_eq!(store(anchor.clone()).read().unwrap().unwrap(),b"first");s.write(b"second").unwrap();
    db.commit(&[Change{domain:"secrets",key:"state",expected_revision:2,payload:Some(b"corrupt ciphertext")}]).unwrap();
    let rejected=store(anchor.clone());assert!(rejected.read().is_err());assert!(rejected.write(b"must not replace corrupt evidence").is_err());
    assert_eq!(db.read("secrets","state").unwrap().unwrap().payload,b"corrupt ciphertext");
    rejected.delete().unwrap();assert!(rejected.read().unwrap().is_none());assert!(s.write(b"stale").is_err());
    assert!(store(dir.path()).read().is_err());assert!(store(PathBuf::from("relative")).read().is_err());
    let s=store(dir.path().join("empty"));assert!(s.write(&[]).is_err());assert!(s.write(&vec![0;STATE_BYTES+1]).is_err());
}
#[cfg(windows)]
#[test]
fn real_dpapi_sqlite_reopen_never_persists_private_bytes(){
    let dir=tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();let anchor=dir.path().join("companion-client.dpapi");let s=SealedSqliteStore::new(anchor.clone());let private=b"real DPAPI protected installation token";
    s.available().unwrap();s.write(private).unwrap();assert_eq!(SealedSqliteStore::new(anchor.clone()).read().unwrap().unwrap(),private);
    assert!(!std::fs::read(s.database(false).unwrap().unwrap().path()).unwrap().windows(private.len()).any(|w|w==private));assert!(!anchor.exists());
    s.delete().unwrap();assert!(s.read().unwrap().is_none());
}

#[test]
fn missing_or_locked_keys_never_call_a_key_creating_probe_or_seal_over_existing_state(){
    use std::sync::atomic::{AtomicBool,AtomicUsize,Ordering};
    struct Guarded {locked:AtomicBool,creations:AtomicUsize}
    impl NativeSealer for Guarded {
        fn available(&self)->Result<(),String>{self.creations.fetch_add(1,Ordering::SeqCst);Ok(())}
        fn seal(&self,bytes:&[u8])->Result<Vec<u8>,String>{self.creations.fetch_add(1,Ordering::SeqCst);encrypt(bytes)}
        fn unseal(&self,bytes:&[u8])->Result<Vec<u8>,String>{if self.locked.load(Ordering::SeqCst){Err("OS protection key unavailable".into())}else{decrypt(bytes)}}
    }
    let dir=tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();let sealer=Arc::new(Guarded{locked:AtomicBool::new(false),creations:AtomicUsize::new(0)});
    let store=SealedSqliteStore::with_sealer(dir.path().join("state"),sealer.clone());store.write(b"saved identity").unwrap();let attempts=sealer.creations.load(Ordering::SeqCst);
    sealer.locked.store(true,Ordering::SeqCst);assert!(store.available().is_err());assert!(store.write(b"must not overwrite identity").is_err());
    assert_eq!(sealer.creations.load(Ordering::SeqCst),attempts);sealer.locked.store(false,Ordering::SeqCst);assert_eq!(store.read().unwrap().unwrap(),b"saved identity");
}

use super::*;

#[test]
fn delayed_invalidation_cannot_clear_a_newer_operation_or_regress_its_epoch() {
    let directory=tempfile::tempdir().unwrap();
    let client=ClientRuntime::open(directory.path().to_owned()).unwrap();
    client.shared.epoch.store(2,Ordering::SeqCst);
    {
        let mut state=client.shared.state.lock().unwrap();
        state.status.epoch=2;state.status.phase="connecting".into();
        state.status.session=Some(json!({"sessionId":"new-session"}));
    }
    // The old invalidation advanced its epoch before the newer operation took
    // State. Its delayed projection must not clear the new session afterward.
    publish_invalidation(&client.shared,1,"old lifecycle boundary");
    let status=client.status();
    assert_eq!(status.epoch,2);assert_eq!(status.phase,"connecting");
    assert_eq!(status.session.unwrap()["sessionId"],"new-session");
    publish_invalidation(&client.shared,2,"current lifecycle boundary");
    assert_eq!(client.status().epoch,2);assert!(client.status().session.is_none());
}

#[test]
fn epoch_exhaustion_rejects_new_work_and_still_retires_existing_io() {
    let directory=tempfile::tempdir().unwrap();
    let client=ClientRuntime::open(directory.path().to_owned()).unwrap();
    client.shared.epoch.store(u64::MAX,Ordering::SeqCst);
    let old=io_scope(&client.shared,u64::MAX).unwrap();
    assert!(client.start_worker("connecting",None,|_,_|panic!("Exhausted work must never start")).is_err());
    assert!(!client.status().busy);
    client.invalidate("exhausted lifecycle boundary");
    assert!(old.is_closed());assert_eq!(client.shared.epoch.load(Ordering::SeqCst),u64::MAX);
    assert_eq!(client.status().epoch,u64::MAX);
    assert!(io_scope(&client.shared,u64::MAX).is_err());
}

#[derive(Default)]
struct Memory;
impl StateStorage for Memory {
    fn read(&self)->Result<Option<Vec<u8>>,String>{Ok(None)}
    fn write(&self,_:&[u8])->Result<(),String>{Ok(())}
    fn delete(&self)->Result<(),String>{Ok(())}
}

#[test]
fn local_network_denial_parks_new_work_and_revocation_retires_existing_io() {
    let directory = tempfile::tempdir().unwrap();
    let client = ClientRuntime::with_storage_network_access(
        directory.path().to_owned(),
        Arc::new(Memory),
        false,
    ).unwrap();

    for command in ["discover", "pair-start", "pair-code-start", "connect", "refresh-assignments"] {
        let error = client.command(json!({"type":command})).unwrap_err();
        assert!(error.contains("Local Network"), "{command}: {error}");
    }
    assert!(client.command(json!({"type":"status"})).is_ok());
    assert!(client.command(json!({"type":"forget"})).is_ok());
    assert!(io_scope(&client.shared, client.status().epoch).is_err());

    let wake = client.shared.presence_wake.observed();
    client.set_network_access(true);
    assert_ne!(client.shared.presence_wake.observed(), wake);
    let scope = io_scope(&client.shared, client.status().epoch).unwrap();
    client.set_network_access(false);
    assert!(scope.is_closed());
    assert!(io_scope(&client.shared, client.status().epoch).is_err());

    let epoch = client.status().epoch;
    client.set_network_access(false);
    assert_eq!(client.status().epoch, epoch);
    client.set_network_access(true);
    assert!(scope.is_closed(), "Grant must not reuse revoked I/O");
    assert!(io_scope(&client.shared, client.status().epoch).is_ok());
}

#[test]
fn a_credential_selection_captured_before_forget_cannot_start_later() {
    let directory=tempfile::tempdir().unwrap();
    let client=ClientRuntime::with_storage(directory.path().to_owned(),Arc::new(Memory)).unwrap();
    let selected=client.status().epoch;
    client.command(json!({"type":"forget"})).unwrap();
    assert!(client.start_worker("connecting",Some(selected),|_,_|panic!("Stale selection reached networking")).is_err());
    assert!(!client.status().busy);assert_eq!(client.status().phase,"unpaired");
}

#[test]
fn a_connection_reserved_during_forget_is_superseded_at_delete_commit() {
    struct DeleteGate {
        entered:std::sync::mpsc::Sender<()>,
        release:Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl StateStorage for DeleteGate {
        fn read(&self)->Result<Option<Vec<u8>>,String>{Ok(None)}
        fn write(&self,_:&[u8])->Result<(),String>{Ok(())}
        fn delete(&self)->Result<(),String>{self.entered.send(()).unwrap();self.release.lock().unwrap().recv().unwrap();Ok(())}
    }
    let directory=tempfile::tempdir().unwrap();
    let (entered,ready)=std::sync::mpsc::channel();let(release,blocked)=std::sync::mpsc::channel();
    let client=Arc::new(ClientRuntime::with_storage(directory.path().to_owned(),Arc::new(DeleteGate{entered,release:Mutex::new(blocked)})).unwrap());
    let deleting=client.clone();
    let forget=thread::spawn(move||deleting.command(json!({"type":"forget"})));
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let before=client.shared.epoch.load(Ordering::SeqCst);
    let selecting=client.clone();
    let start=thread::spawn(move||selecting.start_worker("pairing",None,|_,_|panic!("Superseded reservation reached networking")));
    let deadline=Instant::now()+Duration::from_secs(2);
    while client.shared.epoch.load(Ordering::SeqCst)==before {
        assert!(Instant::now()<deadline);thread::yield_now();
    }
    release.send(()).unwrap();
    forget.join().unwrap().unwrap();assert!(start.join().unwrap().is_err());
    let status=client.status();assert!(!status.busy);assert_eq!(status.phase,"unpaired");
    assert_eq!(status.epoch,client.shared.epoch.load(Ordering::SeqCst));
}

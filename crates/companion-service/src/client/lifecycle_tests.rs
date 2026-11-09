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
    assert!(client.start_worker("connecting",u64::MAX,&||true,|_,_|panic!("Exhausted work must never start")).is_err());
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
    assert!(client.set_network_access_while_current(true, &|| true));
    assert_ne!(client.shared.presence_wake.observed(), wake);
    let scope = io_scope(&client.shared, client.status().epoch).unwrap();
    assert!(!client.set_network_access_while_current(false, &|| {
        assert!(client.shared.network_access_update.try_lock().is_err());
        false
    }));
    assert!(!scope.is_closed(), "A stale permission result must leave current access live");
    assert!(client.set_network_access_while_current(false, &|| true));
    assert!(scope.is_closed());
    assert!(io_scope(&client.shared, client.status().epoch).is_err());

    let epoch = client.status().epoch;
    assert!(client.set_network_access_while_current(false, &|| true));
    assert_eq!(client.status().epoch, epoch);
    assert!(client.set_network_access_while_current(true, &|| true));
    assert!(scope.is_closed(), "Grant must not reuse revoked I/O");
    assert!(io_scope(&client.shared, client.status().epoch).is_ok());
}

#[test]
fn a_credential_selection_captured_before_forget_cannot_start_later() {
    let directory=tempfile::tempdir().unwrap();
    let client=ClientRuntime::with_storage(directory.path().to_owned(),Arc::new(Memory)).unwrap();
    let selected=client.status().epoch;
    client.command(json!({"type":"forget"})).unwrap();
    assert!(client.start_worker("connecting",selected,&||true,|_,_|panic!("Stale selection reached networking")).is_err());
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
    let start=thread::spawn(move||selecting.start_worker("pairing",before,&||true,|_,_|panic!("Superseded reservation reached networking")));
    let deadline=Instant::now()+Duration::from_secs(2);
    while client.shared.epoch.load(Ordering::SeqCst)==before {
        assert!(Instant::now()<deadline);thread::yield_now();
    }
    release.send(()).unwrap();
    forget.join().unwrap().unwrap();assert!(start.join().unwrap().is_err());
    let status=client.status();assert!(!status.busy);assert_eq!(status.phase,"unpaired");
    assert_eq!(status.epoch,client.shared.epoch.load(Ordering::SeqCst));
}

fn pairing_code_request() -> Value {
    json!({"type":"pair-code-start","code":"ABCDEFGH","endpoint":"wss://127.0.0.1:1/companion","name":"Fixture"})
}

#[test]
fn a_shell_request_cannot_start_pairing_after_suspend_during_storage_check() {
    struct StoreGate {
        block: AtomicBool,
        entered: mpsc::Sender<()>,
        resume: Mutex<mpsc::Receiver<()>>,
    }
    impl StateStorage for StoreGate {
        fn available(&self) -> Result<(), String> {
            if self.block.load(Ordering::SeqCst) {
                self.entered.send(()).unwrap();
                self.resume.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
            }
            Ok(())
        }
        fn read(&self) -> Result<Option<Vec<u8>>, String> { Ok(None) }
        fn write(&self, _: &[u8]) -> Result<(), String> { panic!("Stale request wrote credentials") }
        fn delete(&self) -> Result<(), String> { Ok(()) }
    }
    let directory = tempfile::tempdir().unwrap();
    let (entered, ready) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let storage = Arc::new(StoreGate { block: AtomicBool::new(false), entered, resume: Mutex::new(resume) });
    let client = ClientRuntime::with_storage(directory.path().to_owned(), storage.clone()).unwrap();
    let shell_current = AtomicBool::new(true);
    storage.block.store(true, Ordering::SeqCst);
    thread::scope(|scope| {
        let worker = scope.spawn(|| client.command_while_current(pairing_code_request(), &|| shell_current.load(Ordering::SeqCst)));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        shell_current.store(false, Ordering::SeqCst);
        client.invalidate_active("Native application suspended");
        release.send(()).unwrap();
        assert!(worker.join().unwrap().unwrap_err().contains("activity changed"));
    });
    assert_eq!(client.status().epoch, 0);
    assert!(!client.status().busy);
}

#[test]
fn a_shell_request_checks_lifecycle_after_claiming_the_worker_slot() {
    let directory = tempfile::tempdir().unwrap();
    let client = ClientRuntime::with_storage(directory.path().to_owned(), Arc::new(Memory)).unwrap();
    let current = || !client.shared.running.load(Ordering::SeqCst);
    let error = client.command_while_current(pairing_code_request(), &current).unwrap_err();
    assert!(error.contains("activity changed"), "{error}");
    assert_eq!(client.status().epoch, 0);
    assert!(!client.status().busy);
}

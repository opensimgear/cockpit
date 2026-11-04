use super::*;
use crate::observations;
use std::{sync::{atomic::AtomicUsize,mpsc},time::{Duration,Instant}};

const KEY:&str="metadata endpoint exact/key";
const CLIENT:&str="metadata-reader";
#[derive(Default)]
struct Fake {
    snapshot:Mutex<UsbSnapshot>,
    wakes:Mutex<Vec<Weak<osg_streams::NativeWake>>>,
    requests:AtomicUsize,
    snapshots:AtomicUsize,
    panic:AtomicBool,
}
impl Endpoint for Fake {
    fn snapshot(&self)->UsbSnapshot {
        self.snapshots.fetch_add(1,Ordering::AcqRel);
        assert!(!self.panic.load(Ordering::Acquire),"injected cache panic");
        self.snapshot.lock().unwrap().clone()
    }
    fn watch_inputs(&self,wake:Arc<osg_streams::NativeWake>) {
        self.wakes.lock().unwrap().push(Arc::downgrade(&wake));
    }
    fn request(&self,_:UsbRequest)->Result<UsbSnapshot,String> {
        self.requests.fetch_add(1,Ordering::AcqRel);Err("unexpected I/O".into())
    }
    fn shutdown(&self) {}
}
impl Fake {
    fn commit(&self,revision:u32) {
        let mut snapshot=self.snapshot.lock().unwrap();
        snapshot.config.as_mut().unwrap()["revision"]=json!(revision);
        snapshot.config.as_mut().unwrap()["owner"]=json!(42);
        snapshot.config.as_mut().unwrap()["dirty"]=json!(true);
        snapshot.observed_at_ms=Some(u64::from(revision));
        drop(snapshot);
        for wake in self.wakes.lock().unwrap().iter().filter_map(Weak::upgrade) {wake.notify();}
    }
}
fn port()->PortInfo {
    PortInfo {path:"COM8".into(),serial_number:Some("00112233445566778899aabb".into()),
        vendor_id:None,product_id:None,manufacturer:None,friendly_name:None}
}
fn route(generation:u64)->UsbRouteToken {
    UsbRouteToken { module_id:"4f53474400112233445566778899aabb".into(),serial:"00112233445566778899aabb".into(),
        connection_generation:generation,boot:7,configuration_generation:3 }
}
fn fake(generation:u64)->Arc<Fake> {
    Arc::new(Fake {snapshot:Mutex::new(UsbSnapshot {connected:true,port:Some("COM8".into()),
        route:Some(route(generation)),config:Some(json!({"view":"active","config":{"name":"active body"},
            "revision":1,"revisionExhausted":false,"generation":3,"savedGeneration":3,"owner":null,
            "claimExpiresMs":0,"dirty":false,"pendingApply":false})),
        draft:Some(json!({"secret":"draft"})),claimed_by:Some("private-client".into()),
        actions:Some(json!({"secret":"actions"})),events:Some(json!({"secret":"events"})),
        observed_at_ms:Some(1),..Default::default()}),..Default::default()})
}
fn insert(fleet:&Fleet,key:&str,service:Arc<Fake>) {
    fleet.state.lock().unwrap().slots.insert(key.into(),Slot {port:port(),present:true,automatic:false,
        ambiguous:false,suppressed:false,cached:service.snapshot(),service:Some(service),retiring_service:None,
        busy:Arc::new(AtomicBool::new(false)),closing:Arc::new(AtomicBool::new(false))});
}
fn fixture()->(Arc<Fleet>,Arc<Fake>) {
    let service=fake(1);let create=service.clone();
    let fleet=Arc::new(Fleet::with_create(Arc::new(||panic!("observation enumerated ports")),false,
        Arc::new(move||{let _=&create;panic!("observation opened endpoint")})));
    insert(&fleet,KEY,service.clone());(fleet,service)
}
fn subscribe(fleet:&Arc<Fleet>,runtime:&observations::Runtime,key:&str,client:&str)
    ->Result<(String,mpsc::Receiver<observations::Delivery>),String> {
    let topic=metadata_observations::topic(key);let(tx,rx)=mpsc::channel();
    let token=fleet.with_metadata_observation(&topic,runtime.broker(),|wake|runtime.watch_reader_lifecycle(wake),||
        runtime.subscribe(client,"ui",&topic,move|delivery|tx.send(delivery).map_err(|e|e.to_string())))?;
    Ok((token,rx))
}
fn recv(runtime:&observations::Runtime,client:&str,rx:&mpsc::Receiver<observations::Delivery>)->observations::Delivery {
    let delivery=rx.recv_timeout(Duration::from_secs(2)).expect("metadata delivery timeout");
    runtime.acknowledge(client,&delivery.subscription_id,&delivery.delivery).unwrap();delivery
}
fn idle(fleet:&Fleet) {
    let probe=fleet.metadata_bridges.lock().unwrap().get(KEY).unwrap().idle_probe();
    probe.recv_timeout(Duration::from_secs(2)).expect("metadata worker parked");
}
fn retired(fleet:&Fleet) {
    let deadline=Instant::now()+Duration::from_secs(2);
    while !fleet.metadata_bridges.lock().unwrap().is_empty() {
        assert!(Instant::now()<deadline,"metadata worker failed to retire");std::thread::yield_now();
    }
}

#[test]
fn passive_shared_metadata_baseline_and_committed_changes_are_exact() {
    let(fleet,service)=fixture();let runtime=observations::Runtime::new();
    let(token,rx)=subscribe(&fleet,&runtime,KEY,CLIENT).unwrap();
    let baseline=recv(&runtime,CLIENT,&rx);let payload=baseline.frame.payload.unwrap();
    assert_eq!(baseline.frame.kind,"snapshot");
    let keys=payload.as_object().unwrap().keys().map(String::as_str).collect::<BTreeSet<_>>();
    assert_eq!(keys,BTreeSet::from(["key","connected","port","identity","route","config","observedAtMs","error"]));
    assert_eq!(payload["config"]["config"]["name"],"active body");
    assert!(!payload.to_string().contains("private-client"));
    let(second,rx2)=subscribe(&fleet,&runtime,KEY,"second-reader").unwrap();
    assert_eq!(recv(&runtime,"second-reader",&rx2).frame.payload,Some(payload));
    assert_eq!(fleet.metadata_bridges.lock().unwrap().len(),1);idle(&fleet);
    let reads=service.snapshots.load(Ordering::Acquire);
    assert_eq!(service.requests.load(Ordering::Acquire),0);
    service.commit(2);
    let update=recv(&runtime,CLIENT,&rx);assert_eq!(update.frame.kind,"snapshot");
    let update=update.frame.payload.unwrap();
    assert_eq!(update["config"]["revision"],2);assert_eq!(update["config"]["owner"],42);
    assert_eq!(update["config"]["dirty"],true);assert_eq!(update["config"]["config"]["name"],"active body");
    assert_eq!(recv(&runtime,"second-reader",&rx2).frame.payload,Some(update));
    idle(&fleet);assert!(service.snapshots.load(Ordering::Acquire)>reads);
    assert_eq!(service.requests.load(Ordering::Acquire),0);
    runtime.unsubscribe(CLIENT,&token).unwrap();runtime.unsubscribe("second-reader",&second).unwrap();
    retired(&fleet);fleet.shutdown();runtime.close();
}

#[test]
fn replacement_and_disconnect_replace_generations_and_fence_old_cache() {
    let(fleet,old)=fixture();let runtime=observations::Runtime::new();
    let(token,rx)=subscribe(&fleet,&runtime,KEY,CLIENT).unwrap();let _=recv(&runtime,CLIENT,&rx);idle(&fleet);
    let next=fake(2);insert(&fleet,KEY,next.clone());
    for wake in fleet.input_wakes.lock().unwrap().get(KEY).unwrap().iter().filter_map(Weak::upgrade) {
        next.watch_inputs(wake);
    }
    fleet.notify_input_wakes(KEY);
    assert_eq!(recv(&runtime,CLIENT,&rx).frame.kind,"loss");
    let current=recv(&runtime,CLIENT,&rx);assert_eq!(current.frame.payload.unwrap()["route"]["connectionGeneration"],2);idle(&fleet);
    old.commit(99);next.commit(3);
    assert_eq!(recv(&runtime,CLIENT,&rx).frame.payload.unwrap()["config"]["revision"],3);idle(&fleet);
    {let mut state=fleet.state.lock().unwrap();let slot=state.slots.get_mut(KEY).unwrap();
        slot.cached=offline(next.snapshot());slot.service=None;slot.present=false;}
    fleet.notify_input_wakes(KEY);
    assert_eq!(recv(&runtime,CLIENT,&rx).frame.kind,"loss");
    assert_eq!(recv(&runtime,CLIENT,&rx).frame.payload.unwrap()["connected"],false);
    runtime.unsubscribe(CLIENT,&token).unwrap();retired(&fleet);fleet.shutdown();runtime.close();
}

#[test]
fn admission_limits_unknown_topics_failure_and_shutdown_release_registration() {
    let(fleet,service)=fixture();let runtime=observations::Runtime::new();
    assert!(fleet.with_metadata_observation("usb.metadata.INVALID",runtime.broker(),|_|{},||Ok(())).is_err());
    assert!(subscribe(&fleet,&runtime,"unknown",CLIENT).is_err());
    let failed=fleet.with_metadata_observation(&metadata_observations::topic(KEY),runtime.broker(),|_|{},||Err::<(),_>("rejected".into()));
    assert!(failed.is_err());retired(&fleet);
    for index in 0..5 {insert(&fleet,&format!("key-{index}"),service.clone());}
    for index in 0..4 {
        let client=format!("reader-{index}");let(_,rx)=subscribe(&fleet,&runtime,&format!("key-{index}"),&client).unwrap();
        let _=recv(&runtime,&client,&rx);
    }
    assert!(subscribe(&fleet,&runtime,"key-4","fifth").is_err());
    fleet.shutdown();retired(&fleet);
    assert!(subscribe(&fleet,&runtime,KEY,CLIENT).is_err());
    assert_eq!(service.requests.load(Ordering::Acquire),0);runtime.close();
}

#[test]
fn cache_panic_retires_registration_and_worker() {
    let(fleet,service)=fixture();let runtime=observations::Runtime::new();
    let(_,rx)=subscribe(&fleet,&runtime,KEY,CLIENT).unwrap();let _=recv(&runtime,CLIENT,&rx);idle(&fleet);
    service.panic.store(true,Ordering::Release);fleet.notify_input_wakes(KEY);retired(&fleet);
    service.panic.store(false,Ordering::Release);
    let(token,rx)=subscribe(&fleet,&runtime,KEY,"replacement-reader").unwrap();
    let _=recv(&runtime,"replacement-reader",&rx);
    runtime.unsubscribe("replacement-reader",&token).unwrap();retired(&fleet);fleet.shutdown();runtime.close();
}

#[test]
fn subscriber_callback_panic_does_not_leak_idle_worker() {
    let(fleet,_)=fixture();let runtime=observations::Runtime::new();
    let failed=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
        fleet.with_metadata_observation(&metadata_observations::topic(KEY),runtime.broker(),|_|{},||->Result<(),String>{
            panic!("injected subscriber callback panic")
        })));
    assert!(failed.is_err());retired(&fleet);fleet.shutdown();runtime.close();
}

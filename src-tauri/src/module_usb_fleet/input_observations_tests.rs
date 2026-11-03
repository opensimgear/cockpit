use super::*;
use crate::{module_console::actor::Input, module_usb::stream::Capture, observations};
use serde_json::json;
use std::{sync::{atomic::AtomicUsize,mpsc}, time::{Duration, Instant}};

const KEY: &str = "endpoint-fixture";
const CLIENT_A: &str = "usb-input-test-a";
const CLIENT_B: &str = "usb-input-test-b";

fn route(connection_generation: u64) -> UsbRouteToken {
    UsbRouteToken { module_id:"4f53474400112233445566778899aabb".into(),serial:"00112233445566778899aabb".into(),
        connection_generation,boot:7,configuration_generation:3 }
}
fn state(head:u32,sampled:u32)->Value {
    json!({"ok":true,"schemaVersion":1,"stream":3,"head":head,"oldest":1,"overflow":0,
        "buttonCount":1,"held":[head>0],"encoderButtons":[],"contactButtonCount":1,
        "maintainedButtons":[],"selectorButtons":[],"axisCount":1,"axes":[32767],
        "axisAvailable":[true],"axisUsage":[48],"axisNeutral":[32767],"sampledAtMs":sampled})
}
fn frame(capture:&Capture,sequence:u32,kind:&str,body:Value){
    let at=Instant::now();
    capture.accept(Input::Line{line:format!("@events {}",json!({"boot":7,"sequence":sequence,"kind":kind,"state":body})),received_at:at});
}
fn fixture()->(Arc<Fleet>,Arc<FakeInput>,Arc<Capture>){
    let capture=Capture::new(route(1)).unwrap();
    let fake=Arc::new(FakeInput{capture:Mutex::new(Some(capture.clone())),snapshot:Mutex::new(UsbSnapshot{
        connected:true,port:Some("COM8".into()),route:Some(route(1)),..Default::default()}),
        requests:AtomicUsize::new(0),wakes:Mutex::new(Vec::new())});
    let f=fake.clone();
    let fleet=Arc::new(Fleet::with_create(Arc::new(||Ok(Vec::new())),false,
        Arc::new(move||f.clone())));
    fleet.state.lock().unwrap().slots.insert(KEY.into(),Slot{port:PortInfo{path:"COM8".into(),serial_number:Some("00112233445566778899aabb".into()),
        vendor_id:None,product_id:None,manufacturer:None,friendly_name:None},present:true,automatic:false,ambiguous:false,
        suppressed:false,service:Some(fake.clone()),cached:fake.snapshot(),retiring_service:None,
        busy:Arc::new(AtomicBool::new(false)),closing:Arc::new(AtomicBool::new(false))});
    (fleet,fake,capture)
}
#[derive(Default)]
struct FakeInput { capture:Mutex<Option<Arc<Capture>>>,snapshot:Mutex<UsbSnapshot>,requests:AtomicUsize,
    wakes:Mutex<Vec<Weak<osg_streams::NativeWake>>> }
impl Endpoint for FakeInput {
    fn input_capture(&self)->Option<Arc<Capture>>{self.capture.lock().unwrap().clone()}
    fn watch_inputs(&self,wake:Arc<osg_streams::NativeWake>){self.wakes.lock().unwrap().push(Arc::downgrade(&wake));}
    fn snapshot(&self)->UsbSnapshot{self.snapshot.lock().unwrap().clone()}
    fn request(&self,_:UsbRequest)->Result<UsbSnapshot,String>{self.requests.fetch_add(1,Ordering::AcqRel);Err("unexpected command".into())}
    fn shutdown(&self){if let Some(capture)=self.capture.lock().unwrap().take(){capture.close();}}
}
fn subscribe(fleet:&Arc<Fleet>,runtime:&observations::Runtime,client:&'static str)->(String,mpsc::Receiver<observations::Delivery>){
    let topic=input_observations::topic(KEY);let(tx,rx)=mpsc::channel();let rt=runtime.clone();let client_owned=client.to_owned();
    let token=fleet.with_input_observation(&topic,runtime.broker(),|wake|runtime.watch_reader_lifecycle(wake),||
        rt.subscribe(&client_owned,"ui",&topic,move|delivery|tx.send(delivery).map_err(|e|e.to_string()))).unwrap();
    (token,rx)
}
fn recv(rx:&mpsc::Receiver<observations::Delivery>)->observations::Delivery{
    rx.recv_timeout(Duration::from_secs(2)).expect("USB observation delivery timeout")
}
fn ack(runtime:&observations::Runtime,client:&str,delivery:&observations::Delivery){
    runtime.acknowledge(client,&delivery.subscription_id,&delivery.delivery).unwrap();
}

#[test]
fn readers_share_passive_bridge_and_keep_native_capture_clock(){
    let(fleet,fake,capture)=fixture();let runtime=observations::Runtime::new();
    let(token_a,rx_a)=subscribe(&fleet,&runtime,CLIENT_A);
    let first=recv(&rx_a);assert_eq!(first.frame.kind,"snapshot");assert_eq!(first.frame.payload.as_ref().unwrap()["key"],KEY);
    assert_eq!(first.frame.payload.as_ref().unwrap()["state"],Value::Null);ack(&runtime,CLIENT_A,&first);
    let(token_b,rx_b)=subscribe(&fleet,&runtime,CLIENT_B);let second=recv(&rx_b);ack(&runtime,CLIENT_B,&second);
    assert_eq!(fleet.input_bridges.lock().unwrap().len(),1);
    let mut probe=capture.subscribe().unwrap();let _=probe.recv().unwrap();
    frame(&capture,1,"baseline",state(0,10));
    let baseline=recv(&rx_a);assert_eq!(baseline.frame.kind,"loss");ack(&runtime,CLIENT_A,&baseline);
    let _=probe.recv().unwrap();let source_baseline=match probe.recv().unwrap(){
        osg_streams::Delivery::Snapshot(snapshot)=>snapshot,
        other=>panic!("expected native capture baseline, got {other:?}"),
    };
    let baseline=ready(&runtime,CLIENT_A,&rx_a);assert_eq!(baseline.frame.kind,"snapshot");
    assert_eq!(baseline.frame.clock_id,source_baseline.clock_id);assert_eq!(baseline.frame.captured_us,source_baseline.captured_us.to_string());
    assert!(baseline.frame.payload.as_ref().unwrap()["state"].get("events").is_none());
    frame(&capture,2,"update",{let mut v=state(1,20);v["events"]=json!([{"sequence":1,"control":0,"pressed":true,"capturedAtMs":20}]);v["after"]=json!(0);v["next"]=json!(1);v["more"]=json!(false);v});
    let source_update=match probe.recv().unwrap(){
        osg_streams::Delivery::Event(event)=>event,
        other=>panic!("expected native capture update, got {other:?}"),
    };let update=recv(&rx_a);assert_eq!(update.frame.kind,"snapshot"); // UI absolute state coalesces.
    assert_eq!(update.frame.captured_us,source_update.captured_us.to_string());assert_eq!(update.frame.clock_id,source_update.clock_id);
    assert!(update.frame.payload.as_ref().unwrap()["state"].get("events").is_none());
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    runtime.unsubscribe(CLIENT_A,&token_a).unwrap();runtime.unsubscribe(CLIENT_B,&token_b).unwrap();
    let deadline=Instant::now()+Duration::from_secs(2);while !fleet.input_bridges.lock().unwrap().is_empty(){assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));}
    fleet.shutdown();runtime.close();
}

fn idle(fleet:&Arc<Fleet>){
    let probe=fleet.input_bridges.lock().unwrap().get(KEY).expect("live input bridge").idle_probe();
    probe.recv_timeout(Duration::from_secs(2)).expect("bridge did not reach its idle park boundary");
}
fn retired(fleet:&Arc<Fleet>){
    let deadline=Instant::now()+Duration::from_secs(2);
    while !fleet.input_bridges.lock().unwrap().is_empty(){
        assert!(Instant::now()<deadline,"last reader did not retire input bridge");
        std::thread::yield_now();
    }
}
fn ready(runtime:&observations::Runtime,client:&str,rx:&mpsc::Receiver<observations::Delivery>)->observations::Delivery{
    loop{
        let delivery=recv(rx);
        let matches=delivery.frame.payload.as_ref().is_some_and(|payload|payload["ready"]==true);
        ack(runtime,client,&delivery);
        if matches{return delivery;}
    }
}

#[test]
fn idle_source_notification_and_last_reader_retirement_work_after_park(){
    let(fleet,fake,capture)=fixture();let runtime=observations::Runtime::new();
    let(token,rx)=subscribe(&fleet,&runtime,CLIENT_A);
    let initial=recv(&rx);ack(&runtime,CLIENT_A,&initial);
    idle(&fleet);
    frame(&capture,1,"baseline",state(0,10));
    let current=ready(&runtime,CLIENT_A,&rx);
    assert_eq!(current.frame.kind,"snapshot");
    idle(&fleet);
    runtime.unsubscribe(CLIENT_A,&token).unwrap();
    retired(&fleet);
    // Retirement drops the native subscription and watch, without stopping
    // the service-owned capture or sending a device command.
    let until=Instant::now()+Duration::from_secs(2);
    let readers=loop{
        if let Ok(readers)=(0..8).map(|_|capture.subscribe()).collect::<Result<Vec<_>,_>>(){break readers;}
        assert!(Instant::now()<until,"retired bridge retained native leases");
        std::thread::yield_now();
    };
    assert!(capture.snapshot().ready);
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    drop(readers);fleet.shutdown();runtime.close();
}

#[test]
fn failed_second_admission_preserves_existing_reader(){
    let(fleet,fake,capture)=fixture();let runtime=observations::Runtime::new();
    let(token,rx)=subscribe(&fleet,&runtime,CLIENT_A);
    let initial=recv(&rx);ack(&runtime,CLIENT_A,&initial);idle(&fleet);
    let failed=fleet.with_input_observation(&input_observations::topic(KEY),runtime.broker(),
        |wake|runtime.watch_reader_lifecycle(wake),||Err::<(),_>("injected subscribe rejection".to_owned()));
    assert!(failed.is_err());
    assert_eq!(fleet.input_bridges.lock().unwrap().len(),1);
    frame(&capture,1,"baseline",state(0,10));
    assert_eq!(ready(&runtime,CLIENT_A,&rx).frame.payload.unwrap()["ready"],true);
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    runtime.unsubscribe(CLIENT_A,&token).unwrap();retired(&fleet);fleet.shutdown();runtime.close();
}

#[test]
fn physical_capture_replacement_fences_old_route_and_uses_new_clock(){
    let(fleet,fake,old)=fixture();let runtime=observations::Runtime::new();
    frame(&old,1,"baseline",state(0,10));
    let(token,rx)=subscribe(&fleet,&runtime,CLIENT_A);
    let first=recv(&rx);let old_clock=first.frame.clock_id.clone();ack(&runtime,CLIENT_A,&first);idle(&fleet);
    old.close();
    let lost=recv(&rx);assert_eq!(lost.frame.kind,"loss");ack(&runtime,CLIENT_A,&lost);
    let disconnected=recv(&rx);assert_eq!(disconnected.frame.kind,"snapshot");
    assert_eq!(disconnected.frame.payload.as_ref().unwrap()["ready"],false);
    ack(&runtime,CLIENT_A,&disconnected);idle(&fleet);
    let replacement=Capture::new(route(2)).unwrap();
    let mut native=replacement.subscribe().unwrap();let _=native.recv().unwrap();
    frame(&replacement,1,"baseline",state(0,10));let _=native.recv().unwrap();
    let osg_streams::Delivery::Snapshot(new_snapshot)=native.recv().unwrap()else{panic!("native replacement baseline")};
    *fake.capture.lock().unwrap()=Some(replacement.clone());
    fake.snapshot.lock().unwrap().route=Some(route(2));
    fleet.notify_input_wakes(KEY);
    let loss=recv(&rx);assert_eq!(loss.frame.kind,"loss");ack(&runtime,CLIENT_A,&loss);
    let current=recv(&rx);assert_eq!(current.frame.kind,"snapshot");
    assert_eq!(current.frame.payload.as_ref().unwrap()["route"]["connectionGeneration"],2);
    assert_eq!(current.frame.clock_id,new_snapshot.clock_id);
    assert_ne!(current.frame.clock_id,old_clock);
    assert_eq!(current.frame.captured_us,new_snapshot.captured_us.to_string());
    ack(&runtime,CLIENT_A,&current);idle(&fleet);
    // Old RX remains incapable of addressing the new binding.
    frame(&old,2,"update",{let mut v=state(0,20);v["events"]=json!([]);v["after"]=json!(0);v["next"]=json!(0);v["more"]=json!(false);v["axes"]=json!([11111]);v});
    frame(&replacement,2,"update",{let mut v=state(0,20);v["events"]=json!([]);v["after"]=json!(0);v["next"]=json!(0);v["more"]=json!(false);v["axes"]=json!([50000]);v});
    let update=recv(&rx);
    assert_eq!(update.frame.clock_id,new_snapshot.clock_id);
    assert_eq!(update.frame.payload.as_ref().unwrap()["route"]["connectionGeneration"],2);
    assert_eq!(update.frame.payload.as_ref().unwrap()["state"]["axes"],json!([50000]));
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    runtime.unsubscribe(CLIENT_A,&token).unwrap();retired(&fleet);drop(native);fleet.shutdown();runtime.close();
}

#[test]
fn unacknowledged_reader_does_not_delay_fast_latest_state_reader(){
    let(fleet,fake,capture)=fixture();let runtime=observations::Runtime::new();
    let(slow_token,slow)=subscribe(&fleet,&runtime,CLIENT_A);
    let slow_initial=recv(&slow); // Intentionally retain the delivery credit.
    let(fast_token,fast)=subscribe(&fleet,&runtime,CLIENT_B);
    let initial=recv(&fast);ack(&runtime,CLIENT_B,&initial);idle(&fleet);
    frame(&capture,1,"baseline",state(0,10));ready(&runtime,CLIENT_B,&fast);
    for index in 0..4{
        frame(&capture,index+2,"update",{let mut v=state(0,20+index);v["events"]=json!([]);v["after"]=json!(0);v["next"]=json!(0);v["more"]=json!(false);v["axes"]=json!([40000+index]);v});
        let delivery=recv(&fast);
        assert_eq!(delivery.frame.payload.as_ref().unwrap()["state"]["axes"],json!([40000+index]));
        ack(&runtime,CLIENT_B,&delivery);
    }
    ack(&runtime,CLIENT_A,&slow_initial);
    let latest=ready(&runtime,CLIENT_A,&slow);
    assert_eq!(latest.frame.payload.as_ref().unwrap()["state"]["axes"],json!([40003]));
    runtime.unsubscribe(CLIENT_B,&fast_token).unwrap();
    assert_eq!(fleet.input_bridges.lock().unwrap().len(),1);
    runtime.unsubscribe(CLIENT_A,&slow_token).unwrap();retired(&fleet);
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    fleet.shutdown();runtime.close();
}

#[test]
fn fleet_shutdown_wakes_and_joins_a_parked_input_bridge(){
    let(fleet,fake,_capture)=fixture();let runtime=observations::Runtime::new();
    let(_token,rx)=subscribe(&fleet,&runtime,CLIENT_A);
    let initial=recv(&rx);ack(&runtime,CLIENT_A,&initial);idle(&fleet);
    let(done,completed)=mpsc::channel();let closing=fleet.clone();
    let shutdown=std::thread::spawn(move||{closing.shutdown();let _=done.send(());});
    completed.recv_timeout(Duration::from_secs(2)).expect("parked bridge prevented fleet shutdown");
    shutdown.join().unwrap();
    assert!(fleet.input_bridges.lock().unwrap().is_empty());
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    runtime.close();
}

#[test]
fn stopped_fleet_cannot_recreate_a_bridge_for_a_retained_endpoint(){
    let(fleet,fake,_capture)=fixture();let runtime=observations::Runtime::new();
    fleet.shutdown();
    let admitted=AtomicBool::new(false);
    let result=fleet.with_input_observation(&input_observations::topic(KEY),runtime.broker(),
        |wake|runtime.watch_reader_lifecycle(wake),||{admitted.store(true,Ordering::Release);Ok(())});
    assert!(result.is_err());
    assert!(!admitted.load(Ordering::Acquire));
    assert!(fleet.input_bridges.lock().unwrap().is_empty());
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    runtime.close();
}

#[test]
fn offline_observer_receives_new_service_capture_before_any_metadata_request(){
    let(fleet,fake,_old)=fixture();let runtime=observations::Runtime::new();
    fake.capture.lock().unwrap().take();
    fleet.state.lock().unwrap().slots.get_mut(KEY).unwrap().service=None;
    let(token,rx)=subscribe(&fleet,&runtime,CLIENT_A);
    let initial=recv(&rx);ack(&runtime,CLIENT_A,&initial);idle(&fleet);
    let service=fleet.create_input_service(KEY);
    fleet.state.lock().unwrap().slots.get_mut(KEY).unwrap().service=Some(service);
    let capture=Capture::new(route(2)).unwrap();
    *fake.capture.lock().unwrap()=Some(capture.clone());
    frame(&capture,1,"baseline",state(0,10));
    // Only the new service's native capture-install notification fires.
    // Fleet completion and metadata requests have deliberately not run.
    for wake in fake.wakes.lock().unwrap().iter().filter_map(Weak::upgrade){wake.notify();}
    let current=ready(&runtime,CLIENT_A,&rx);
    assert_eq!(current.frame.payload.unwrap()["route"]["connectionGeneration"],2);
    assert_eq!(fake.requests.load(Ordering::Acquire),0);
    runtime.unsubscribe(CLIENT_A,&token).unwrap();retired(&fleet);fleet.shutdown();runtime.close();
}

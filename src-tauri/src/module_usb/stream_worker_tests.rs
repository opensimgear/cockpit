//! Real service handoff over the continuously blocking test transport.
use super::*;
use osg_streams::{Delivery, NativeSubscription};
type Reader=NativeSubscription<stream::Snapshot,stream::Batch>;

fn ready(capture:&stream::Capture)->Reader{
    let mut reader=capture.subscribe().unwrap();
    loop{match reader.recv().unwrap(){
        Delivery::Snapshot(snapshot) if snapshot.value.ready=>break,
        Delivery::Snapshot(_)|Delivery::Loss(_)=>{},
        _=>panic!("input event preceded its fresh baseline"),
    }}reader
}
fn edge(device:&Arc<Mutex<Device>>,sample:u32,pressed:bool){
    let (pipe,packet)={let mut device=device.lock().unwrap();
        let after=device.head;device.head+=1;device.stream_sequence+=1;
        let mut state=stream_io::state(&device,sample);
        state["held"]=json!([pressed]);state["after"]=json!(after);
        state["next"]=json!(device.head);state["more"]=json!(false);
        state["events"]=json!([{"sequence":device.head,"control":0,"pressed":pressed,"capturedAtMs":sample}]);
        (device.stream_pipe.clone().unwrap(),json!({"boot":device.boot,
            "sequence":device.stream_sequence,"kind":"update","state":state}))};
    pipe.send(format!("@events {packet}\r\n").as_bytes());
}
fn next(reader:&mut Reader)->stream::Batch{
    let Delivery::Event(event)=reader.recv().unwrap()else{panic!("ordered input update missing")};
    event.value.clone()
}

#[test]
fn passive_readers_share_one_capture_and_preserve_press_release_without_queries(){
    let (service,device)=setup();connect(&service);
    let capture=service.input_capture().unwrap();
    let mut first=ready(&capture);let mut second=ready(&capture);
    let writes=device.lock().unwrap().writes.clone();
    edge(&device,1,true);edge(&device,2,false);
    for reader in [&mut first,&mut second]{
        let down=next(reader);let up=next(reader);
        assert!(down.events[0].pressed);assert!(!up.events[0].pressed);
        assert_eq!(down.events[0].sequence+1,up.events[0].sequence);
        assert_eq!(down.events[0].captured_at_ms,1);
        assert!(down.events[0].received_at<=up.events[0].received_at);
    }
    assert_eq!(device.lock().unwrap().writes,writes);
    assert!(Arc::ptr_eq(&capture,&service.input_capture().unwrap()));
    drop(first);edge(&device,3,true);assert!(next(&mut second).events[0].pressed);
    assert_eq!(device.lock().unwrap().opens,1);
}

#[test]
fn input_arrives_while_configuration_reply_is_pending(){
    let (service,device)=setup();connect(&service);
    let service=Arc::new(service);let capture=service.input_capture().unwrap();
    let mut reader=ready(&capture);
    let pipe={let mut device=device.lock().unwrap();device.drop_command=Some("config get_active".into());device.stream_pipe.clone().unwrap()};
    let (tx,rx)=mpsc::channel();let worker={let service=service.clone();thread::spawn(move||tx.send(service.request(UsbRequest::Refresh)).unwrap())};
    pipe.wait_command("config get_active",2);
    edge(&device,1,true);
    assert!(next(&mut reader).events[0].pressed);
    assert!(matches!(rx.try_recv(),Err(mpsc::TryRecvError::Empty)));
    let config=device.lock().unwrap().snapshot(true);
    pipe.send(format!("OK\r\n{config}\r\n").as_bytes());
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap().connected);
    worker.join().unwrap();
}

#[test]
fn reconnect_retires_old_capture_and_old_receiver_cannot_publish_into_new_one(){
    let (service,device)=setup();let old_route=connect(&service).route.unwrap();
    let old=service.input_capture().unwrap();let mut old_reader=ready(&old);
    let old_pipe=device.lock().unwrap().stream_pipe.clone().unwrap();
    service.request(UsbRequest::Disconnect).unwrap();
    assert!(!old.snapshot().ready);
    let new_route=connect(&service).route.unwrap();assert_ne!(old_route,new_route);
    let new=service.input_capture().unwrap();let mut new_reader=ready(&new);
    assert!(!Arc::ptr_eq(&old,&new));
    old_pipe.send(b"@events malformed old source\r\n");
    edge(&device,1,true);
    assert!(next(&mut new_reader).events[0].pressed);
    assert!(!old.snapshot().ready);
    assert!(matches!(old_reader.recv().unwrap(),Delivery::Loss(_)));
}

#[test]
fn shutdown_interrupts_pending_reply_and_withdraws_capture(){
    let (service,device)=setup();connect(&service);
    let service=Arc::new(service);let capture=service.input_capture().unwrap();let _reader=ready(&capture);
    let pipe={let mut device=device.lock().unwrap();device.drop_command=Some("config get_active".into());device.stream_pipe.clone().unwrap()};
    let worker={let service=service.clone();thread::spawn(move||service.request(UsbRequest::Refresh))};
    pipe.wait_command("config get_active",2);
    service.shutdown();
    assert!(worker.join().unwrap().is_err());assert!(!capture.snapshot().ready);
    assert!(!service.snapshot().connected);
    assert_eq!(device.lock().unwrap().controls.last(),Some(&false));
}

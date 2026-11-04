use super::*;
use crate::transport::duplex::CancelIo;

fn until(mut condition:impl FnMut()->bool){
    let deadline=Instant::now()+Duration::from_secs(2);
    while !condition(){assert!(Instant::now()<deadline,"metadata worker state did not converge");thread::sleep(Duration::from_millis(5));}
}

fn change(device:&Arc<Mutex<Device>>,sequence:u32,revision:u32,owner:u32,dirty:bool){
    let (pipe,packet)={let device=device.lock().unwrap();
        (device.stream_pipe.clone().unwrap(),json!({"boot":device.boot,"sequence":sequence,
            "kind":"change","revision":revision,"exhausted":false,"state":{
                "generation":device.generation,"savedGeneration":device.saved,"owner":owner,
                "claimExpiresMs":if owner==0{0}else{30000},"dirty":dirty,"pendingApply":false}}))};
    pipe.send(format!("@meta {packet}\r\n").as_bytes());
}

fn external_expiry_and_same_owner_reclaim(device:&mut Device){
    device.metadata_revision=device.metadata_revision.checked_add(1).unwrap();
    device.draft["name"]=json!("External writer draft");
    device.dirty=true;
}

#[test]
fn connected_idle_worker_performs_no_periodic_status_or_configuration_reads(){
    let (service,device)=setup();connect(&service);
    let commands=device.lock().unwrap().writes.len();
    thread::sleep(Duration::from_millis(2200));
    let device=device.lock().unwrap();
    assert_eq!(device.writes.len(),commands);
    assert!(!device.writes[commands..].iter().any(|line|line=="can status"||line=="config get_active"));
    drop(device);service.shutdown();
}

#[test]
fn unsolicited_owner_expiry_withdraws_claim_and_draft_without_a_request(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    assert_eq!(service.snapshot().claimed_by.as_deref(),Some("editor-a"));
    let writes=device.lock().unwrap().writes.len();
    stream_io::emit_metadata(&device,|device|{device.owner=0;device.draft=device.active.clone();device.dirty=false;});
    until(||service.snapshot().claimed_by.is_none());
    assert!(service.snapshot().draft.is_none());
    assert_eq!(device.lock().unwrap().writes.len(),writes,"metadata was handled without a console query");
    service.shutdown();
}

#[test]
fn unsolicited_generation_change_retires_route_and_cached_routes(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();
    actions(&service,&route);
    service.request(UsbRequest::EventsState{route:route.clone()}).unwrap();
    stream_io::emit_metadata(&device,|device|{device.generation=2;device.saved=2;});
    until(||!service.snapshot().connected);
    assert!(service.snapshot().route.is_none());
    assert!(service.snapshot().events.is_none());
    let actions=service.snapshot().actions.unwrap();
    assert_eq!(actions["status"],"uncertain");assert_eq!(actions["states"],json!([]));
    service.shutdown();
}

#[test]
fn command_receipt_revision_keeps_a_legitimate_writer_despite_an_equal_revision_notice(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    let owner=device.lock().unwrap().owner;
    change(&device,2,2,owner,false);
    thread::sleep(Duration::from_millis(60));
    assert_eq!(service.snapshot().claimed_by.as_deref(),Some("editor-a"));
    service.shutdown();
}

#[test]
fn contradictory_state_at_a_command_receipt_revision_fails_closed(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    change(&device,2,2,0,false);
    until(||!service.snapshot().connected);
    assert!(service.snapshot().error.is_some());service.shutdown();
}

#[test]
fn same_owner_external_revision_withdraws_private_writer_and_rejects_save_without_a_write(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    let writes=device.lock().unwrap().writes.len();
    stream_io::emit_metadata(&device,|_|{});
    until(||service.snapshot().claimed_by.is_none());
    assert!(service.snapshot().draft.is_none());
    assert_eq!(device.lock().unwrap().writes.len(),writes);
    assert!(service.request(UsbRequest::ConfigChange{route,client_id:"editor-a".into(),change:ConfigChange::Save}).is_err());
    assert_eq!(device.lock().unwrap().writes.len(),writes,"revoked writer must not issue config save");
    service.shutdown();
}

#[test]
fn active_read_with_a_newer_unknown_revision_cannot_keep_same_owner_authority(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    device.lock().unwrap().metadata_revision+=1;
    service.request(UsbRequest::Refresh).unwrap();
    let snapshot=service.snapshot();assert!(snapshot.claimed_by.is_none());assert!(snapshot.draft.is_none());
    assert_eq!(snapshot.config.as_ref().unwrap()["revision"],3);
    assert!(device.lock().unwrap().wire_writes.iter().any(|line|line=="config get_active"),"read remains an unguarded command");
    let writes=device.lock().unwrap().wire_writes.len();
    assert!(service.request(UsbRequest::ConfigChange{route,client_id:"editor-a".into(),change:ConfigChange::Save}).is_err());
    assert_eq!(device.lock().unwrap().wire_writes.len(),writes,"stale private authority cannot write after read admission");
    service.shutdown();
}

#[test]
fn guarded_mutation_rejects_expiry_and_same_owner_reclaim_before_touching_draft(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    let owner=device.lock().unwrap().owner;
    device.lock().unwrap().before_guard=Some(external_expiry_and_same_owner_reclaim);
    let error=service.request(UsbRequest::ConfigChange{route,client_id:"editor-a".into(),change:ConfigChange::Reset}).unwrap_err();
    assert!(error.contains("revision_changed"),"unexpected guarded mutation response: {error}");
    let device=device.lock().unwrap();
    assert_eq!(device.owner,owner);assert_eq!(device.draft["name"],"External writer draft");
    assert_eq!(device.writes.iter().filter(|line|line.as_str()==format!("config reset {owner}")).count(),1,"no retry after guard rejection");
    assert_eq!(device.wire_writes.iter().filter(|line|line.starts_with("config if_revision 2 reset ")).count(),1);
    drop(device);
    assert!(service.snapshot().claimed_by.is_none());
    service.shutdown();
}

#[test]
fn successful_receipt_advances_guard_revision_for_the_next_mutation(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    let owner=device.lock().unwrap().owner;
    service.request(UsbRequest::ConfigChange{route,client_id:"editor-a".into(),change:ConfigChange::Reset}).unwrap();
    let device=device.lock().unwrap();
    assert!(device.wire_writes.iter().any(|line|line==&format!("config if_revision 1 claim {owner}")));
    assert!(device.wire_writes.iter().any(|line|line==&format!("config if_revision 2 reset {owner}")));
    assert_eq!(device.metadata_revision,3);
    drop(device);
    service.shutdown();
}

#[test]
fn older_metadata_cannot_rollback_a_newer_configuration_command_receipt(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    service.request(UsbRequest::ConfigChange{route:route.clone(),client_id:"editor-a".into(),change:ConfigChange::Reset}).unwrap();
    let before=service.snapshot();assert_eq!(before.claimed_by.as_deref(),Some("editor-a"));
    assert_eq!(before.draft.as_ref().unwrap()["dirty"],true);
    change(&device,2,2,0,false);
    thread::sleep(Duration::from_millis(60));
    let after=service.snapshot();assert_eq!(after.claimed_by.as_deref(),Some("editor-a"));
    assert_eq!(after.draft.as_ref().unwrap()["dirty"],true);
    service.shutdown();
}

#[test]
fn metadata_gap_malformed_frame_and_transport_loss_fail_closed(){
    for bytes in [
        concat!(r#"@meta {"boot":100,"sequence":3,"kind":"change","revision":2,"exhausted":false,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false}}"#, "\r\n").as_bytes(),
        b"@meta {bad json}\r\n".as_slice(),
    ]{
        let (service,device)=setup();connect(&service);
        device.lock().unwrap().stream_pipe.clone().unwrap().send(bytes);
        until(||!service.snapshot().connected);
        assert!(service.snapshot().error.is_some());service.shutdown();
    }
    let (service,device)=setup();connect(&service);
    device.lock().unwrap().stream_pipe.clone().unwrap().cancel();
    until(||!service.snapshot().connected);
    service.shutdown();
}

#[test]
fn missing_metadata_baseline_fails_connection_before_it_becomes_usable(){
    let (service,device)=setup();device.lock().unwrap().metadata_baseline=false;
    assert!(service.request(UsbRequest::Connect{port:port()}).is_err());
    assert!(!service.snapshot().connected);
    service.shutdown();
}

#[test]
fn retired_connection_metadata_cannot_overwrite_a_replacement_connection(){
    let (service,device)=setup();let old_route=connect(&service).route.unwrap();
    let old_pipe=device.lock().unwrap().stream_pipe.clone().unwrap();
    service.request(UsbRequest::Disconnect).unwrap();
    let new_route=connect(&service).route.unwrap();assert_ne!(old_route,new_route);
    let late=json!({"boot":100,"sequence":2,"kind":"change","revision":2,"exhausted":false,
        "state":{"generation":1,"savedGeneration":1,"owner":991,"claimExpiresMs":30000,"dirty":true,"pendingApply":false}});
    old_pipe.send_late(format!("@meta {late}\r\n").as_bytes());
    thread::sleep(Duration::from_millis(80));
    let current=service.snapshot();assert!(current.connected);assert_eq!(current.route,Some(new_route));
    assert!(current.claimed_by.is_none());assert!(current.draft.is_none());
    service.shutdown();
}

#[test]
fn metadata_snapshot_projects_cached_active_state_and_tracks_expiry_without_io(){
    let (service,device)=setup();let route=connect(&service).route.unwrap();claim(&service,&route);
    actions(&service,&route);
    service.request(UsbRequest::EventsState{route:route.clone()}).unwrap();
    let full=service.snapshot();
    assert!(full.claimed_by.is_some());assert!(full.draft.is_some());
    assert!(full.actions.is_some());assert!(full.events.is_some());
    let capture=service.input_capture().unwrap();
    let writes=device.lock().unwrap().writes.len();

    let metadata=service.metadata_snapshot();
    assert!(metadata.connected);assert_eq!(metadata.port.as_deref(),Some("COM8"));
    assert_eq!(metadata.identity,full.identity);assert_eq!(metadata.route,full.route);
    assert_eq!(metadata.config.as_ref().unwrap()["revision"],full.config.as_ref().unwrap()["revision"]);
    assert_eq!(metadata.observed_at_ms,full.observed_at_ms);assert_eq!(metadata.error,full.error);
    assert!(metadata.claimed_by.is_none());assert!(metadata.draft.is_none());
    assert!(metadata.actions.is_none());assert!(metadata.events.is_none());
    assert!(Arc::ptr_eq(&capture,&service.input_capture().unwrap()));
    assert_eq!(device.lock().unwrap().writes.len(),writes);

    stream_io::emit_metadata(&device,|device|{device.owner=0;device.draft=device.active.clone();device.dirty=false;});
    until(||service.snapshot().claimed_by.is_none());
    let after=service.metadata_snapshot();
    assert!(after.connected);assert_eq!(after.config.as_ref().unwrap()["owner"],0);
    assert!(after.claimed_by.is_none());assert!(after.draft.is_none());
    assert!(after.actions.is_none());assert!(after.events.is_none());
    assert!(Arc::ptr_eq(&capture,&service.input_capture().unwrap()));
    assert_eq!(device.lock().unwrap().writes.len(),writes,"expiry projection must not query the console");
    service.shutdown();
}

use super::*;
use std::time::{Duration, Instant};

const ID: &str = "stm32:00112233445566778899aabb";
fn port() -> Value { json!({"gatewayId":ID,"backend":"stm32","path":"COM1"}) }
fn entry(state: Value) -> Entry {
    let mut entry = Entry::new(ID.into(), port());
    entry.state = Arc::new(state);
    entry
}
fn connected() -> Value {
    json!({"connected":true,"port":"COM1","backend":"stm32","modules":[],"moduleConfigs":{},"moduleActions":{}})
}
fn begin(entry: &mut Entry, token: u64) -> Completion {
    entry.job = Some((token, entry.session.clone()));
    entry.job_source = Some(entry.state.clone());
    Completion { id:ID.into(), session:entry.session.clone(), token, action:Action::Connect, error:None }
}

#[test]
fn workspace_policy_connects_only_present_unpaused_idle_endpoints() {
    let mut row = entry(Value::Null);
    assert_eq!(row.plan(Instant::now()), Some(Action::Connect));
    row.present=false; assert_eq!(row.plan(Instant::now()),None);
    row.present=true; row.paused=true; assert_eq!(row.plan(Instant::now()),None);
    row.paused=false; let completion=begin(&mut row,1);
    assert_eq!(row.plan(Instant::now()),None);
    row.complete(&completion).unwrap();
    assert!(row.deadline().is_some());
    assert_eq!(row.plan(Instant::now()),None,"unchanged successful reply must back off");
    assert_eq!(row.plan(Instant::now()+Duration::from_secs(2)),Some(Action::Connect));
}

#[test]
fn workspace_hydration_preserves_claim_and_protocol_guards() {
    let mut state=connected();
    state["modules"]=json!([{"id":"m1","online":true,"registered":true,"canConfigure":true,"boot":1}]);
    let mut row=entry(state);
    assert_eq!(row.plan(Instant::now()),Some(Action::Config("m1".into())));
    row.owners.insert("m1".into(),"private-client".into());
    assert_eq!(row.plan(Instant::now()),Some(Action::Actions("m1".into())));
    let state=Arc::make_mut(&mut row.state);
    state["moduleActions"]["m1"]=json!({"actions":[]});
    assert_eq!(row.plan(Instant::now()),None);
    row.owners.clear();
    Arc::make_mut(&mut row.state)["modules"][0]["directUsb"]=json!(true);
    assert_eq!(row.plan(Instant::now()),None);
    Arc::make_mut(&mut row.state)["backend"]=json!("pico");
    assert_eq!(row.plan(Instant::now()),None);
    row.owners.insert("m1".into(),"private-client".into());
    assert_eq!(row.plan(Instant::now()),None);
    row.owners.clear();
    Arc::make_mut(&mut row.state)["needsReconnect"]=json!(true);
    assert_eq!(row.plan(Instant::now()),None);
}

#[test]
fn unsupported_pico_firmware_parks_auto_connect_until_topology_changes() {
    let mut row=entry(Value::Null);
    row.port["backend"]=json!("pico");
    let mut completion=begin(&mut row,1);
    completion.error=Some(crate::pico::FIRMWARE_REQUIRED.into());
    row.complete(&completion).unwrap();
    assert_eq!(row.plan(Instant::now()+Duration::from_secs(60)),None);
    assert_eq!(row.deadline(),None);
    row.present=false;
    row.port(&row.port.clone()).unwrap();
    assert_eq!(row.plan(Instant::now()),Some(Action::Connect));
}

#[test]
fn workspace_completion_is_token_and_session_fenced_and_real_progress_clears_retry() {
    let mut row=entry(Value::Null);
    let mut completion=begin(&mut row,4);
    completion.token=3;
    row.complete(&completion).unwrap(); assert!(row.job.is_some());
    completion.token=4; completion.error=Some("old failure".into());
    row.port(&json!({"gatewayId":ID,"backend":"stm32","path":"COM2"})).unwrap();
    row.complete(&completion).unwrap();
    assert!(row.error.is_empty()); assert!(row.job.is_none()); assert!(row.deadline().is_none());
    let mut completion=begin(&mut row,5); completion.error=Some("denied".into());
    row.complete(&completion).unwrap(); assert!(row.deadline().is_some());
    let completion=begin(&mut row,6); row.state=Arc::new(connected());
    row.complete(&completion).unwrap();
    assert!(row.error.is_empty()); assert!(row.deadline().is_none());
}

#[test]
fn workspace_wire_hashes_owners_hides_receipts_and_withdraws_wrong_port_state() {
    let mut state=connected(); state["nativeSnapshotAtMs"]=json!(42);
    state["claimedModules"]=json!(["m1"]);
    let mut row=entry(state); row.owners.insert("m1".into(),"secret-client".into());
    let wire=row.wire(&[port()]);
    assert!(!wire.to_string().contains("secret-client"));
    assert!(wire["claimOwners"]["m1"].as_str().unwrap().starts_with("sha256:"));
    assert!(wire["state"].get("nativeSnapshotAtMs").is_none());
    assert_eq!(wire["state"]["claimedModules"],json!([]));
    row.port(&json!({"gatewayId":ID,"backend":"stm32","path":"COM2"})).unwrap();
    assert!(row.wire(&[])["state"].is_null());
}

#[test]
fn workspace_background_requests_never_claim_or_renew_configuration() {
    for action in [Action::Connect,Action::Config("m1".into()),Action::Actions("m1".into())] {
        let request=action.request(ID,&port());
        assert_eq!(request["clientId"],policy::CLIENT);
        assert_eq!(request["gatewayId"],ID);
        assert!(!["claim","workspace_client_renew"].contains(&request["command"].as_str().unwrap()));
    }
}

#[derive(Default)]
struct EmptyProvider { calls:AtomicUsize, ports:Mutex<Vec<crate::transport::PortInfo>>, adapters:AtomicUsize }
use std::sync::atomic::AtomicUsize;
impl super::super::Provider for EmptyProvider {
    fn ports(&self)->Result<Vec<crate::transport::PortInfo>,String>{self.calls.fetch_add(1,Ordering::AcqRel);Ok(self.ports.lock().unwrap().clone())}
    fn adapter(&self,_:&str)->Arc<dyn Adapter>{self.adapters.fetch_add(1,Ordering::AcqRel);panic!("passive workspace must not acquire an adapter without ports")}
}

#[test]
fn workspace_owner_publishes_once_without_periodic_discovery_and_shutdown_is_final() {
    let provider=Arc::new(EmptyProvider::default());
    let manager=Arc::new(GatewayManager::with_provider(provider.clone()));
    manager.start_workspace().unwrap();
    assert_eq!(provider.calls.load(Ordering::Acquire),1);
    for _ in 0..20 {manager.workspace.changed();}
    // An explicit worker wake may publish cached data, never enumerate ports.
    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(provider.calls.load(Ordering::Acquire),1);
    manager.workspace.shutdown();
    assert!(manager.start_workspace().is_err());
    assert!(manager.workspace.page(&json!({"command":"workspace_read_model_page","id":"sha256:bad","offset":0})).is_err());
    manager.shutdown();
}

#[test]
fn workspace_pause_survives_missing_discovery_and_prevents_background_connect_on_arrival() {
    let provider=Arc::new(EmptyProvider::default());
    let manager=Arc::new(GatewayManager::with_provider(provider.clone()));
    manager.workspace.pause(ID,true).unwrap();
    manager.start_workspace().unwrap();
    let mut reader=manager.observations.broker().subscribe("workspace-test","ui","workspace.state").unwrap();
    assert!(reader.try_recv().unwrap().is_some());
    provider.ports.lock().unwrap().push(crate::transport::PortInfo {
        path:"COM1".into(),serial_number:Some("00112233445566778899aabb".into()),
        vendor_id:Some("1209".into()),product_id:Some("4010".into()),
        manufacturer:Some("OpenSimGear".into()),friendly_name:Some("OpenSimGear Gateway".into()),
    });
    manager.discover().unwrap();
    let deadline=Instant::now()+Duration::from_secs(2);
    loop {
        if let Some(frame)=reader.try_recv().unwrap() {
            let payload=frame.payload.unwrap();
            if payload["value"]["entries"].as_array().is_some_and(|entries|!entries.is_empty()) {
                assert_eq!(payload["value"]["entries"][0]["autoConnectPaused"],true);break;
            }
        }
        assert!(Instant::now()<deadline,"arrival was not published");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(provider.adapters.load(Ordering::Acquire),0);
    manager.shutdown();
}

#[test]
fn workspace_publication_pages_large_cache_reuses_issued_records_and_retires_consumers() {
    let manager=GatewayManager::with_provider(Arc::new(EmptyProvider::default()));
    let runtime=Runtime::new(); let mut view=publisher::View::new();
    let mut row=entry(connected());
    Arc::make_mut(&mut row.state)["large"]=json!("a".repeat(20000));
    let entries=BTreeMap::from([(ID.into(),row)]);
    view.publish(&runtime.inner,&manager,&entries,&[port()],"").unwrap();
    let broker=manager.observations.broker();
    let mut reader=broker.subscribe("workspace-pages","ui","workspace.state").unwrap();
    let first=reader.try_recv().unwrap().unwrap();
    let payload=first.payload.unwrap();
    let page=&payload["value"]["entries"][0]["state"]["large"]["$gamePage"];
    let request=json!({"command":"workspace_read_model_page","id":page["id"],"offset":0});
    let chunk=runtime.page(&request).unwrap();
    assert_eq!(chunk["total"],20000); assert_eq!(chunk["nextOffset"],8192);
    view.publish(&runtime.inner,&manager,&entries,&[port()],"").unwrap();
    assert!(reader.try_recv().unwrap().is_none(),"unchanged cached projection must stay quiet");
    assert_eq!(runtime.page(&request).unwrap(),chunk);
    assert!(runtime.page(&json!({"command":"workspace_read_model_page","id":page["id"],"offset":0,"extra":1})).is_err());
    view.retire(&manager);
    assert!(broker.subscribe("after-retirement","ui","workspace.state").is_err());
    manager.shutdown();
}

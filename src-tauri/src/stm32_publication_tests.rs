use super::*;
use std::{collections::VecDeque,io::{Read,Write}};
const UID:&str="10112233445566778899aabb";
const MODULE:&str="00112233445566778899aabbccddeeff";
const OWNER:&str="1234567890abcdef1234567890abcdef";
fn wire()->Value {
    let module=json!({"identity":MODULE,"online":true,"directUsb":false,"node":1,"sourceNode":11,"lease":30,"boot":20,"configurationGeneration":2,
        "profile":{"buttons":1,"axes":0,"hats":0,"revision":1,"axisUsage":[],"neutral":[]},"enabled":true,"published":true,"descriptorIndex":0,"reportId":1,
        "profileMatch":true,"hasInput":true,"report":"01","publicationSlot":0,"configuredEnabled":true,"publicationReportId":1,"configuredDescriptorIndex":0});
    json!({"gatewayType":"stm32-can-gateway","protocol":3,"canProtocol":3,"firmware":"0.4.0","identity":UID,"epoch":10,
        "running":true,"lifecycle":"automatic","layoutPending":false,"layoutError":null,"admissionFailure":null,"rejectedAnnouncements":0,
        "maxModules":32,"maxRemembered":32,"maxControllers":32,"activeGeneration":1,"pendingGeneration":1,
        "controllerPublicationOverride":capability(),"publicationRevision":4,"configuredGeneration":1,"effectiveGeneration":1,
        "publication":{"phase":"idle","outcome":"no_change","leaseId":0,"expiresAtMs":0,"transition":"0","claimRevision":0,"targetMask":"00000000","enabledMask":"00000000","expectedReconnect":false},
        "modules":[module]})
}
fn owned(wire:&mut Value,enabled:bool){
    wire["publication"]=json!({"phase":"owned","outcome":"ok","leaseId":8,"expiresAtMs":5000,"transition":"1","claimRevision":4,"targetMask":"00000001","enabledMask":if enabled{"00000001"}else{"00000000"},"expectedReconnect":false});
}
fn request(operation:PublicationOperation)->PublicationRequest{PublicationRequest{epoch:10,operation,authority:Some(Arc::new(AtomicBool::new(true))),control_epoch:Some((Arc::new(AtomicU64::new(4)),4)),fence:None}}

#[test]
fn capability_and_actual_overlay_preserve_configured_state_without_granting_debug_authority(){
    let mut value=wire();let normal=snapshot(&value,None).unwrap();assert_eq!(normal["controllerPublicationOverride"],capability());
    assert_eq!(normal["modules"][0]["publicationSlot"],0);assert_eq!(normal["modules"][0]["active"],true);
    owned(&mut value,true);value["modules"][0]["enabled"]=json!(false);value["modules"][0]["configuredEnabled"]=json!(false);value["modules"][0]["configuredDescriptorIndex"]=json!(-1);
    let normal=snapshot(&value,None).unwrap();assert_eq!(normal["modules"][0]["buttons"],json!([1]));assert_eq!(normal["modules"][0]["active"],true);assert_eq!(normal["modules"][0]["configuredEnabled"],false);
    value["controllerPublicationOverride"]["schema"]=json!(2);assert!(snapshot(&value,None).is_err(),"Unknown metadata cannot authorize a mismatched overlay");
    let mut value=wire();value["controllerPublicationOverride"]["unexpected"]=json!(true);assert_eq!(snapshot(&value,None).unwrap()["controllerPublicationOverride"],Value::Null);
    value["controllerPublicationOverride"]=Value::Null;assert!(snapshot(&value,None).unwrap()["publication"].is_null());
}
#[test]
fn malformed_recognized_ownership_and_target_metadata_cannot_authorize_a_route(){
    let value=wire();
    for (pointer,invalid) in [("/publicationRevision",json!(4294967296u64)),("/configuredGeneration",json!(-1)),("/publication/phase",json!("running")),("/publication/outcome",json!("done")),("/publication/leaseId",json!(-1)),("/publication/expectedReconnect",json!(1)),("/publication/transition",json!("01")),("/publication/transition",json!("18446744073709551616")),("/publication/targetMask",json!("0000000A")),("/publication/enabledMask",json!("00000001")),("/modules/0/publicationSlot",json!(32)),("/modules/0/publicationReportId",json!(256)),("/modules/0/configuredEnabled",json!(false)),("/modules/0/configuredDescriptorIndex",json!(-1))] {
        let mut bad=value.clone();*bad.pointer_mut(pointer).unwrap()=invalid;assert!(snapshot(&bad,None).is_err(),"{pointer}");
    }
    for key in ["extra"]{let mut bad=value.clone();bad["publication"][key]=json!(true);assert!(snapshot(&bad,None).is_err());}
    let mut bad=value.clone();owned(&mut bad,true);bad["modules"][0]["directUsb"]=json!(true);assert!(snapshot(&bad,None).is_err());
    bad["modules"][0]["directUsb"]=json!(false);bad["modules"][0]["online"]=json!(false);assert!(snapshot(&bad,None).is_err());
    let mut bad=value.clone();let extra=bad["modules"][0].clone();bad["modules"].as_array_mut().unwrap().push(extra);assert!(metadata(&bad).is_err());
}
#[test]
fn pending_or_revoked_owned_publication_never_reports_live_input(){
    let mut value=wire();owned(&mut value,false);assert!(snapshot(&value,None).is_err(),"Hidden target may not claim held input");
    value["modules"][0]["report"]=json!("00");assert_eq!(snapshot(&value,None).unwrap()["modules"][0]["active"],false);
    for phase in ["restoring","attention"]{value["publication"]["phase"]=json!(phase);assert_eq!(snapshot(&value,None).unwrap()["modules"][0]["active"],false);}
    value["publication"]["phase"]=json!("owned");value["publication"]["enabledMask"]=json!("00000001");value["layoutPending"]=json!(true);assert_eq!(snapshot(&value,None).unwrap()["modules"][0]["active"],false);
    value["modules"][0]["published"]=json!(false);value["modules"][0]["descriptorIndex"]=json!(-1);value["modules"][0]["reportId"]=Value::Null;value["modules"][0]["profileMatch"]=json!(false);value["layoutPending"]=json!(false);assert_eq!(snapshot(&value,None).unwrap()["modules"][0]["active"],false);
}
#[test]
fn typed_commands_are_bounded_and_revocation_precedes_dispatch(){
    let claim=request(PublicationOperation::Claim{owner:OWNER.into(),revision:u32::MAX,target_mask:u32::MAX,enabled_mask:u32::MAX,ttl_ms:5000});
    let text=claim.command().unwrap();assert!(text.len()+40<384);assert!(claim.mutating());assert!(text.contains("ffffffff ffffffff"));
    for operation in [PublicationOperation::State,PublicationOperation::Inspect{owner:OWNER.into(),lease:8},PublicationOperation::Release{owner:OWNER.into(),lease:8},PublicationOperation::Renew{owner:OWNER.into(),lease:8,ttl_ms:250}] {assert!(request(operation).command().is_ok());}
    for operation in [PublicationOperation::Claim{owner:OWNER.into(),revision:0,target_mask:1,enabled_mask:1,ttl_ms:1000},PublicationOperation::Claim{owner:OWNER.into(),revision:4,target_mask:0,enabled_mask:0,ttl_ms:1000},PublicationOperation::Claim{owner:OWNER.into(),revision:4,target_mask:1,enabled_mask:2,ttl_ms:1000},PublicationOperation::Renew{owner:OWNER.into(),lease:8,ttl_ms:249},PublicationOperation::Renew{owner:OWNER.into(),lease:8,ttl_ms:5001},PublicationOperation::Inspect{owner:OWNER.into(),lease:0},PublicationOperation::Release{owner:"0".repeat(32),lease:8},PublicationOperation::Release{owner:"f".repeat(32),lease:8},PublicationOperation::Release{owner:"bad\nowner".into(),lease:8}] {assert!(request(operation).command().is_err());}
    let mut revoked=claim.clone();revoked.authority=None;assert!(revoked.command().is_err());revoked.authority=Some(Arc::new(AtomicBool::new(false)));assert!(revoked.command().is_err());
    let mut late=claim.clone();late.control_epoch=None;assert!(late.command().is_err());late.control_epoch=Some((Arc::new(AtomicU64::new(5)),4));assert!(late.command().is_err());
    let mut bad=request(PublicationOperation::State);bad.epoch=0;assert!(bad.command().is_err());
}

#[test]
fn actual_production_c_publication_and_inspect_vectors_normalize_without_inventing_ownership(){
    let fixture:Value=serde_json::from_str(include_str!("../tests/fixtures/stm32-publication-v1.json")).unwrap();
    assert_eq!(fixture["schema"],1);
    let responses=fixture["responses"].as_array().unwrap();assert_eq!(responses.len(),4);
    for response in responses {
        let normalized=snapshot(&response["state"],Some("0123456789abcdef01234567")).unwrap();
        assert_eq!(normalized["controllerPublicationOverride"],capability());
        assert_eq!(normalized["publication"],response["state"]["publication"]);
        assert_eq!(normalized["modules"][0]["configuredEnabled"],true);
        assert_eq!(normalized["modules"][0]["published"],response["state"]["modules"][0]["published"]);
        assert!(normalized.get("publicationOwned").is_none(),"Ownership proof belongs to the correlated inspect reply");
    }
    assert_eq!(responses[2]["publicationOwned"],true);
    let mut churned=wire();churned["modules"][0]["publicationReportId"]=json!(200);
    assert!(snapshot(&churned,None).is_ok(),"A legal unselected configured report ID does not authorize or invalidate a claim");
}

struct Fake {state:Value,writes:Vec<String>,owned:Option<bool>,drop_reply:bool,controls:Vec<bool>}
type Context=Arc<Mutex<Fake>>;
struct FakeFactory(Context);
fn port()->PortInfo {PortInfo{path:"COM37".into(),serial_number:Some(UID.into()),vendor_id:Some("0483".into()),product_id:Some("5740".into()),manufacturer:Some("OpenSimGear".into()),friendly_name:Some("CAN gateway".into())}}
impl Factory for FakeFactory {
    fn ports(&mut self)->Result<Vec<PortInfo>,String>{Ok(vec![port()])}
    fn open(&mut self,_:&PortInfo,_:Duration)->Result<Box<dyn Connection>,String>{Ok(Box::new(FakeConnection{context:self.0.clone(),bytes:VecDeque::new()}))}
}
struct FakeConnection{context:Context,bytes:VecDeque<u8>}
impl Read for FakeConnection {
    fn read(&mut self,buffer:&mut [u8])->io::Result<usize>{if self.bytes.is_empty(){thread::sleep(Duration::from_millis(1));return Err(io::ErrorKind::TimedOut.into());}let count=buffer.len().min(self.bytes.len());for byte in &mut buffer[..count]{*byte=self.bytes.pop_front().unwrap();}Ok(count)}
}
impl Write for FakeConnection {
    fn write(&mut self,bytes:&[u8])->io::Result<usize>{let text=std::str::from_utf8(bytes).unwrap();let mut context=self.context.lock().unwrap();context.writes.push(text.into());if context.drop_reply{return Ok(bytes.len());}let id=text.split_whitespace().nth(1).unwrap().parse::<u32>().unwrap();let mut reply=json!({"id":id,"ok":true,"error":null,"state":context.state});if text.contains("publication inspect "){if let Some(owned)=context.owned{reply["publicationOwned"]=json!(owned);}}
        self.bytes.extend(format!("{reply}\n").bytes());Ok(bytes.len())}
    fn flush(&mut self)->io::Result<()>{Ok(())}
}
impl Connection for FakeConnection {fn set_control_lines(&mut self,dtr:bool,_:bool)->Result<(),String>{self.context.lock().unwrap().controls.push(dtr);Ok(())}}
fn gateway()->(Gateway,Context){
    let context=Arc::new(Mutex::new(Fake{state:wire(),writes:vec![],owned:Some(true),drop_reply:false,controls:vec![]}));
    let mut timing=Timing::default();timing.command=Duration::from_millis(30);timing.settle=Duration::ZERO;timing.poll=Duration::from_millis(1);
    let mut gateway=Gateway::new(Box::new(FakeFactory(context.clone())),timing,Arc::new(AtomicBool::new(false)));
    gateway.state=snapshot(&wire(),None).unwrap();
    gateway.board=Some(Board{info:port(),uid:UID.into()});
    gateway.connection=Some(GatewayConnection::RequestReply(Box::new(FakeConnection{
        context:context.clone(),bytes:VecDeque::new(),
    })));
    (gateway,context)
}

struct DuplexFactory(Option<crate::transport::duplex::DuplexParts>);
impl Factory for DuplexFactory {
    fn gateway_duplex(&mut self, _: &PortInfo) -> Option<Result<crate::transport::duplex::DuplexParts, String>> {
        self.0.take().map(Ok)
    }
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> { Ok(vec![port()]) }
    fn open(&mut self, _: &PortInfo, _: Duration) -> Result<Box<dyn Connection>, String> {
        panic!("native gateway fell back to request-only serial")
    }
}

#[test]
fn native_gateway_negotiates_and_delivers_panel_state_while_idle() {
    use crate::transport::duplex::tcp_fixture;
    use std::io::{BufRead, BufReader};
    let (parts, mut peer) = tcp_fixture();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let device = thread::spawn(move || {
        let mut lines = BufReader::new(peer.try_clone().unwrap()).lines();
        writeln!(peer, "gatewayType=stm32-can-gateway panelStream=1").unwrap();
        let first = lines.next().unwrap().unwrap();
        let id = |line: &str| {
            let parts: Vec<_> = line.split_ascii_whitespace().collect();
            assert_eq!(parts.len(), 3);
            assert_eq!((parts[0], parts[2]), ("cockpit", "snapshot"));
            parts[1].parse::<u32>().unwrap()
        };
        writeln!(peer, "{}", json!({"id":id(&first),"ok":true,"state":wire()})).unwrap();
        assert_eq!(lines.next().unwrap().unwrap(), "panel-stream");
        writeln!(peer, "{}", json!({"panelStream":"reset"})).unwrap();
        for (sequence, tag, body) in [
            (1u32, 0, [vec![1, 0, 0], vec![0; 16]].concat()),
            (2u32, 1, [vec![1], vec![0; 32]].concat()),
        ] {
            let mut bytes = vec![1];
            for field in [20, 30, 10, 2, sequence, 0, sequence] {
                bytes.extend_from_slice(&field.to_le_bytes());
            }
            bytes.push(tag);
            bytes.extend(body);
            let payload: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            writeln!(peer, "{}", json!({"panelStream":"event","moduleId":MODULE,"sourceNode":11,"payload":payload})).unwrap();
        }
        let second = lines.next().unwrap().unwrap();
        assert_eq!(id(&second), id(&first).wrapping_add(1).max(1));
        writeln!(peer, "{}", json!({"id":id(&second),"ok":true,"state":wire()})).unwrap();
    });
    let mut gateway = Gateway::new(
        Box::new(DuplexFactory(Some(parts))), Timing::default(), Arc::new(AtomicBool::new(false)),
    );
    gateway.open(port(), Some(UID.into()), Instant::now() + Duration::from_secs(2)).unwrap();
    assert_eq!(gateway.decorated(None)["panelStreamSupported"], true);
    let capture = gateway.panels.capture(MODULE).unwrap();
    let wake = Arc::new(osg_streams::NativeWake::default());
    wake.bind_thread(thread::current()).unwrap();
    let _watch = capture.watch(wake.clone()).unwrap();
    let mut reader = capture.subscribe().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let sample = loop {
        let observed = wake.observed();
        match reader.try_recv().unwrap() {
            Some(osg_streams::Delivery::Snapshot(snapshot)) if snapshot.value.0.is_some() => break snapshot.value.0.unwrap(),
            Some(osg_streams::Delivery::Event(event)) => break event.value.latest,
            Some(_) => continue,
            None => assert!(wake.wait(observed, deadline.saturating_duration_since(Instant::now()))),
        }
    };
    assert_eq!(sample.state.held[0], 1);
    assert_eq!(sample.route.source_node, 11);
    gateway.exchange("snapshot", Duration::from_secs(2)).unwrap();
    device.join().unwrap();
}

#[test]
fn inventory_reads_only_on_events_and_discards_superseded_readback() {
    use crate::transport::duplex::tcp_fixture;
    use std::io::{BufRead, BufReader};
    let (parts, mut peer) = tcp_fixture();
    let (step, steps) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let device = thread::spawn(move || {
        let mut input = BufReader::new(peer.try_clone().unwrap());
        let read_snapshot = |input: &mut BufReader<_>| {
            let mut line = String::new();
            input.read_line(&mut line).unwrap();
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 3);
            assert_eq!((fields[0], fields[2]), ("cockpit", "snapshot"));
            fields[1].parse::<u32>().unwrap()
        };
        let reply = |peer: &mut std::net::TcpStream, id, generation| {
            let mut state = wire();
            state["modules"][0]["configurationGeneration"] = json!(generation);
            writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state})).unwrap();
        };
        let event = |peer: &mut std::net::TcpStream, sequence: u64| {
            writeln!(peer, "{}", json!({"publicationStream":"change","epoch":10,"sequence":sequence.to_string()})).unwrap();
        };
        reply(&mut peer, read_snapshot(&mut input), 2);
        let mut opt_in = String::new();
        input.read_line(&mut opt_in).unwrap();
        assert_eq!(opt_in.trim_end(), "panel-stream");
        step.send(0).unwrap();
        resumed.recv_timeout(Duration::from_secs(2)).unwrap();
        event(&mut peer, 1);
        reply(&mut peer, read_snapshot(&mut input), 2);
        input.get_ref().set_read_timeout(Some(Duration::from_millis(130))).unwrap();
        let quiet = input.read_line(&mut String::new()).unwrap_err();
        assert!(matches!(quiet.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock));
        input.get_ref().set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        step.send(1).unwrap();
        resumed.recv_timeout(Duration::from_secs(2)).unwrap();
        event(&mut peer, 2);
        let obsolete = read_snapshot(&mut input);
        event(&mut peer, 3);
        step.send(2).unwrap();
        resumed.recv_timeout(Duration::from_secs(2)).unwrap();
        reply(&mut peer, obsolete, 3);
        let current = read_snapshot(&mut input);
        writeln!(peer, "{}", json!({"publicationStream":"inventory","epoch":10,"sequence":"4"})).unwrap();
        reply(&mut peer, current, 4);
        let diagnostic = read_snapshot(&mut input);
        step.send(3).unwrap();
        resumed.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut state = wire();
        state["modules"][0]["configurationGeneration"] = json!(4);
        state["admissionFailure"] = json!({"identity":MODULE,"error":"capacity_exceeded"});
        writeln!(peer, "{}", json!({"id":diagnostic,"ok":true,"state":state})).unwrap();
        resumed.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    let mut timing = Timing::default();
    timing.heartbeat = Duration::from_millis(20);
    timing.settle = Duration::ZERO;
    let service = Stm32Service::with_factory(Box::new(DuplexFactory(Some(parts))), timing);
    service.set_workspace_owned(true);
    let connected = service.request(json!({"command":"connect","port":port().path,"clientId":"inventory-cockpit-test"}));
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(steps.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
    assert_eq!(service.cached_state()["nativeInventoryCurrent"], false);
    resume.send(()).unwrap();
    let wait_state = |expected: bool, generation: u32| {
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            let state = service.cached_state();
            if state["nativeInventoryCurrent"] == expected && state["modules"][0]["configurationGeneration"] == generation {
                break state;
            }
            assert!(Instant::now() < until, "{state}");
            thread::sleep(Duration::from_millis(1));
        }
    };
    assert_eq!(steps.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    let baseline = wait_state(true, 2);
    assert!(service.workspace_snapshot().0.get("nativeInventoryCurrent").is_none());
    resume.send(()).unwrap();
    assert_eq!(steps.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
    let stale = wait_state(false, 2);
    assert_eq!(stale["nativeSnapshotAtMs"], baseline["nativeSnapshotAtMs"]);
    resume.send(()).unwrap();
    wait_state(true, 4);
    let authority = service.publication_fence().unwrap();
    assert_eq!(steps.recv_timeout(Duration::from_secs(2)).unwrap(), 3);
    assert_eq!(service.cached_state()["nativeInventoryCurrent"], true);
    assert!(authority.is_current());
    resume.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while service.workspace_snapshot().0["admissionFailure"]["error"] != "capacity_exceeded" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(authority.is_current());
    service.shutdown();
    resume.send(()).unwrap();
    device.join().unwrap();
}
#[test]
fn worker_preserves_atomic_inspect_proof_and_never_caches_private_owner(){
    let (mut gateway,context)=gateway();
    let inspected=gateway.publication(request(PublicationOperation::Inspect{owner:OWNER.into(),lease:8})).unwrap();assert_eq!(inspected.owned,Some(true));assert!(!inspected.state.to_string().contains(OWNER));assert!(inspected.state.get("publicationOwned").is_none());
    context.lock().unwrap().owned=Some(false);assert_eq!(gateway.publication(request(PublicationOperation::Inspect{owner:OWNER.into(),lease:8})).unwrap().owned,Some(false));
    context.lock().unwrap().owned=None;assert!(gateway.publication(request(PublicationOperation::Inspect{owner:OWNER.into(),lease:8})).is_err());
    let state=gateway.publication(request(PublicationOperation::State)).unwrap();assert_eq!(state.owned,None);
    let mut wrong=request(PublicationOperation::State);wrong.epoch=11;let before=context.lock().unwrap().writes.len();assert!(gateway.publication(wrong).is_err());assert_eq!(context.lock().unwrap().writes.len(),before);
    gateway.role=Role::PicoDebug;assert!(gateway.publication(request(PublicationOperation::State)).is_err());
}
#[test]
fn lost_mutation_reply_reconnects_by_reads_without_replaying_claim(){
    let (mut gateway,context)=gateway();context.lock().unwrap().drop_reply=true;
    assert!(gateway.publication(request(PublicationOperation::Claim{owner:OWNER.into(),revision:4,target_mask:1,enabled_mask:0,ttl_ms:1000})).is_err());assert!(gateway.connection.is_none());
    context.lock().unwrap().drop_reply=false;
    gateway.publication(request(PublicationOperation::State)).unwrap();
    assert_eq!(context.lock().unwrap().writes.iter().filter(|text|text.contains("publication claim ")).count(),1);
    let latch=Arc::new(AtomicBool::new(false));let before=context.lock().unwrap().writes.len();assert!(gateway.exchange_guarded("publication renew unsent",Duration::from_millis(1),Some(&latch),None,None).is_err());assert_eq!(context.lock().unwrap().writes.len(),before);assert!(gateway.connection.is_some(),"Unsent old work cannot close the replacement transport");
    let epoch=AtomicU64::new(6);assert!(gateway.exchange_guarded("publication renew late",Duration::from_millis(1),None,Some((&epoch,5)),None).is_err());assert_eq!(context.lock().unwrap().writes.len(),before);assert!(gateway.connection.is_some());
    let counter = Arc::new(AtomicU64::new(1));
    let fence = PublicationFence::fixture(10, counter.clone()).unwrap();
    counter.store(2, Ordering::Release);
    assert!(gateway.exchange_guarded("publication state", Duration::from_millis(1), None, None, Some(&fence)).is_err());
    assert_eq!(context.lock().unwrap().writes.len(), before);
    assert!(gateway.connection.is_some());
}
#[test]
fn typed_service_uses_the_existing_serial_owner_and_closes_bounded_requests(){
    let (_,context)=gateway();let service=Stm32Service::with_factory(Box::new(FakeFactory(context.clone())),Timing::default());
    assert_eq!(service.request(json!({"command":"connect","clientId":"native-publication-test","port":"COM37","gatewayId":format!("stm32:{UID}")}))["ok"],true);
    assert_eq!(service.publication(request(PublicationOperation::State)).unwrap().owned,None);
    assert_eq!(service.publication(request(PublicationOperation::Inspect{owner:OWNER.into(),lease:8})).unwrap().owned,Some(true));
    assert_eq!(service.request(json!({"command":"publication_claim","clientId":"native-publication-test","owner":OWNER}))["ok"],false,"JSON commands have no publication authority");
    service.shutdown();assert!(service.publication(request(PublicationOperation::State)).is_err());
}
mod layout_recovery {
    include!("stm32/layout_recovery_tests.rs");
}
mod command_route {
    include!("stm32/command_route_tests.rs");
}

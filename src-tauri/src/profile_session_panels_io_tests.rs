use super::*;
use crate::module_usb::{ConfigChange, UsbRouteToken};
use std::{thread, time::Duration};
const GATEWAY: &str = "stm32:00112233445566778899aabb";
const MODULE: &str = "4f53474400112233445566778899aabb";
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<Value>>,
    publications: Mutex<Vec<crate::stm32::PublicationRequest>>,
    claims: Mutex<Vec<crate::module_configuration_routes::WriterPin>>,
}
impl Adapter for Fake {
    fn publication(&self,request:crate::stm32::PublicationRequest)->Result<crate::stm32::PublicationReply,String>{
        self.publications.lock().unwrap().push(request);
        Ok(crate::stm32::PublicationReply{state:self.cached_state(),owned:None})
    }
    fn cached_state(&self) -> Value {
        json!({"connected":true,"modules":[{"id":MODULE,"online":true,"boot":10}]})
    }
    fn configuration_claims(&self) -> Vec<crate::module_configuration_routes::WriterPin> {
        self.claims.lock().unwrap().clone()
    }
    fn request(&self, request: Value) -> Value {
        self.calls.lock().unwrap().push(request.clone());
        if request["command"] == "claim" {
            self.claims
                .lock()
                .unwrap()
                .push(crate::module_configuration_routes::WriterPin {
                    module_id: MODULE.into(),
                    client_id: request["clientId"].as_str().unwrap().into(),
                    boot: 10,
                });
        }
        if request["command"] == "release" {
            self.claims
                .lock()
                .unwrap()
                .retain(|pin| request["clientId"] != pin.client_id);
        }
        json!({"ok":true,"state":self.cached_state()})
    }
    fn shutdown(&self) {}
}
fn fixture() -> (Arc<ProfilePanelIo>, Arc<Fake>, String, UsbRouteToken) {
    let usb = Arc::new(crate::module_usb_fleet::tests::manager_fixture());
    let selected = usb.snapshots().pop().unwrap();
    let route = selected.snapshot.route.unwrap();
    let key = selected.key;
    let fake = Arc::new(Fake::default());
    let mut sessions: BTreeMap<String, Arc<dyn Adapter>> = BTreeMap::new();
    sessions.insert(GATEWAY.into(), fake.clone());
    (
        Arc::new(ProfilePanelIo::new(
            Arc::new(Mutex::new(sessions)),
            usb,
            Arc::new(Mutex::new(())),
            Arc::new(AtomicBool::new(false)),
        )),
        fake,
        key,
        route,
    )
}
fn request(command: &str) -> Value {
    json!({"command":command,"gatewayId":GATEWAY,"moduleId":MODULE,"clientId":"profile-test","boot":10})
}
#[test]
fn native_dispatch_cannot_open_save_reset_forget_or_mutate_a_restarted_module() {
    let (io, adapter, key, route) = fixture();
    assert_eq!(io.can(request("status"))["ok"], true);
    for command in [
        "connect",
        "disconnect",
        "forget",
        "save",
        "config_save",
        "config_apply_save",
        "config_reset",
        "profile_reset",
    ] {
        assert_eq!(io.can(request(command))["ok"], false);
    }
    let mut stale = request("action_invoke");
    stale["boot"] = json!(11);
    assert_eq!(io.can(stale)["ok"], false);
    let mut missing = request("enable");
    missing.as_object_mut().unwrap().remove("boot");
    assert_eq!(io.can(missing)["ok"], false);
    assert_eq!(adapter.calls.lock().unwrap().len(), 1);
    for change in [
        ConfigChange::Save,
        ConfigChange::Reset,
        ConfigChange::Discard,
        ConfigChange::Apply {
            config: json!({}),
            save: true,
        },
    ] {
        assert!(io
            .usb(
                &key,
                UsbRequest::ConfigChange {
                    route: route.clone(),
                    client_id: "profile-test".into(),
                    change
                }
            )
            .is_err());
    }
    assert!(io.usb(&key, UsbRequest::Disconnect).is_err());
    assert!(io
        .usb(
            &key,
            UsbRequest::ConfigRead {
                route: route.clone()
            }
        )
        .is_ok());
    let mut stale = route;
    stale.connection_generation += 1;
    assert!(io
        .usb(&key, UsbRequest::ConfigRead { route: stale })
        .is_err());
}
#[test]
fn native_claims_share_the_manager_gate_and_require_explicit_release_before_switching() {
    let (io, _, key, route) = fixture();
    assert_eq!(io.can(request("claim"))["ok"], true);
    assert!(io
        .usb(
            &key,
            UsbRequest::Claim {
                route: route.clone(),
                client_id: "profile-test".into()
            }
        )
        .is_err());
    assert_eq!(io.can(request("release"))["ok"], true);
    io.usb(
        &key,
        UsbRequest::Claim {
            route: route.clone(),
            client_id: "profile-test".into(),
        },
    )
    .unwrap();
    assert_eq!(io.can(request("claim"))["ok"], false);
    io.usb(
        &key,
        UsbRequest::Release {
            route: route.clone(),
            client_id: "profile-test".into(),
        },
    )
    .unwrap();
    assert_eq!(io.can(request("claim"))["ok"], true);
}
#[test]
fn simultaneous_native_usb_can_claims_cannot_both_own_the_configuration() {
    let (io, _, key, route) = fixture();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let a = io.clone();
    let entered = barrier.clone();
    let can = thread::spawn(move || {
        entered.wait();
        a.can(request("claim"))["ok"] == true
    });
    let a = io.clone();
    let entered = barrier.clone();
    let usb = thread::spawn(move || {
        entered.wait();
        a.usb(
            &key,
            UsbRequest::Claim {
                route,
                client_id: "profile-test".into(),
            },
        )
        .is_ok()
    });
    barrier.wait();
    assert_ne!(can.join().unwrap(), usb.join().unwrap());
}
#[test]
fn native_dispatch_waits_for_the_shared_gate_and_shutdown_prevents_late_mutation() {
    let (io, adapter, _, _) = fixture();
    let guard = io.gate.lock().unwrap();
    let pending = io.clone();
    let result = thread::spawn(move || pending.can(request("enable")));
    thread::sleep(Duration::from_millis(10));
    assert!(adapter.calls.lock().unwrap().is_empty());
    io.closed.store(true, Ordering::Release);
    drop(guard);
    assert_eq!(result.join().unwrap()["ok"], false);
    assert!(adapter.calls.lock().unwrap().is_empty());
}
#[test]
fn native_read_errors_and_forged_preference_never_create_a_session_or_move_a_writer() {
    let (io, adapter, key, route) = fixture();
    let mut r = request("claim");
    r["_directUsbPreferred"] = json!(true);
    assert_eq!(io.can(r)["ok"], true);
    assert_eq!(
        adapter.calls.lock().unwrap()[0]["_directUsbPreferred"],
        false
    );
    let mut missing = request("status");
    missing["gatewayId"] = json!("missing");
    assert_eq!(io.can(missing)["ok"], false);
    assert_eq!(io.can(json!([]))["ok"], false);
    assert_eq!(io.can(json!({}))["ok"], false);
    assert_eq!(
        io.can(json!({"command":"status","padding":"x".repeat(65536)}))["ok"],
        false
    );
    assert_eq!(io.usb_snapshots().len(), 1);
    io.closed.store(true, Ordering::Release);
    assert!(io.usb(&key, UsbRequest::ConfigRead { route }).is_err());
    assert_eq!(io.can(request("status"))["ok"], false);
}

#[test]
fn cached_native_can_snapshot_performs_no_request_or_discovery() {
    let (io, adapter, _, _) = fixture();
    assert_eq!(io.can_snapshot(GATEWAY).unwrap()["connected"], true);
    assert!(adapter.calls.lock().unwrap().is_empty());
    assert!(io.can_snapshot("unknown").is_err());
    io.close();
    assert!(io.can_snapshot(GATEWAY).is_err());
}

#[test]
fn native_publication_dispatch_is_typed_exact_scoped_and_closed_after_the_writer_gate(){
    use crate::stm32::{PublicationOperation,PublicationRequest};
    let (io,adapter,_,_)=fixture();
    let read=||PublicationRequest{epoch:10,operation:PublicationOperation::State,authority:None,control_epoch:None};
    assert!(io.publication(GATEWAY,read()).is_ok());
    assert_eq!(adapter.publications.lock().unwrap().len(),1);
    assert!(io.publication("unknown",read()).is_err());
    assert_eq!(io.can(request("publication_claim"))["ok"],false,"JSON routes cannot provide private ownership");
    adapter.publications.lock().unwrap().clear();
    let guard=io.gate.lock().unwrap();let waiting=io.clone();
    let operation=thread::spawn(move||waiting.publication(GATEWAY,PublicationRequest{epoch:10,operation:PublicationOperation::Release{owner:"1234567890abcdef1234567890abcdef".into(),lease:1},authority:None,control_epoch:None}));
    thread::sleep(Duration::from_millis(10));assert!(adapter.publications.lock().unwrap().is_empty());
    io.closed.store(true,Ordering::Release);drop(guard);assert!(operation.join().unwrap().is_err());
    assert!(io.publication(GATEWAY,read()).is_err());assert!(adapter.publications.lock().unwrap().is_empty());
}

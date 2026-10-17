use super::*;
use osg_profiles::{resources::*, resources_ports::*};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
const MODULE: &str = "4f53474400112233445566778899aabb";
const GATEWAY: &str = "stm32:ffeeddccbbaa998877665544";
struct TestProtector;
impl Protector for TestProtector {
    fn seal(&self, b: &[u8]) -> Result<Vec<u8>, String> {
        Ok(b.iter().map(|v| v ^ 0x96).collect())
    }
    fn unseal(&self, b: &[u8]) -> Result<Vec<u8>, String> {
        Ok(b.iter().map(|v| v ^ 0x96).collect())
    }
}
fn owner() -> RunOwner {
    RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: uuid::Uuid::new_v4().to_string(),
        profile_revision: 1,
        game_id: None,
    }
}
fn descriptors() -> Vec<Value> {
    vec![
        json!({"id":"display.set","label":"Set display","endpoint":"display","semantics":"setter","readable":true,"parameters":[{"id":"enabled","label":"Enabled","type":"boolean"},{"id":"level","label":"Level","type":"integer","minimum":-10,"maximum":100},{"id":"mode","label":"Mode","type":"enum","options":["on","off"]},{"id":"text","label":"Text","type":"string","maxBytes":63}]}),
        json!({"id":"display.flash","label":"Flash display","endpoint":"display","semantics":"oneShot","readable":false,"parameters":[]}),
    ]
}
struct Device {
    cache: Value,
    current: Vec<Value>,
    fallback: Vec<Value>,
    last: Option<Vec<Value>>,
    owned: bool,
    secret: String,
    id: u32,
    writes: Vec<Value>,
    lost: Option<String>,
    bad_ack: Option<String>,
    bad_page: bool,
    fallback_status: String,
}
impl Device {
    fn new() -> Self {
        Self {
            cache: json!({"status":"ready","catalogueRevision":7,"actions":descriptors(),"states":[],"ownership":{"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000,"restorePolicy":"if-unchanged","fallbackPolicy":"capture-at-claim"}}),
            current: vec![json!(false), json!(5), json!("off"), json!("prior")],
            fallback: vec![],
            last: None,
            owned: false,
            secret: String::new(),
            id: 0,
            writes: vec![],
            lost: None,
            bad_ack: None,
            bad_page: false,
            fallback_status: "none".into(),
        }
    }
    fn run(&mut self, r: Value) -> Result<(), String> {
        let command = r["command"].as_str().unwrap().to_owned();
        if command != "action_state" {
            // Both native USB and CAN paths use this production encoder before
            // firmware sees a lease command. The device fixture must not admit
            // a private token that either real adapter would reject.
            crate::output_leases::child(
                &command,
                &r,
                self.cache["catalogueRevision"].as_u64().unwrap(),
                self.cache["actions"].as_array().unwrap(),
            )?;
        }
        self.writes.push(r.clone());
        let check = |this: &Self| {
            this.owned && r["ownershipToken"] == this.secret && r["leaseId"] == this.id
        };
        match command.as_str() {
            "action_state" => {
                self.cache["status"] = json!("ready");
                self.cache["states"] = json!([{"id":"display.set","values":self.current}]);
            }
            "action_claim_current" => {
                if self.owned {
                    return Err("outputBusy".into());
                }
                self.id += 1;
                self.owned = true;
                self.secret = r["ownershipToken"].as_str().unwrap().into();
                self.fallback = self.current.clone();
                self.fallback_status = "none".into();
                self.last = Some(self.current.clone());
                self.cache["lastLease"] = json!({"leaseId":self.id,"endpoint":"display","remainingMs":5000,"released":null});
            }
            "action_invoke_owned" => {
                if !check(self) {
                    return Err("outputNotOwned".into());
                }
                if r["actionId"] == "display.set" {
                    self.current = r["values"].as_array().unwrap().clone();
                } else {
                    self.current[0] = json!(!self.current[0].as_bool().unwrap());
                }
                self.last = Some(self.current.clone());
                self.cache["states"] = json!([{"id":r["actionId"],"values":r["values"]}]);
            }
            "action_renew" => {
                if !check(self) {
                    return Err("outputNotOwned".into());
                }
                self.cache["lastLease"] = json!({"leaseId":self.id,"endpoint":"display","remainingMs":5000,"released":null});
            }
            "action_release" => {
                if !check(self) {
                    return Err("outputNotOwned".into());
                }
                let applied = self.last.as_ref() == Some(&self.current);
                if applied {
                    self.current = self.fallback.clone();
                }
                self.owned = false;
                self.fallback_status = if applied { "applied" } else { "preserved" }.into();
                self.cache["lastLease"] = json!({"leaseId":self.id,"endpoint":"display","released":true,"fallbackStatus":self.fallback_status});
            }
            "action_ownership" => {
                self.cache["ownershipPage"] = json!({"schemaVersion":1,"catalogueRevision":7,"total":1,"next":null,"ownership":[{"endpoint":"display","actionId":"display.set","owned":self.owned,"leaseId":self.id,"remainingMs":if self.owned{5000}else{0},"fallback":self.fallback,"fallbackStatus":self.fallback_status}]});
                if self.bad_page {
                    self.cache["ownershipPage"]["next"] = json!(r["offset"]);
                }
            }
            _ => panic!("unexpected physical command {command}"),
        }
        if let Some(field) = self.bad_ack.take() {
            self.cache["lastLease"][field] = json!("wrong");
        }
        if self.lost.as_ref() == Some(&command) {
            self.lost = None;
            self.cache["status"] = json!("uncertain");
            return Err("lost reply containing private input must not escape".into());
        }
        Ok(())
    }
}
struct Fake {
    device: Mutex<Device>,
    registry: Mutex<Registry>,
    route: Mutex<Route>,
    usb_route: Mutex<UsbRouteToken>,
    fail_authority: AtomicBool,
    // Native normalized identity fields; edits model independent discoveries.
    can_identity: Mutex<Value>,
}
impl Fake {
    fn snapshot(&self) -> UsbSnapshot {
        let route = self.usb_route.lock().unwrap().clone();
        let mut actions = self.device.lock().unwrap().cache.clone();
        actions["route"] = json!(route);
        UsbSnapshot {
            connected: true,
            route: Some(route),
            actions: Some(actions),
            ..Default::default()
        }
    }
    fn state(&self) -> Value {
        let route = self.route.lock().unwrap().clone();
        let v: Vec<Value> = serde_json::from_str(&route.provider_generation).unwrap();
        let node: Value = serde_json::from_str(&route.attributes["node"]).unwrap();
        let mut cache = self.device.lock().unwrap().cache.clone();
        cache["route"] = json!([v[0], node, v[2], v[1]]);
        cache["configurationGeneration"] = v[3].clone();
        let identity = self.can_identity.lock().unwrap();
        json!({"backend":"stm32","gatewaySerial":identity["gatewaySerial"],"connected":true,"epoch":v[0],"modules":[{"id":identity["id"],"online":true,"node":node,"lease":v[2],"boot":v[1],"configurationGeneration":v[3],"sourceNode":identity["sourceNode"],"profile":identity["profile"]}],"moduleActions":{MODULE:cache}})
    }
    fn calls(&self, command: &str) -> usize {
        self.device
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|r| r["command"] == command)
            .count()
    }
}
impl Io for Fake {
    fn preparation(&self, p: &PortPreparation) -> Result<Route, String> {
        if self.fail_authority.load(Ordering::Acquire) {
            return Err("revoked".into());
        }
        Ok(self.registry.lock().unwrap().validate_preparation(p)?.route)
    }
    fn delivery(&self, t: &ResourceToken) -> Result<Route, String> {
        if self.fail_authority.load(Ordering::Acquire) {
            return Err("revoked".into());
        }
        Ok(self
            .registry
            .lock()
            .unwrap()
            .validate_token(t, &t.port_id, t.access)?
            .route)
    }
    fn snapshots(&self) -> Vec<SlotSnapshot> {
        vec![SlotSnapshot {
            key: "module".into(),
            port: crate::transport::PortInfo {
                path: "COM7".into(),
                serial_number: Some("00112233445566778899aabb".into()),
                vendor_id: None,
                product_id: None,
                manufacturer: None,
                friendly_name: None,
            },
            present: true,
            automatic: false,
            admissible: true,
            snapshot: self.snapshot(),
            diagnostics: vec![],
        }]
    }
    fn can_cached(&self, _: &str) -> Result<Value, String> {
        Ok(self.state())
    }
    fn usb(&self, _: &str, r: UsbRequest) -> Result<UsbSnapshot, String> {
        let UsbRequest::Action { route, action } = r else {
            panic!("not an action")
        };
        assert_eq!(route, *self.usb_route.lock().unwrap());
        let command = match action {
            ActionRequest::State => Command::State,
            ActionRequest::Ownership { offset } => Command::Ownership(offset),
            ActionRequest::ClaimCurrent {
                catalogue_revision,
                action_id,
                token,
                ttl_ms,
            } => {
                assert_eq!(ttl_ms, TTL_MS);
                Command::Claim {
                    revision: catalogue_revision,
                    id: action_id,
                    secret: token,
                }
            }
            ActionRequest::Renew {
                token,
                lease_id,
                ttl_ms,
            } => {
                assert_eq!(ttl_ms, TTL_MS);
                Command::Renew {
                    secret: token,
                    lease: lease_id,
                }
            }
            ActionRequest::Release { token, lease_id } => Command::Release {
                secret: token,
                lease: lease_id,
            },
            ActionRequest::InvokeOwned {
                catalogue_revision,
                action_id,
                values,
                token,
                lease_id,
            } => Command::Invoke {
                revision: catalogue_revision,
                id: action_id,
                values,
                secret: token,
                lease: lease_id,
            },
            _ => panic!("adapter must not use unowned/legacy actions"),
        };
        self.device.lock().unwrap().run(command.can())?;
        Ok(self.snapshot())
    }
    fn can(&self, r: Value) -> Value {
        assert_eq!(r["gatewayId"], GATEWAY);
        assert_eq!(r["moduleId"], MODULE);
        assert_eq!(r["boot"], 10);
        let result = self.device.lock().unwrap().run(r);
        match result {
            Ok(()) => json!({"ok":true,"state":self.state()}),
            Err(e) => json!({"ok":false,"error":e}),
        }
    }
}
fn fixture(can: bool, one_shot: bool) -> (SinkPlan, Arc<Fake>, RunOwner) {
    let usb = UsbRouteToken {
        module_id: MODULE.into(),
        serial: "00112233445566778899aabb".into(),
        connection_generation: 1,
        boot: 10,
        configuration_generation: 20,
    };
    let route = Route {
        id: if can {
            format!("can:{GATEWAY}:{MODULE}")
        } else {
            "usb:module".into()
        },
        transport: if can { Transport::Can } else { Transport::Usb },
        provider_generation: if can {
            json!([30, 10, 40, 20, null]).to_string()
        } else {
            "1:10:20".into()
        },
        attributes: if can {
            BTreeMap::from([
                ("gatewayId".into(), GATEWAY.into()),
                ("moduleId".into(), MODULE.into()),
                ("node".into(), "50".into()),
                ("boot".into(), "10".into()),
            ])
        } else {
            BTreeMap::new()
        },
        baseline_ready: false,
        writable: true,
        priority: 100,
    };
    let device = Device::new();
    let ports = crate::profile_resource_ports::actions(&device.cache).unwrap();
    let mut registry = Registry::new();
    registry
        .reconcile(ProviderBatch {
            scope: "native".into(),
            session: "provider".into(),
            revision: 1,
            complete: true,
            observations: vec![Observation {
                key: MODULE.into(),
                name: "Physical panel".into(),
                resource_type: ResourceType::OsgModule,
                evidence: vec![],
                attributes: BTreeMap::new(),
                diagnostics: vec![],
                route: Some(route.clone()),
                ports: Some(PortUpdate::complete(ports.clone()).unwrap()),
            }],
        })
        .unwrap();
    let id = registry.list(0, 128).unwrap().resources[0].id.clone();
    let p = ports[usize::from(one_shot)].clone();
    let pin = registry
        .prepare_port(&id, &p.id, &p.contract_hash().unwrap(), Access::Invoke)
        .unwrap();
    let token = registry.token(&id, &p.id, Access::Invoke).unwrap();
    let sink = SinkPlan {
        endpoint: osg_profiles::Endpoint {
            node_id: "logical-panel".into(),
            port_id: "write".into(),
        },
        adapter: SinkAdapter::PhysicalAction,
        inventory_resource_id: Some(id),
        observed_port_id: Some(p.id.clone()),
        contract: p.contract,
        token: Some(token),
        preparation: Some(pin),
        route: Some(route.clone()),
        readiness: PlanReadiness::Ready,
    };
    (
        sink,
        Arc::new(Fake {
            device: Mutex::new(device),
            registry: Mutex::new(registry),
            route: Mutex::new(route),
            usb_route: Mutex::new(usb),
            fail_authority: AtomicBool::new(false),
            can_identity: Mutex::new(
                json!({"gatewaySerial":"ffeeddccbbaa998877665544","id":MODULE,"sourceNode":50,"profile":{"revision":1,"buttons":12,"axes":1,"hats":0,"neutral":[32768],"axisUsage":[48]}}),
            ),
        }),
        owner(),
    )
}
fn prepared(sink: &SinkPlan, io: &Arc<Fake>, owner: &RunOwner) -> Lease {
    Lease::prepare_io(sink, owner, io.clone(), &TestProtector).unwrap()
}
fn args() -> Vec<NamedArgument> {
    vec![
        NamedArgument {
            id: "enabled".into(),
            value: osg_profiles::PortValue::Boolean(true),
        },
        NamedArgument {
            id: "level".into(),
            value: osg_profiles::PortValue::Number(42.),
        },
        NamedArgument {
            id: "mode".into(),
            value: osg_profiles::PortValue::Text("on".into()),
        },
        NamedArgument {
            id: "text".into(),
            value: osg_profiles::PortValue::Text("é display".into()),
        },
    ]
}
#[test]
fn capture_is_read_only_and_native_cleanup_receipt_has_no_replay_tuple_or_secret() {
    let (sink, io, owner) = fixture(false, false);
    let mut lease = prepared(&sink, &io, &owner);
    assert!(io.device.lock().unwrap().writes.is_empty());
    let receipt = lease.receipt();
    let public = serde_json::to_string(&receipt).unwrap();
    assert!(public.len() < 8192);
    assert!(!public.contains(&lease.cleanup.secret));
    assert!(!public.contains("prior"));
    assert!(!public.contains("display.set"));
    assert!(!public.contains("ownershipToken"));
    assert_eq!(lease.endpoint_key(), receipt.resource_id);
    assert_eq!(lease.release().unwrap(), Released::Preserved);
    assert!(lease.claim().is_err());
    assert!(io.device.lock().unwrap().writes.is_empty());
}
#[test]
fn atomic_claim_captures_actual_prior_state_and_dispatch_preserves_exact_argument_tuple() {
    for can in [false, true] {
        let (sink, io, owner) = fixture(can, false);
        let mut lease = prepared(&sink, &io, &owner);
        io.device.lock().unwrap().current = vec![
            json!(false),
            json!(9),
            json!("off"),
            json!("changed after prepare"),
        ];
        let prior = io.device.lock().unwrap().current.clone();
        lease.claim().unwrap();
        assert!(lease.claim().is_err());
        assert!(lease.token.is_none());
        lease.bind_delivery(&sink).unwrap();
        lease.invoke(&owner, &args()).unwrap();
        lease.renew(&owner).unwrap();
        let command = io
            .device
            .lock()
            .unwrap()
            .writes
            .iter()
            .find(|r| r["command"] == "action_invoke_owned")
            .unwrap()
            .clone();
        assert_eq!(command["values"], json!([true, 42, "on", "é display"]));
        assert_eq!(io.calls("action_claim_current"), 1);
        assert_eq!(lease.release().unwrap(), Released::Restored);
        assert_eq!(io.device.lock().unwrap().current, prior);
        assert_eq!(lease.release().unwrap(), Released::Restored);
        assert_eq!(io.calls("action_release"), 1);
        assert!(lease.renew(&owner).is_err());
    }
}
#[test]
fn production_parser_admits_live_and_recovered_endpoint_lease_commands_on_usb_and_can() {
    for can in [false, true] {
        let (sink, io, owner) = fixture(can, false);
        let mut lease = prepared(&sink, &io, &owner);
        let receipt = lease.receipt();
        let prior = io.device.lock().unwrap().current.clone();
        lease.claim().unwrap();
        lease.bind_delivery(&sink).unwrap();
        lease.invoke(&owner, &args()).unwrap();
        lease.renew(&owner).unwrap();
        let mut recovered =
            Lease::recover_io(&receipt, &owner, io.clone(), &TestProtector).unwrap();
        assert!(canonical_uuid(&recovered.cleanup.secret));
        assert_eq!(recovered.cleanup.secret, lease.cleanup.secret);
        assert!(recovered.claim().is_err());
        assert!(recovered.bind_delivery(&sink).is_err());
        assert!(recovered.invoke(&owner, &args()).is_err());
        assert!(recovered.renew(&owner).is_err());
        assert_eq!(recovered.release().unwrap(), Released::Restored);
        assert_eq!(io.device.lock().unwrap().current, prior);
        for command in [
            "action_claim_current",
            "action_invoke_owned",
            "action_renew",
            "action_release",
        ] {
            assert_eq!(io.calls(command), 1, "{command} must never be replayed");
        }
    }
}

#[test]
fn recovery_rejects_sealed_tokens_that_the_endpoint_wire_parser_cannot_admit() {
    let (sink, io, owner) = fixture(true, false);
    let lease = prepared(&sink, &io, &owner);
    let receipt = lease.receipt();
    let envelope: Envelope = serde_json::from_value(receipt.state.clone()).unwrap();
    let plain = TestProtector
        .unseal(&unhex(&envelope.sealed_hex).unwrap())
        .unwrap();
    let mut cleanup: Cleanup = serde_json::from_slice(&plain).unwrap();
    for token in [
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::nil().to_string(),
        "00112233_4455-4677-8899-aabbccddeeff".into(),
    ] {
        cleanup.secret = token;
        let sealed = TestProtector
            .seal(&serde_json::to_vec(&cleanup).unwrap())
            .unwrap();
        let mut bad = receipt.clone();
        bad.state = serde_json::to_value(Envelope {
            schema_version: 1,
            sealed_hex: hex(&sealed),
        })
        .unwrap();
        assert!(Lease::recover_io(&bad, &owner, io.clone(), &TestProtector).is_err());
    }
    assert!(io.device.lock().unwrap().writes.is_empty());
}
#[test]
fn one_shot_alias_uses_same_readable_endpoint_lease_and_preserves_external_edits() {
    let (sink, io, owner) = fixture(true, true);
    let mut lease = prepared(&sink, &io, &owner);
    lease.claim().unwrap();
    lease.bind_delivery(&sink).unwrap();
    lease.invoke(&owner, &[]).unwrap();
    assert_eq!(
        io.device.lock().unwrap().writes[0]["actionId"],
        "display.set"
    );
    assert_eq!(
        io.device
            .lock()
            .unwrap()
            .writes
            .iter()
            .find(|r| r["command"] == "action_invoke_owned")
            .unwrap()["actionId"],
        "display.flash"
    );
    io.device.lock().unwrap().current[3] = json!("external edit");
    assert_eq!(lease.release().unwrap(), Released::Preserved);
    assert_eq!(io.device.lock().unwrap().current[3], "external edit");
}
#[test]
fn lost_claim_never_replays_and_recovery_only_releases_exact_existing_ownership() {
    let (sink, io, owner) = fixture(false, false);
    let mut lease = prepared(&sink, &io, &owner);
    let receipt = lease.receipt();
    io.device.lock().unwrap().lost = Some("action_claim_current".into());
    assert!(lease.claim().is_err());
    assert!(lease.claim().is_err());
    assert!(lease.bind_delivery(&sink).is_err());
    assert_eq!(io.calls("action_claim_current"), 1);
    let mut recovery = Lease::recover_io(&receipt, &owner, io.clone(), &TestProtector).unwrap();
    assert!(recovery.claim().is_err());
    assert!(recovery.bind_delivery(&sink).is_err());
    assert!(recovery.invoke(&owner, &args()).is_err());
    assert_eq!(recovery.release().unwrap(), Released::Restored);
    assert_eq!(io.calls("action_claim_current"), 1);
    assert_eq!(io.calls("action_invoke_owned"), 0);
}
#[test]
fn lost_invoke_or_renew_revokes_without_replay_and_release_stays_compare_fenced() {
    for command in ["action_invoke_owned", "action_renew"] {
        let (sink, io, owner) = fixture(true, false);
        let mut lease = prepared(&sink, &io, &owner);
        lease.claim().unwrap();
        lease.bind_delivery(&sink).unwrap();
        io.device.lock().unwrap().lost = Some(command.into());
        let result = if command == "action_invoke_owned" {
            lease.invoke(&owner, &args())
        } else {
            lease.renew(&owner)
        };
        assert!(result.is_err());
        assert!(lease.invoke(&owner, &args()).is_err());
        assert!(lease.renew(&owner).is_err());
        assert_eq!(io.calls(command), 1);
        assert_eq!(lease.release().unwrap(), Released::Restored);
    }
}
#[test]
fn uncertain_release_is_not_reissued_by_same_adapter_and_unowned_recovery_is_read_only() {
    let (sink, io, owner) = fixture(false, false);
    let mut lease = prepared(&sink, &io, &owner);
    lease.claim().unwrap();
    let receipt = lease.receipt();
    io.device.lock().unwrap().lost = Some("action_release".into());
    assert!(lease.release().is_err());
    assert!(lease.release().is_err());
    assert_eq!(io.calls("action_release"), 1);
    let mut recovery = Lease::recover_io(&receipt, &owner, io.clone(), &TestProtector).unwrap();
    assert_eq!(recovery.release().unwrap(), Released::Preserved);
    assert_eq!(io.calls("action_release"), 1);
}
#[test]
fn missing_atomic_or_readable_capability_blocks_before_claim_and_no_legacy_fallback_is_used() {
    for field in ["fallbackPolicy", "restorePolicy"] {
        let (sink, io, owner) = fixture(false, false);
        io.device.lock().unwrap().cache["ownership"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(Lease::prepare_io(&sink, &owner, io.clone(), &TestProtector).is_err());
        assert_eq!(io.calls("action_claim_current"), 0);
    }
    let (sink, io, owner) = fixture(true, true);
    io.device.lock().unwrap().cache["actions"] = json!([descriptors()[1]]);
    assert!(Lease::prepare_io(&sink, &owner, io.clone(), &TestProtector).is_err());
    assert!(io.device.lock().unwrap().writes.is_empty());
}
#[test]
fn prepared_and_delivery_identity_contract_route_and_owner_cannot_be_substituted() {
    let (sink, io, owner) = fixture(false, false);
    for field in 0..6 {
        let mut bad = sink.clone();
        match field {
            0 => bad.preparation = None,
            1 => bad.inventory_resource_id = Some(uuid::Uuid::new_v4().to_string()),
            2 => bad.observed_port_id = Some("action:other".into()),
            3 => bad.readiness = PlanReadiness::Blocked,
            4 => bad.adapter = SinkAdapter::PhysicalState,
            _ => bad.route.as_mut().unwrap().provider_generation = "2:10:20".into(),
        };
        assert!(Lease::prepare_io(&bad, &owner, io.clone(), &TestProtector).is_err());
    }
    let mut lease = prepared(&sink, &io, &owner);
    lease.claim().unwrap();
    for field in ["access", "hash", "resource", "port", "generation"] {
        let mut bad = sink.clone();
        let token = bad.token.as_mut().unwrap();
        match field {
            "access" => token.access = Access::Read,
            "hash" => token.contract_hash = "other".into(),
            "resource" => token.resource_id = uuid::Uuid::new_v4().to_string(),
            "port" => token.port_id = "action:other".into(),
            _ => token.provider_generation = "2:10:20".into(),
        };
        assert!(lease.bind_delivery(&bad).is_err());
    }
    lease.bind_delivery(&sink).unwrap();
    let mut other = owner.clone();
    other.generation += 1;
    assert!(lease.invoke(&other, &args()).is_err());
    assert!(
        lease.invoke(&owner, &args()).is_err(),
        "wrong-owner delivery revokes the target instead of allowing later replay"
    );
    assert_eq!(io.calls("action_invoke_owned"), 0);
}
#[test]
fn stale_usb_connection_or_can_boot_configuration_epoch_blocks_before_worker() {
    for can in [false, true] {
        for field in 0..4 {
            let (sink, io, owner) = fixture(can, false);
            let mut lease = prepared(&sink, &io, &owner);
            if can {
                let mut r = io.route.lock().unwrap();
                let mut v: Vec<Value> = serde_json::from_str(&r.provider_generation).unwrap();
                v[field] = json!(999);
                r.provider_generation = serde_json::to_string(&v).unwrap();
            } else {
                let mut r = io.usb_route.lock().unwrap();
                match field {
                    0 => r.connection_generation += 1,
                    1 => r.boot += 1,
                    2 => r.configuration_generation += 1,
                    _ => r.serial = "ffffffffffffffffffffffff".into(),
                }
            }
            assert!(lease.claim().is_err());
            assert!(io.device.lock().unwrap().writes.is_empty());
        }
    }
}
#[test]
fn exact_tuple_names_type_bounds_and_order_are_checked_before_output() {
    let (sink, io, owner) = fixture(false, false);
    let mut lease = prepared(&sink, &io, &owner);
    lease.claim().unwrap();
    lease.bind_delivery(&sink).unwrap();
    for field in 0..5 {
        let mut values = args();
        match field {
            0 => values.swap(0, 1),
            1 => values[1].value = osg_profiles::PortValue::Number(101.),
            2 => values[2].value = osg_profiles::PortValue::Text("unknown".into()),
            3 => values[3].value = osg_profiles::PortValue::Text("line\nbreak".into()),
            _ => values.pop().map(|_| ()).unwrap(),
        };
        assert!(lease.invoke(&owner, &values).is_err());
    }
    assert_eq!(io.calls("action_invoke_owned"), 0);
    lease.invoke(&owner, &args()).unwrap();
}
#[test]
fn mismatched_claim_ack_and_later_owner_are_preserved_without_unsafe_renew_or_release() {
    let (sink, io, owner) = fixture(true, false);
    let mut lease = prepared(&sink, &io, &owner);
    io.device.lock().unwrap().bad_ack = Some("endpoint".into());
    assert!(lease.claim().is_err());
    assert!(lease.renew(&owner).is_err());
    assert!(lease.bind_delivery(&sink).is_err());
    assert_eq!(lease.release().unwrap(), Released::Restored);
    let (sink, io, owner) = fixture(false, false);
    let mut lease = prepared(&sink, &io, &owner);
    lease.claim().unwrap();
    {
        let mut d = io.device.lock().unwrap();
        d.id += 1;
        d.secret = "a later native owner".into();
        d.current[3] = json!("later owner");
    }
    assert!(lease.release().is_err());
    assert_eq!(io.calls("action_release"), 0);
    assert_eq!(io.device.lock().unwrap().current[3], "later owner");
}
#[test]
fn cleanup_receipts_are_strict_bounded_and_owner_pinned_and_bad_pages_never_release() {
    let (sink, io, owner) = fixture(false, false);
    let mut lease = prepared(&sink, &io, &owner);
    let receipt = lease.receipt();
    let mut other = owner.clone();
    other.profile_revision += 1;
    assert!(Lease::recover_io(&receipt, &other, io.clone(), &TestProtector).is_err());
    for field in 0..5 {
        let mut bad = receipt.clone();
        match field {
            0 => bad.kind = "other".into(),
            1 => bad.resource_id = "other".into(),
            2 => bad.state["extra"] = json!(true),
            3 => bad.state["sealedHex"] = json!("AA"),
            _ => bad.state["sealedHex"] = json!("a".repeat(8194)),
        };
        assert!(Lease::recover_io(&bad, &owner, io.clone(), &TestProtector).is_err());
    }
    lease.claim().unwrap();
    io.device.lock().unwrap().bad_page = true;
    assert!(lease.release().is_err());
    assert_eq!(io.calls("action_release"), 0);
}
#[test]
fn native_dpapi_receipt_roundtrip_is_cleanup_only() {
    #[cfg(windows)]
    {
        let (sink, io, owner) = fixture(false, false);
        let lease = Lease::prepare_io(&sink, &owner, io.clone(), &NativeProtector).unwrap();
        let receipt = lease.receipt();
        let recovered = Lease::recover_io(&receipt, &owner, io, &NativeProtector).unwrap();
        assert!(recovered.revoked);
        assert!(recovered.preparation.is_none());
        assert!(recovered.token.is_none());
    }
}

#[test]
fn portable_argument_labels_are_cosmetic_but_actual_provider_contract_stays_pinned() {
    let (mut sink, io, owner) = fixture(false, false);
    if let PortContract::Action { parameters, .. } = &mut sink.contract {
        parameters[0].name = "My cosmetic label".into();
    }
    let mut lease = prepared(&sink, &io, &owner);
    lease.claim().unwrap();
    lease.bind_delivery(&sink).unwrap();
    lease.invoke(&owner, &args()).unwrap();
    io.device.lock().unwrap().cache["actions"][0]["parameters"][0]["label"] =
        json!("New native label");
    assert!(lease.renew(&owner).is_err());
    assert_eq!(io.calls("action_renew"), 0);
}

fn advance_can_route(io: &Fake, field: usize) {
    let mut route = io.route.lock().unwrap();
    let mut generation: Vec<Value> = serde_json::from_str(&route.provider_generation).unwrap();
    generation[field] = json!(generation[field].as_u64().unwrap() + 1);
    route.provider_generation = json!(generation).to_string();
}
fn expire_action(io: &Fake) {
    let mut device = io.device.lock().unwrap();
    assert!(device.owned);
    if device.last.as_ref() == Some(&device.current) {
        device.current = device.fallback.clone();
        device.fallback_status = "applied".into();
    } else {
        device.fallback_status = "preserved".into();
    }
    device.owned = false;
    // Firmware deliberately retires the secret at TTL expiry.
    device.secret.clear();
}

#[test]
fn publication_can_lease_change_allows_only_cleanup_of_the_exact_old_action_owner() {
    let (sink, io, owner) = fixture(true, false);
    let mut lease = prepared(&sink, &io, &owner);
    assert!(lease.cleanup.can_cleanup.is_some());
    lease.claim().unwrap();
    lease.bind_delivery(&sink).unwrap();
    lease.invoke(&owner, &args()).unwrap();
    let old_target = lease.cleanup.target.clone();
    advance_can_route(&io, 2);
    assert!(old_target.cached(io.as_ref()).is_err());
    assert!(lease.invoke(&owner, &args()).is_err());
    assert!(lease.renew(&owner).is_err());
    assert!(lease.claim().is_err());
    assert_eq!(lease.release().unwrap(), Released::Restored);
    assert_eq!(
        lease.cleanup.target, old_target,
        "productive routing was never rebound"
    );
    assert_eq!(io.calls("action_claim_current"), 1);
    assert_eq!(io.calls("action_invoke_owned"), 1);
    assert_eq!(io.calls("action_renew"), 0);
    assert_eq!(io.calls("action_release"), 1);
    assert_eq!(
        io.device.lock().unwrap().current,
        lease.fallback.clone().unwrap()
    );
}

#[test]
fn recovered_active_action_can_release_after_publication_route_change_without_reclaim_or_replay() {
    let (sink, io, owner) = fixture(true, false);
    let mut lease = prepared(&sink, &io, &owner);
    let receipt = lease.receipt();
    lease.claim().unwrap();
    lease.bind_delivery(&sink).unwrap();
    lease.invoke(&owner, &args()).unwrap();
    advance_can_route(&io, 2);
    let mut recovery = Lease::recover_io(&receipt, &owner, io.clone(), &TestProtector).unwrap();
    assert!(recovery.claim().is_err());
    assert!(recovery.invoke(&owner, &args()).is_err());
    assert!(recovery.renew(&owner).is_err());
    assert_eq!(recovery.release().unwrap(), Released::Restored);
    assert_eq!(io.calls("action_claim_current"), 1);
    assert_eq!(io.calls("action_invoke_owned"), 1);
    assert_eq!(io.calls("action_release"), 1);
}

#[test]
fn in_memory_expiry_reconciles_only_exact_captured_fallback_and_stable_old_lease() {
    for changed in ["none", "value", "fallback", "lease", "status"] {
        let (sink, io, owner) = fixture(true, false);
        let mut lease = prepared(&sink, &io, &owner);
        lease.claim().unwrap();
        lease.bind_delivery(&sink).unwrap();
        lease.invoke(&owner, &args()).unwrap();
        expire_action(&io);
        advance_can_route(&io, 2);
        {
            let mut d = io.device.lock().unwrap();
            match changed {
                "value" => d.current[3] = json!("independent edit"),
                "fallback" => d.fallback[3] = json!("unrelated prior state"),
                "lease" => d.id += 1,
                "status" => d.fallback_status = "preserved".into(),
                _ => (),
            }
        }
        if changed == "none" {
            assert_eq!(lease.release().unwrap(), Released::Restored);
        } else {
            assert!(lease.release().is_err(), "{changed}");
        }
        assert_eq!(
            io.calls("action_release"),
            0,
            "retired leases are read only"
        );
        assert_eq!(io.calls("action_claim_current"), 1);
        assert_eq!(io.calls("action_invoke_owned"), 1);
    }
}

#[test]
fn recovered_retired_action_on_changed_route_stays_pending_without_secret_bound_receipt() {
    let (sink, io, owner) = fixture(true, false);
    let mut lease = prepared(&sink, &io, &owner);
    let receipt = lease.receipt();
    lease.claim().unwrap();
    expire_action(&io);
    advance_can_route(&io, 2);
    let mut recovery = Lease::recover_io(&receipt, &owner, io.clone(), &TestProtector).unwrap();
    let error = recovery.release().unwrap_err();
    assert!(error.contains("pending"));
    assert!(error.contains("authenticated retired action receipt"));
    assert_eq!(io.calls("action_release"), 0);
    assert_eq!(io.calls("action_claim_current"), 1);
    assert_eq!(io.calls("action_invoke_owned"), 0);
}

#[test]
fn changed_can_cleanup_rejects_wrong_gateway_module_epoch_boot_configuration_profile_and_catalogue()
{
    for changed in [
        "gateway",
        "module",
        "epoch",
        "boot",
        "configuration",
        "node",
        "source",
        "profile",
        "catalogue",
    ] {
        let (sink, io, owner) = fixture(true, false);
        let mut lease = prepared(&sink, &io, &owner);
        lease.claim().unwrap();
        advance_can_route(&io, 2);
        match changed {
            "gateway" => {
                io.can_identity.lock().unwrap()["gatewaySerial"] = json!("112233445566778899aabbcc")
            }
            "module" => {
                io.can_identity.lock().unwrap()["id"] = json!("4f53474411112233445566778899aabb")
            }
            "epoch" => advance_can_route(&io, 0),
            "boot" => advance_can_route(&io, 1),
            "configuration" => advance_can_route(&io, 3),
            "node" => {
                io.route
                    .lock()
                    .unwrap()
                    .attributes
                    .insert("node".into(), "51".into());
            }
            "source" => io.can_identity.lock().unwrap()["sourceNode"] = json!(51),
            "profile" => io.can_identity.lock().unwrap()["profile"]["neutral"][0] = json!(32767),
            // The selected setter is unchanged: the whole catalogue is pinned.
            _ => {
                io.device.lock().unwrap().cache["actions"][1]["label"] =
                    json!("Independent catalogue")
            }
        }
        let before = io.device.lock().unwrap().writes.len();
        assert!(lease.release().is_err(), "{changed}");
        assert_eq!(io.device.lock().unwrap().writes.len(), before, "{changed}");
        assert!(io.device.lock().unwrap().owned);
    }
}

#[test]
fn changed_route_preserves_new_owner_and_compare_fenced_external_values_without_retry() {
    for recovered in [false, true] {
        let (sink, io, owner) = fixture(true, false);
        let mut lease = prepared(&sink, &io, &owner);
        let receipt = lease.receipt();
        lease.claim().unwrap();
        advance_can_route(&io, 2);
        {
            let mut d = io.device.lock().unwrap();
            d.id += 1;
            d.secret = "new private owner".into();
            d.current[3] = json!("new owner's value");
        }
        if recovered {
            lease = Lease::recover_io(&receipt, &owner, io.clone(), &TestProtector).unwrap();
        }
        assert!(lease.release().is_err());
        assert!(lease.release().is_err());
        assert!(io.device.lock().unwrap().owned);
        assert_eq!(io.device.lock().unwrap().current[3], "new owner's value");
        assert_eq!(io.calls("action_release"), usize::from(recovered));
    }
    let (sink, io, owner) = fixture(true, false);
    let mut lease = prepared(&sink, &io, &owner);
    lease.claim().unwrap();
    lease.bind_delivery(&sink).unwrap();
    lease.invoke(&owner, &args()).unwrap();
    advance_can_route(&io, 2);
    io.device.lock().unwrap().current[3] = json!("external edit");
    assert!(
        lease.release().is_err(),
        "preserved values remain pending on changed route"
    );
    assert!(lease.release().is_err());
    assert_eq!(io.calls("action_release"), 1);
    assert_eq!(io.device.lock().unwrap().current[3], "external edit");
}

#[test]
fn receipts_without_full_can_identity_remain_strict_and_cannot_rebind_cleanup() {
    let (sink, io, owner) = fixture(true, false);
    io.can_identity.lock().unwrap()["profile"] = Value::Null;
    let mut lease = prepared(&sink, &io, &owner);
    assert!(lease.cleanup.can_cleanup.is_none());
    lease.claim().unwrap();
    advance_can_route(&io, 2);
    assert!(lease.release().is_err());
    assert_eq!(io.calls("action_release"), 0);
}

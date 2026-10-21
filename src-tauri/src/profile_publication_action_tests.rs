use super::*;
use osg_mapping::signal::{
    ActionParameter, ActionSemantics, Direction, PortContract, ScalarSchema,
};
use osg_profiles::{Binding, Endpoint, Operation, Port, ProcessingNode};

struct ActionState {
    current: bool,
    fallback: bool,
    last: bool,
    owned: bool,
    secret: String,
    lease: u32,
    status: &'static str,
    last_lease: Value,
    calls: Vec<(String, u64)>,
    held: bool,
    head: u32,
    sampled_ms: u32,
    fail_release: bool,
}
pub(super) struct ActionDevice(Mutex<ActionState>);
impl ActionDevice {
    pub(super) fn new() -> Self {
        Self(Mutex::new(ActionState {
            current: false,
            fallback: false,
            last: false,
            owned: false,
            secret: String::new(),
            lease: 0,
            status: "none",
            last_lease: Value::Null,
            calls: vec![],
            held: false,
            head: 0,
            sampled_ms: 10,
            fail_release: false,
        }))
    }
    fn cache(&self) -> Value {
        let s = self.0.lock().unwrap();
        json!({"status":"ready","catalogueRevision":7,
            "actions":[{"id":"indicator.set","label":"Set indicator","endpoint":"indicator","semantics":"setter","readable":true,
                "parameters":[{"id":"value","label":"Value","type":"boolean"}]}],
            "ownership":{"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000,"restorePolicy":"if-unchanged","fallbackPolicy":"capture-at-claim"},
            "states":[{"id":"indicator.set","values":[s.current]}],"lastLease":s.last_lease,
            "ownershipPage":{"schemaVersion":1,"catalogueRevision":7,"total":1,"next":null,
                "ownership":[{"endpoint":"indicator","actionId":"indicator.set","owned":s.owned,"leaseId":s.lease,
                    "remainingMs":if s.owned{5000}else{0},"fallback":if s.lease==0{vec![]}else{vec![s.fallback]},"fallbackStatus":s.status}]}})
    }
    pub(super) fn ports(&self) -> Vec<osg_profiles::resources_ports::PortDescriptor> {
        let mut ports = crate::profile_resource_ports::actions(&self.cache()).unwrap();
        ports.extend(crate::profile_resource_ports::events(&self.events(1)).unwrap());
        ports
    }
    fn events(&self, stream: u64) -> Value {
        let s = self.0.lock().unwrap();
        let mut held = vec![false; 8];
        held[0] = s.held;
        json!({"ok":true,"schemaVersion":1,"stream":stream,"head":s.head,"oldest":1,"overflow":0,
            "buttonCount":8,"held":held,"contactButtonCount":8,"maintainedButtons":[0,1,2,3,4,5,6,7],"encoderButtons":[],"selectorButtons":[],
            "axisCount":0,"axes":[],"axisNeutral":[],"axisUsage":[],"axisAvailable":[],"sampledAtMs":s.sampled_ms})
    }
    fn press(&self) {
        let mut s = self.0.lock().unwrap();
        assert!(!s.held);
        s.held = true;
        s.head += 1;
    }
    pub(super) fn publication_release(&self, io: &FakeIo) {
        let lease = io.cached(GATEWAY).unwrap()["modules"][0]["lease"]
            .as_u64()
            .unwrap();
        let mut s = self.0.lock().unwrap();
        assert!(!s.owned,"Physical action ownership must be restored before publication release changes its CAN route");
        s.calls.push(("publication_release".into(), lease));
    }
    pub(super) fn request(&self, io: &FakeIo, request: Value) -> Value {
        let state = io.cached(GATEWAY).unwrap();
        let module = state["modules"][0].clone();
        assert_eq!(request["gatewayId"], GATEWAY);
        assert_eq!(request["moduleId"], module["id"]);
        assert_eq!(request["boot"], module["boot"]);
        let command = request["command"].as_str().unwrap();
        let route = module["lease"].as_u64().unwrap();
        {
            let mut s = self.0.lock().unwrap();
            s.calls.push((command.into(), route));
            let owned =
                s.owned && request["ownershipToken"] == s.secret && request["leaseId"] == s.lease;
            match command {
                "action_state" | "action_ownership" => (),
                // Device samples advance independently of transition history.
                "events_state" | "events_read" => s.sampled_ms += 10,
                "action_claim_current" => {
                    assert_eq!(request["actionId"], "indicator.set");
                    assert_eq!(request["catalogueRevision"], 7);
                    if s.owned {
                        return json!({"ok":false,"error":"outputBusy"});
                    }
                    s.lease += 1;
                    s.secret = request["ownershipToken"].as_str().unwrap().into();
                    s.fallback = s.current;
                    s.last = s.current;
                    s.owned = true;
                    s.status = "none";
                    s.last_lease =
                        json!({"leaseId":s.lease,"endpoint":"indicator","remainingMs":5000});
                }
                "action_invoke_owned" => {
                    if !owned {
                        return json!({"ok":false,"error":"outputNotOwned"});
                    }
                    assert_eq!(request["actionId"], "indicator.set");
                    s.current = request["values"][0].as_bool().unwrap();
                    s.last = s.current;
                }
                "action_renew" => {
                    if !owned {
                        return json!({"ok":false,"error":"outputNotOwned"});
                    }
                    s.last_lease =
                        json!({"leaseId":s.lease,"endpoint":"indicator","remainingMs":5000});
                }
                "action_release" => {
                    if !owned {
                        return json!({"ok":false,"error":"outputNotOwned"});
                    }
                    if s.fail_release {
                        return json!({"ok":false,"error":"The isolated action release failed"});
                    }
                    let applied = s.current == s.last;
                    if applied {
                        s.current = s.fallback;
                    }
                    s.owned = false;
                    s.secret.clear();
                    s.status = if applied { "applied" } else { "preserved" };
                    s.last_lease = json!({"leaseId":s.lease,"endpoint":"indicator","released":true,"fallbackStatus":s.status});
                }
                other => panic!("Unexpected composed physical action command {other}"),
            }
        }
        let mut reply = cached(io, Some(self));
        if command == "events_read" {
            let after = request["after"].as_u64().unwrap();
            let id = module["id"].as_str().unwrap();
            let events = &mut reply["moduleEvents"][id];
            let head = events["head"].as_u64().unwrap();
            events["after"] = json!(after);
            events["events"] = if after < head {
                json!([{"sequence":head,"control":0,"pressed":true}])
            } else {
                json!([])
            };
            events["next"] = json!(head);
            events["more"] = json!(false);
        }
        json!({"ok":true,"state":reply})
    }
    fn count(&self, command: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(c, _)| c == command)
            .count()
    }
    fn expire(&self) {
        let mut s = self.0.lock().unwrap();
        assert!(s.owned);
        if s.current == s.last {
            s.current = s.fallback;
            s.status = "applied";
        } else {
            s.status = "preserved";
        }
        s.owned = false;
        s.secret.clear();
    }
}
pub(super) fn cached(io: &FakeIo, actions: Option<&ActionDevice>) -> Value {
    let mut state = io.cached(GATEWAY).unwrap();
    if let Some(actions) = actions {
        state["backend"] = json!("stm32");
        let module = state["modules"][0].clone();
        let id = module["id"].as_str().unwrap().to_owned();
        let mut cache = actions.cache();
        cache["route"] = json!([
            state["epoch"],
            module["node"],
            module["lease"],
            module["boot"]
        ]);
        cache["configurationGeneration"] = module["configurationGeneration"].clone();
        state["moduleActions"][&id] = cache;
        let mut events = actions.events(module["lease"].as_u64().unwrap());
        events["route"] = json!([
            state["epoch"],
            module["node"],
            module["lease"],
            module["boot"]
        ]);
        events["configurationGeneration"] = module["configurationGeneration"].clone();
        state["moduleEvents"][&id] = events;
    }
    state
}
fn port(id: &str, direction: Direction, contract: PortContract) -> Port {
    Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract,
    }
}
fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
impl Fixture {
    pub(super) fn action_candidate(&self) -> Candidate {
        let mut candidate = self.candidate(Some(false));
        let native = self.actions.as_ref().unwrap().ports().remove(0);
        let logical = native.contract.clone();
        let button = self
            .actions
            .as_ref()
            .unwrap()
            .ports()
            .into_iter()
            .find(|p| p.id == "button:0")
            .unwrap();
        candidate.document.resources[0].ports = vec![
            port("write", Direction::Input, logical),
            port("button", Direction::Output, button.contract.clone()),
        ];
        let state = button.contract.clone();
        let parameters = vec![ActionParameter {
            id: "value".into(),
            name: "Value".into(),
            schema: ScalarSchema::Boolean {},
        }];
        candidate.document.processing_nodes = vec![ProcessingNode { draft: None,
            id: "pack".into(),
            name: "Explicit setter".into(),
            operation: Operation::SetOnChange {
                arguments: vec!["value".into()],
            },
            inputs: vec![port("value", Direction::Input, state)],
            outputs: vec![port(
                "invoke",
                Direction::Output,
                PortContract::Invocation {
                    parameters,
                    semantics: ActionSemantics::Setter,
                },
            )],
        }];
        candidate.document.bindings = vec![
            Binding {
                id: "value".into(),
                from: endpoint("controller-0", "button"),
                to: endpoint("pack", "value"),
            },
            Binding {
                id: "write".into(),
                from: endpoint("pack", "invoke"),
                to: endpoint("controller-0", "write"),
            },
        ];
        candidate.assignments[0].ports = vec![osg_profiles::resources::PortExpectation {
            profile_port_id: "write".into(),
            observed_port_id: native.id.clone(),
            expected_contract_hash: native.contract_hash().unwrap(),
        }];
        candidate.assignments[0]
            .ports
            .push(osg_profiles::resources::PortExpectation {
                profile_port_id: "button".into(),
                observed_port_id: button.id.clone(),
                expected_contract_hash: button.contract_hash().unwrap(),
            });
        osg_profiles::validate_document(&candidate.document).unwrap();
        let (snapshot, pins) = self
            .resources
            .snapshot_for_preparation(candidate.assignments.clone())
            .unwrap();
        let report =
            osg_profiles::compiler::compile_for_preparation(&candidate.document, &snapshot, &pins);
        assert_eq!(
            report.plan.as_ref().unwrap().sinks[0].readiness,
            osg_profiles::compiler_models::PlanReadiness::Ready,
            "{:?}",
            report.diagnostics
        );
        candidate.stage = Some(
            NativeProfileHost::stage_with_handoff(
                &self.resources,
                &self.games,
                &self.companions,
                self.controllers.clone(),
                false,
                &candidate,
                self.coordinator.visibility_handoff_source().unwrap(),
            )
            .unwrap(),
        );
        candidate
    }
    fn drive_action(&mut self) -> Arc<ActionDevice> {
        let actions = self.actions.as_ref().unwrap().clone();
        let deadline = Instant::now() + Duration::from_secs(2);
        // Start may still be binding the live reader after its acquisition
        // snapshot. A control change belongs after that fresh live baseline.
        let reads = actions.count("events_read");
        while actions.count("events_read") < reads + 2 {
            self.coordinator.poll().unwrap();
            assert!(
                Instant::now() < deadline,
                "The actual native input reader did not start"
            );
            thread::sleep(Duration::from_millis(5));
        }
        self.coordinator.poll().unwrap();
        let request = crate::profile_session::inspection::Request::parse(&json!({"command":"profile_runtime_inspect","owner":self.coordinator.status().owner,"nodeId":"controller-0","offset":0,"limit":64})).unwrap();
        let baseline = serde_json::to_value(self.coordinator.inspect(&request).unwrap()).unwrap();
        let button = baseline["ports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["endpoint"]["portId"] == "button")
            .unwrap();
        assert_eq!(
            button["sample"],
            json!({"status":"known","value":{"kind":"scalar","value":false}}),
            "{baseline}"
        );
        assert_eq!(
            self.coordinator.status().state,
            State::Running,
            "{:?}; baseline={baseline}",
            self.coordinator.status()
        );
        actions.press();
        while !actions.0.lock().unwrap().current {
            self.coordinator.poll().unwrap();
            let calls = actions.0.lock().unwrap().calls.clone();
            assert!(
                Instant::now() < deadline,
                "The actual Host did not dispatch its authored setter: {:?}; calls={:?}",
                self.coordinator.status(),
                calls
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(actions.count("action_invoke_owned") > 0);
        assert!(actions.0.lock().unwrap().current);
        actions
    }
}

#[test]
fn actual_host_releases_physical_action_before_publication_changes_its_can_route() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::with_actions(true);
    let candidate = f.action_candidate();
    f.coordinator.apply(candidate).unwrap();
    assert_eq!(f.coordinator.status().state, State::Running);
    let actions = f.drive_action();
    assert_eq!(actions.count("action_claim_current"), 1);
    let acquired_route = f.io.cached(GATEWAY).unwrap()["modules"][0]["lease"]
        .as_u64()
        .unwrap();
    assert_eq!(
        acquired_route, 67,
        "Action acquisition follows the publication transition and receipt refresh"
    );
    f.coordinator.stop().unwrap();
    assert_eq!(f.coordinator.status().state, State::Stopped);
    let s = actions.0.lock().unwrap();
    let action = s
        .calls
        .iter()
        .position(|(c, _)| c == "action_release")
        .unwrap();
    let publication = s
        .calls
        .iter()
        .position(|(c, _)| c == "publication_release")
        .unwrap();
    assert!(action < publication);
    assert_eq!(s.calls[action].1, 67);
    assert_eq!(s.calls[publication].1, 67);
    assert!(!s.owned);
    assert!(!s.current);
    drop(s);
    assert_eq!(f.io.cached(GATEWAY).unwrap()["modules"][0]["lease"], 68);
    assert_eq!(actions.count("action_claim_current"), 1);
    assert_eq!(actions.count("action_release"), 1);
}

#[test]
fn actual_host_reconciles_in_memory_action_expiry_after_autonomous_publication_route_change() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::with_actions(true);
    let candidate = f.action_candidate();
    f.coordinator.apply(candidate).unwrap();
    let actions = f.drive_action();
    actions.expire();
    // Both firmware leases can expire while the host is unavailable. Preserve
    // exact identities while publication restoration advances only the CAN lease.
    f.io.change(|s| {
        s.state["publication"]["phase"] = json!("idle");
        s.state["publication"]["outcome"] = json!("no_change");
        s.state["publication"]["transition"] = json!("2");
        s.state["publicationRevision"] = json!(19);
        s.state["modules"][0]["published"] = json!(true);
        s.state["modules"][0]["lease"] = json!(68);
    });
    f.coordinator.stop().unwrap();
    assert_eq!(f.coordinator.status().state, State::Stopped);
    assert_eq!(actions.count("action_claim_current"), 1);
    assert_eq!(actions.count("action_release"), 0);
    assert_eq!(f.io.count("claim"), 1);
    assert_eq!(f.io.count("release"), 0);
    assert!(!actions.0.lock().unwrap().current);
}

#[test]
fn actual_host_stop_and_default_retain_foreign_or_independently_preserved_publication() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for foreign in [false, true] {
        for use_default in [false, true] {
            let mut f = Fixture::new();
            let candidate = f.candidate(Some(false));
            f.coordinator.apply(candidate).unwrap();
            f.io.change(|s| {
                if foreign {
                    s.owner = Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into());
                    s.state["publication"]["leaseId"] = json!(74);
                    s.state["publication"]["claimRevision"] = json!(18);
                } else {
                    s.state["publication"]["phase"] = json!("idle");
                    s.state["publication"]["outcome"] = json!("preserved");
                }
            });
            let result = if use_default {
                let default = f.candidate(None);
                f.coordinator.apply(default)
            } else {
                f.coordinator.stop()
            };
            assert!(result.is_err(), "foreign={foreign}, default={use_default}");
            assert_eq!(f.coordinator.status().state, State::RecoveryNeeded);
            assert_eq!(
                f.io.count("release"),
                0,
                "An unrelated publication must remain untouched"
            );
            assert_eq!(
                f.io.cached(GATEWAY).unwrap()["modules"][0]["published"],
                false
            );
            let journal: Value = serde_json::from_slice(
                &std::fs::read(f._directory.path().join("profile-session.json")).unwrap(),
            )
            .unwrap();
            assert!(
                !journal["pending"].is_null() || !journal["active"].is_null(),
                "The exact cleanup intent remains durable"
            );
        }
    }
}

#[test]
fn failed_pre_acquire_recapture_save_discards_only_inert_actions_and_restores_owned_publication() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::with_actions(true);
    let candidate = f.action_candidate();
    let actions = f.actions.as_ref().unwrap().clone();
    f.coordinator
        .fail_save_when_attempting(crate::profile_session::ApplyPhase::Acquire);
    let error = f.coordinator.apply(candidate).unwrap_err();
    assert!(
        error.contains("Injected profile journal failure"),
        "{error}"
    );
    assert!(
        !error.contains("Physical cleanup receipts changed"),
        "{error}"
    );
    assert_eq!(f.coordinator.status().state, State::RecoveryNeeded);
    assert_eq!(f.io.count("claim"), 1);
    assert_eq!(f.io.count("release"), 1, "The committed Overrides phase must restore despite the uncommitted replacement action receipt");
    assert_eq!(
        f.io.cached(GATEWAY).unwrap()["publication"]["phase"],
        "idle"
    );
    assert_eq!(
        f.io.cached(GATEWAY).unwrap()["modules"][0]["published"],
        true
    );
    for command in [
        "action_claim_current",
        "action_invoke_owned",
        "action_renew",
        "action_release",
    ] {
        assert_eq!(
            actions.count(command),
            0,
            "Unattempted Acquire dispatched {command}"
        );
    }
    let journal: Value = serde_json::from_slice(
        &std::fs::read(f._directory.path().join("profile-session.json")).unwrap(),
    )
    .unwrap();
    assert!(journal["active"].is_null() && journal["pending"].is_null());
    f.coordinator.recover().unwrap();
    assert_eq!(f.coordinator.status().state, State::Stopped);
    assert_eq!(f.io.count("claim"), 1);
    assert_eq!(f.io.count("release"), 1);
    assert_eq!(actions.count("action_claim_current"), 0);
}

#[test]
fn failed_pre_overrides_recapture_save_preserves_completed_outgoing_cleanup_without_incoming_claims(
) {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::with_actions(true);
    let first = f.action_candidate();
    f.coordinator.apply(first).unwrap();
    let actions = f.drive_action();
    let invokes = actions.count("action_invoke_owned");
    let incoming = f.action_candidate();
    f.coordinator
        .fail_save_when_attempting(crate::profile_session::ApplyPhase::Overrides);
    let error = f.coordinator.apply(incoming).unwrap_err();
    assert!(
        error.contains("Injected profile journal failure"),
        "{error}"
    );
    assert!(
        !error.contains("Physical cleanup receipts changed"),
        "{error}"
    );
    assert_eq!(f.coordinator.status().state, State::RecoveryNeeded);
    assert_eq!(
        f.io.count("claim"),
        1,
        "The incoming uncommitted Overrides phase cannot claim"
    );
    assert_eq!(f.io.count("release"), 1);
    assert_eq!(actions.count("action_claim_current"), 1);
    assert_eq!(actions.count("action_invoke_owned"), invokes);
    assert_eq!(actions.count("action_release"), 1);
    let state = actions.0.lock().unwrap();
    assert!(!state.owned && !state.current);
    let released = state
        .calls
        .iter()
        .position(|(kind, _)| kind == "action_release")
        .unwrap();
    let publication = state
        .calls
        .iter()
        .position(|(kind, _)| kind == "publication_release")
        .unwrap();
    assert!(released < publication);
    drop(state);
    assert_eq!(
        f.io.cached(GATEWAY).unwrap()["modules"][0]["published"],
        true
    );
    let journal: Value = serde_json::from_slice(
        &std::fs::read(f._directory.path().join("profile-session.json")).unwrap(),
    )
    .unwrap();
    assert!(journal["active"].is_null() && journal["pending"].is_null());
    f.coordinator.recover().unwrap();
    assert_eq!(f.coordinator.status().state, State::Stopped);
    assert_eq!(f.io.count("claim"), 1);
    assert_eq!(actions.count("action_claim_current"), 1);
    assert_eq!(actions.count("action_invoke_owned"), invokes);
}

#[test]
fn unresolved_action_cleanup_revokes_publication_renewal_without_advancing_the_can_route() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::with_actions(true);
    let candidate = f.action_candidate();
    f.coordinator.apply(candidate).unwrap();
    let actions = f.drive_action();
    let invokes = actions.count("action_invoke_owned");
    let action_lease = actions.0.lock().unwrap().lease;
    actions.0.lock().unwrap().fail_release = true;
    assert!(f.coordinator.stop().is_err());
    assert_eq!(f.coordinator.status().state, State::RecoveryNeeded);
    assert_eq!(actions.count("action_release"), 1);
    for _ in 0..3 {
        assert!(f.coordinator.stop().is_err());
        assert_eq!(
            actions.count("action_release"),
            1,
            "Ordinary neutralization cannot retry an uncertain action Release"
        );
        assert_eq!(f.io.count("release"), 0);
    }
    let before = f.io.cached(GATEWAY).unwrap();
    let expires = before["publication"]["expiresAtMs"].clone();
    let lease = before["modules"][0]["lease"].clone();
    let calls = f.io.0.lock().unwrap().calls.len();
    assert_eq!(before["publication"]["phase"], "owned");
    assert_eq!(f.io.count("release"), 0);
    assert!(actions.0.lock().unwrap().owned);
    // Cross a full normal renewal interval after the failed cleanup, while the
    // native control epoch stays unchanged. Only neutralize's run latch fences it.
    thread::sleep(Duration::from_millis(1200));
    let after = f.io.cached(GATEWAY).unwrap();
    assert_eq!(after["publication"]["expiresAtMs"], expires);
    assert_eq!(after["modules"][0]["lease"], lease);
    assert_eq!(f.io.count("release"), 0);
    assert!(f.io.0.lock().unwrap().calls[calls..]
        .iter()
        .filter(|(kind, _)| *kind == "renew")
        .all(|(_, admitted)| !admitted));
    let journal: Value = serde_json::from_slice(
        &std::fs::read(f._directory.path().join("profile-session.json")).unwrap(),
    )
    .unwrap();
    assert!(!journal["active"].is_null() || !journal["pending"].is_null());
    assert_eq!(f.io.count("claim"), 1);
    assert_eq!(actions.count("action_claim_current"), 1);
    assert_eq!(actions.count("action_invoke_owned"), invokes);

    // Only explicit Recover may reconstruct exact cleanup from the durable seal.
    actions.0.lock().unwrap().fail_release = false;
    f.coordinator.recover().unwrap();
    assert_eq!(f.coordinator.status().state, State::Stopped);
    assert_eq!(actions.count("action_release"), 2);
    assert_eq!(actions.count("action_claim_current"), 1);
    assert_eq!(actions.count("action_invoke_owned"), invokes);
    assert_eq!(f.io.count("claim"), 1);
    assert_eq!(f.io.count("release"), 1);
    let state = actions.0.lock().unwrap();
    assert_eq!(state.lease, action_lease);
    assert!(!state.owned && !state.current);
    let restored_action = state
        .calls
        .iter()
        .rposition(|(kind, _)| kind == "action_release")
        .unwrap();
    let released_publication = state
        .calls
        .iter()
        .position(|(kind, _)| kind == "publication_release")
        .unwrap();
    assert!(restored_action < released_publication);
    assert!(state
        .calls
        .iter()
        .filter(|(kind, _)| kind == "action_release")
        .all(|(_, route)| json!(route) == lease));
    drop(state);
    let restored = f.io.cached(GATEWAY).unwrap();
    assert_eq!(restored["publication"]["phase"], "idle");
    assert_eq!(restored["modules"][0]["published"], true);
    assert_eq!(
        restored["modules"][0]["lease"].as_u64().unwrap(),
        lease.as_u64().unwrap() + 1
    );
    let journal: Value = serde_json::from_slice(
        &std::fs::read(f._directory.path().join("profile-session.json")).unwrap(),
    )
    .unwrap();
    assert!(journal["active"].is_null() && journal["pending"].is_null());
}

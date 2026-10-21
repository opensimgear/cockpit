use super::*;
use crate::game_discovery::{Platform, ProcessIdentity};
use osg_profiles::{resources::*, resources_ports::*, store::LocalAssignment, *};
use std::sync::Mutex;

struct NoGame;
impl Platform for NoGame {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<serde_json::Value> { vec![] }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> { Err("No game".into()) }
    fn launch(&self, _: &serde_json::Value) -> Result<ProcessIdentity, String> { Err("No game".into()) }
    fn find(&self, _: &serde_json::Value) -> Result<Option<ProcessIdentity>, String> { Ok(None) }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> { Ok(false) }
}
fn contract() -> PortContract { PortContract::State { schema: ScalarSchema::Boolean {}, freshness_ms: 1000, fallback: None } }
fn port(id: &str, direction: Direction) -> Port { Port { id: id.into(), name: id.into(), direction, contract: contract() } }
fn observed(online: bool, changed: bool, unrelated: bool) -> Observation {
    let mut descriptor = PortDescriptor { id: "button:0".into(), name: "Button".into(), direction: Direction::Output, contract: contract() };
    if changed { descriptor.contract = PortContract::State { schema: ScalarSchema::Boolean {}, freshness_ms: 2000, fallback: None }; }
    let physical = if unrelated { "22222222-2222-4222-8222-222222222222" } else { "11111111-1111-4111-8111-111111111111" };
    Observation { key: physical.into(), name: "Flight controls".into(), resource_type: ResourceType::HostController,
        evidence: vec![IdentityEvidence { kind: EvidenceKind::LocalPath, value: format!("host:{physical}") }], attributes: BTreeMap::new(), diagnostics: vec![],
        route: online.then(|| Route { id: format!("hid:{physical}"), transport: Transport::Hid, provider_generation: "1".into(), attributes: BTreeMap::new(), baseline_ready: true, writable: false, priority: 100 }),
        ports: Some(PortUpdate::complete(vec![descriptor]).unwrap()) }
}
struct Fixture {
    _directory: tempfile::TempDir,
    feed: Arc<Mutex<(bool, bool, bool)>>,
    resources: Arc<crate::profile_resources::Runtime>,
    controllers: Arc<osg_controller_input::ControllerService>,
    games: Arc<crate::games::GamesRuntime>,
    companions: Arc<crate::companion_host::Runtime>,
    epoch: Arc<AtomicU64>,
    candidate: Candidate,
}
impl Fixture {
    fn new() -> Self {
        let directory = crate::test_directory();
        let feed = Arc::new(Mutex::new((false, false, false)));
        let collect = feed.clone();
        let resources = Arc::new(crate::profile_resources::Runtime::new(None, Arc::new(move |revision| {
            let (online, changed, unrelated) = *collect.lock().unwrap();
            // This scope enumerates saved controller identities, including
            // unplugged ones. An unrelated device never removes that saved row.
            let observations = if unrelated { vec![observed(false, false, false), observed(online, changed, true)] } else { vec![observed(online, changed, false)] };
            vec![("host-controllers".into(), Ok(ProviderBatch { scope: "host-controllers".into(), session: "native-offline-fixture".into(), revision, complete: true, observations }))]
        }), false));
        resources.refresh_native().unwrap();
        let source = resources.request(&json!({"command":"profile_resources_list"})).unwrap()["resources"][0]["id"].as_str().unwrap().to_owned();
        let descriptor = observed(false, false, false).ports.unwrap().ports.remove(0);
        let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
        let metadata = directory.path().join("games.json");
        let games = Arc::new(crate::games::GamesRuntime::new(Arc::new(|_| json!({"ok":true,"state":{"integration":{"running":false},"dcsBios":{"running":false}}})),
            crate::games::Paths { installations: metadata.with_extension("installations.json"), provider_setup: metadata.with_extension("setup.json"), recovery: metadata.with_extension("recovery.json") }, Arc::new(NoGame)));
        let companions = Arc::new(crate::companion_host::Runtime::new(None, Arc::new(osg_mapping_host::companions::CompanionProvider::new(Arc::new(|| 0)))));
        let epoch = Arc::new(AtomicU64::new(1));
        let mut document = new_document("Offline native peripheral").unwrap();
        document.resources.push(Resource { id: "controls".into(), name: "Flight controls".into(), kind: ResourceKind::Device, required: true, ports: vec![port("pressed", Direction::Output)] });
        document.processing_nodes = vec![
            ProcessingNode { id: "read-controls".into(), name: "Read controls".into(), operation: Operation::Identity {}, inputs: vec![port("value", Direction::Input)], outputs: vec![port("output", Direction::Output)] },
            ProcessingNode { id: "base".into(), name: "Always active Base".into(), operation: Operation::Constant { value: PortValue::Boolean(true) }, inputs: vec![], outputs: vec![port("value", Direction::Output)] },
        ];
        document.bindings.push(Binding { id: "read-input".into(), from: Endpoint { node_id: "controls".into(), port_id: "pressed".into() }, to: Endpoint { node_id: "read-controls".into(), port_id: "value".into() } });
        let candidate = Candidate { launch_preparation: false, document, assignments: vec![LocalAssignment { resource_id: "controls".into(), source_id: source, ports: vec![PortExpectation { profile_port_id: "pressed".into(), observed_port_id: descriptor.id.clone(), expected_contract_hash: descriptor.contract_hash().unwrap() }] }], read_only: false, game_id: None, admission_epoch: Some(1), stage: None };
        Self { _directory: directory, feed, resources, controllers, games, companions, epoch, candidate }
    }
    fn host(&self) -> NativeProfileHost { NativeProfileHost::new(self.resources.clone(), self.controllers.clone(), self.games.clone(), self.companions.clone(), Arc::new(crate::gateways::ProfilePanelIo::empty()), false, self.epoch.clone()) }
    fn staged(&self) -> Candidate {
        let mut candidate = self.candidate.clone();
        candidate.stage = Some(NativeProfileHost::stage(&self.resources, &self.games, &self.companions, self.controllers.clone(), false, &candidate).unwrap());
        candidate
    }
}
impl Drop for Fixture { fn drop(&mut self) { self.resources.shutdown(); self.games.shutdown(); self.companions.shutdown(); self.controllers.shutdown(); } }

#[test]
fn required_offline_input_activates_degraded_and_keeps_base_and_health_running() {
    let fixture = Fixture::new();
    let mut coordinator = crate::profile_session::Coordinator::new(fixture.host(), None);
    coordinator.apply(fixture.staged()).unwrap();
    let status = coordinator.status().clone();
    assert_eq!(status.state, crate::profile_session::State::Degraded);
    assert!(status.degradation.iter().any(|message| message.contains("Flight controls") && message.contains("offline")));
    let owner = status.owner.unwrap();
    for _ in 0..3 {
        coordinator.poll().unwrap();
        assert_eq!(coordinator.status().state, crate::profile_session::State::Degraded);
        assert_eq!(coordinator.status().owner.as_ref(), Some(&owner));
    }
    let request = crate::profile_session::inspection::Request::parse(&json!({"command":"profile_runtime_inspect","owner":owner,"nodeId":"base"})).unwrap();
    let inspection = serde_json::to_value(coordinator.inspect(&request).unwrap()).unwrap();
    assert!(inspection["ports"].as_array().unwrap().iter().any(|port| port["sample"] == serde_json::to_value(osg_mapping::program::SignalSample::scalar(PortValue::Boolean(true))).unwrap()));
    assert!(coordinator.peripheral_change().is_none());
    coordinator.stop().unwrap();
    assert_eq!(coordinator.status().state, crate::profile_session::State::Stopped);
}

#[test]
fn unused_offline_peripheral_still_has_a_named_persistent_degradation() {
    let mut fixture = Fixture::new();
    fixture.candidate.document.resources[0].ports.clear();
    fixture.candidate.assignments[0].ports.clear();
    fixture.candidate.document.bindings.clear();
    fixture.candidate.document.processing_nodes.retain(|node| node.id == "base");
    let mut coordinator = crate::profile_session::Coordinator::new(fixture.host(), None);
    coordinator.apply(fixture.staged()).unwrap();
    coordinator.poll().unwrap();
    assert_eq!(coordinator.status().state, crate::profile_session::State::Degraded);
    assert_eq!(coordinator.status().degradation, vec!["Flight controls is offline; its settings and peripheral controls are inactive."]);
    assert!(coordinator.status().owner.is_some());
    coordinator.stop().unwrap();
}

#[test]
fn reconnect_trigger_requires_the_assigned_native_identity_and_contract_and_is_stable() {
    let fixture = Fixture::new();
    let mut coordinator = crate::profile_session::Coordinator::new(fixture.host(), None);
    coordinator.apply(fixture.staged()).unwrap();
    assert!(coordinator.peripheral_change().is_none());
    *fixture.feed.lock().unwrap() = (true, false, true);
    fixture.resources.refresh_native().unwrap();
    assert!(coordinator.peripheral_change().is_none(), "An unrelated online controller cannot activate this branch");
    *fixture.feed.lock().unwrap() = (true, true, false);
    fixture.resources.refresh_native().unwrap();
    assert!(coordinator.peripheral_change().is_none(), "Changed exact native contracts still require reconciliation");
    *fixture.feed.lock().unwrap() = (true, false, false);
    fixture.resources.refresh_native().unwrap();
    let changed = coordinator.peripheral_change();
    assert!(changed.is_some(), "Matching returned hardware needs a fresh takeover: {:?}", fixture.resources.snapshot_for_preparation(fixture.candidate.assignments.clone()).map(|(snapshot,pins)| (serde_json::to_value(snapshot).unwrap(),pins.len())));
    let change = changed.unwrap();
    fixture.resources.refresh_native().unwrap();
    assert_eq!(coordinator.peripheral_change().unwrap(), change, "Polling and fresh preparation nonces cannot retrigger changed identity");
    let mut fresh = fixture.host();
    let candidate = fixture.staged();
    let owner = RunOwner { session: uuid::Uuid::new_v4().to_string(), generation: 1, profile_id: candidate.document.id.clone(), profile_revision: candidate.document.revision, game_id: None };
    let prepared = fresh.prepare(&candidate, &owner).unwrap();
    assert!(prepared.admission.optional_unavailable.is_empty());
    assert!(fresh.peripheral_change(&owner).is_none(), "A freshly admitted run must not repeatedly reclaim the same peripheral");
    fresh.discard_preparation(&owner);
    coordinator.stop().unwrap();
}

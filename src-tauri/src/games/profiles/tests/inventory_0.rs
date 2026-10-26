use super::*;
use osg_profiles::resources::{Access, Registry};
const MODULE: &str = "4f534744123456789012345678901234";
struct ClosedGameCatalogue;
impl crate::game_discovery::Platform for ClosedGameCatalogue {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        ["dcs","assetto-corsa","assetto-corsa-competizione","assetto-corsa-evo","assetto-corsa-rally","x-plane-11","x-plane-12"].into_iter()
            .map(|id|json!({"id":id,"name":id,"integrationKey":id,"provider":crate::game_discovery::integration_provider(id),"integrationCompatible":false,"integrationAvailable":true,"installed":false,"processMonitoringSupported":false,"launchSupported":false,"executable":null})).collect()
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        panic!("Closed catalogue must not validate a process")
    }
    fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
        panic!("Closed catalogue must not launch")
    }
    fn find(&self, _: &Value) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
        panic!("Closed catalogue must not scan")
    }
    fn alive(&self, _: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        panic!("Closed catalogue must not inspect processes")
    }
}
fn sources() -> (Sources, tempfile::TempDir) {
    let temporary = {
        let parent = std::env::temp_dir();
        #[cfg(unix)]
        let parent = parent.canonicalize().unwrap();
        tempfile::tempdir_in(parent).unwrap()
    };
    let panel: Panel = Arc::new(|_| {
        panic!("Read-only inventory must not issue transport requests in this fixture")
    });
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let companions = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
        Arc::new(|| 0),
    ));
    let host = Arc::new(crate::companion_host::Runtime::new(
        None,
        companions.clone(),
    ));
    let games = Arc::new(crate::games::GamesRuntime::test_runtime(
        panel.clone(),
        {
            let fixture_metadata_path = temporary.path().join("games.json");
            crate::games::Paths {
                installations: fixture_metadata_path.with_extension("installations-v1.json"),
                provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                recovery: fixture_metadata_path.with_extension("recovery.json"),
            }
        },
        Arc::new(ClosedGameCatalogue),
    ));
    (
        Sources {
            session: "provider-session".into(),
            controllers,
            companions,
            host,
            games,
            gateways: Arc::new(|| vec![]),
            panel,
            native: false,
            usb: Arc::new(crate::module_usb_fleet::Fleet::new(
                Arc::new(|| panic!("No serial enumeration in a synthetic provider")),
                false,
            )),
            can_scopes: BTreeSet::new(),
            can_retained: BTreeMap::new(),
            refreshing: Arc::new(Mutex::new(BTreeSet::new())),
            catalogues: BTreeMap::new(),
        },
        temporary,
    )
}
fn can_state() -> Value {
    json!({"connected":true,"epoch":1,"canProtocol":3,"modules":[{"id":MODULE,"name":"OSG module","online":true,"boot":100,"node":1,"lease":2,"configurationGeneration":1,"canConfigure":true}],
            "moduleEvents":{MODULE:{"ok":true,"schemaVersion":1,"stream":7,"head":0,"oldest":1,"overflow":0,"buttonCount":1,"held":[false],"encoderButtons":[],"route":[1,1,2,100],"configurationGeneration":1}},
            "moduleActions":{MODULE:{"status":"unsupported","route":[1,1,2,100],"configurationGeneration":1}}})
}
#[test]
fn bounded_publication_metadata_keeps_ordinary_input_token_valid_when_publication_changes() {
    let (sources, _temporary) = sources();
    let gateway = "stm32:0123456789abcdef01234567";
    let mut state = can_state();
    state["gatewaySerial"] = json!("0123456789abcdef01234567");
    state["layoutPending"] = json!(false);
    state["nativeSnapshotAtMs"] = json!(hid_publication::now_ms());
    state["publication"] = json!({"phase":"idle","outcome":"no_change"});
    state["modules"][0]["published"] = json!(false);
    state["modules"][0]["directUsb"] = json!(false);
    let mut registry = Registry::new();
    let observe = |state: &Value| {
        sources
            .can_observations(gateway, state, &BTreeMap::new())
            .unwrap()
    };
    registry
        .reconcile(batch(
            &format!("gateway:{gateway}"),
            "native-fixture",
            1,
            observe(&state),
        ))
        .unwrap();
    let before = registry.list(0, 32).unwrap().resources.remove(0);
    let token = registry
        .token(&before.id, "button:0", Access::Read)
        .unwrap();
    let key = osg_profiles::compiler::hid_policy::attribute(&before.connections[0].route.id);
    assert_eq!(key.len(), 71);
    let proof: Value = serde_json::from_str(&before.attributes[&key]).unwrap();
    assert_eq!(proof["status"], "disabled");
    state["modules"][0]["published"] = json!(true);
    registry
        .reconcile(batch(
            &format!("gateway:{gateway}"),
            "native-fixture",
            2,
            observe(&state),
        ))
        .unwrap();
    let after = registry.inspect(&before.id, 0, 32).unwrap().resource;
    assert_eq!(before.generation, after.generation);
    registry
        .validate_token(&token, "button:0", Access::Read)
        .unwrap();
    let proof: Value = serde_json::from_str(&after.attributes[&key]).unwrap();
    assert_eq!(proof["status"], "published");
}

#[test]
fn inventory_retains_distinct_gateway_modules_and_closed_game_ports_without_effects() {
    let (mut sources, _temporary) = sources();
    sources.gateways = Arc::new(|| {
        vec![
            ("stm32:aaaaaaaaaaaaaaaaaaaaaaaa".into(), can_state()),
            ("stm32:bbbbbbbbbbbbbbbbbbbbbbbb".into(), can_state()),
        ]
    });
    let mut registry = Registry::new();
    for (_, batch) in sources.collect(1) {
        registry.reconcile(batch.unwrap()).unwrap();
    }
    let page = registry.list(0, 32).unwrap();
    let modules = page
        .resources
        .iter()
        .filter(|r| r.resource_type == ResourceType::OsgModule)
        .collect::<Vec<_>>();
    assert_eq!(modules.len(), 2);
    assert_ne!(modules[0].id, modules[1].id);
    assert!(modules.iter().all(|r| r.port_count == 2)); // state and ordered events; no hats
    let first = modules[0].id.clone();
    let token = registry.token(&first, "button:0", Access::Read).unwrap();
    let integrations = page
        .resources
        .iter()
        .filter(|r| r.resource_type == ResourceType::GameIntegration)
        .collect::<Vec<_>>();
    assert_eq!(integrations.len(), 7);
    assert!(integrations
        .iter()
        .filter(|r| r.attributes["provider"] != "dcs-bios")
        .all(|r| r.port_count > 0));
    sources.gateways = Arc::new(|| vec![]);
    for (scope, batch) in sources.collect(2) {
        match batch {
            Ok(batch) => {
                registry.reconcile(batch).unwrap();
            }
            Err(error) => registry.provider_failed(&scope, &error),
        }
    }
    let offline = registry.inspect(&first, 0, 32).unwrap();
    assert_eq!(
        offline.resource.availability,
        osg_profiles::resources::Availability::Offline
    );
    assert_eq!(offline.ports.len(), 2);
    assert!(registry
        .validate_token(&token, "button:0", Access::Read)
        .is_err());
}
#[test]
fn stale_can_cache_cannot_verify_ports_on_a_new_boot() {
    let (sources, _temporary) = sources();
    let mut state = can_state();
    state["modules"][0]["boot"] = json!(101);
    let items = sources
        .can_observations("gateway", &state, &BTreeMap::new())
        .unwrap();
    assert!(items[0].ports.is_none());
    assert!(!items[0].route.as_ref().unwrap().baseline_ready);
    let verified = BTreeMap::from([(MODULE.into(), ("123456789012345678901234".into(), 100))]);
    let conflicts = sources
        .can_observations("gateway", &state, &verified)
        .unwrap();
    assert!(!conflicts[0].route.as_ref().unwrap().writable);
    assert!(conflicts[0]
        .evidence
        .iter()
        .all(|e| e.kind != EvidenceKind::VerifiedPhysical));
}

fn usb_slot() -> crate::module_usb_fleet::SlotSnapshot {
    let route = crate::module_usb::UsbRouteToken {
        module_id: MODULE.into(),
        serial: "123456789012345678901234".into(),
        connection_generation: 11,
        boot: 100,
        configuration_generation: 1,
    };
    let marker = json!({"route":route,"boot":100,"status":"unsupported"});
    let mut events = can_state()["moduleEvents"][MODULE].clone();
    events["route"] = json!(route);
    crate::module_usb_fleet::SlotSnapshot {
        key: "usb-fixture".into(),
        present: true,
        automatic: true,
        admissible: true,
        diagnostics: vec![],
        port: crate::transport::PortInfo {
            path: "COM8".into(),
            serial_number: Some(route.serial.clone()),
            vendor_id: Some("1209".into()),
            product_id: Some("0001".into()),
            manufacturer: Some("OpenSimGear".into()),
            friendly_name: Some("Device Template".into()),
        },
        snapshot: crate::module_usb::UsbSnapshot {
            connected: true,
            port: Some("COM8".into()),
            identity: Some(crate::module_console::UsbIdentity {
                model: "device-template".into(),
                board: "0.1.0".into(),
                profile: "USB fixture".into(),
                module_id: MODULE.into(),
                serial: route.serial.clone(),
                protocol: 3,
                button_count: 1,
                axis_count: 0,
                hat_count: 0,
            }),
            route: Some(route),
            events: Some(events),
            actions: Some(marker),
            ..Default::default()
        },
    }
}
fn setter() -> Value {
    json!({"id":"led","label":"LED","endpoint":"led","semantics":"setter","readable":true,"parameters":[{"id":"on","label":"On","type":"boolean"}]})
}

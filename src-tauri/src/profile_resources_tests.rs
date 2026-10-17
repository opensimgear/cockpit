use super::*;
use osg_profiles::resources::{ResourceType, Route, Transport};
use osg_profiles::resources_ports::{
    ActionParameter, ActionSemantics, PortContract, PortDescriptor, PortUpdate, ScalarSchema,
};
use osg_profiles::{Direction, PortValue};
fn observed(generation: u64) -> Observation {
    Observation {
        key: "host-one".into(),
        name: "Controller".into(),
        resource_type: ResourceType::HostController,
        evidence: vec![],
        attributes: BTreeMap::new(),
        diagnostics: vec![],
        route: Some(Route {
            id: "hid-one".into(),
            transport: Transport::Hid,
            provider_generation: generation.to_string(),
            attributes: BTreeMap::new(),
            baseline_ready: true,
            writable: false,
            priority: 0,
        }),
        ports: Some(
            PortUpdate::complete(vec![PortDescriptor {
                id: "button:0".into(),
                name: "Button".into(),
                direction: Direction::Output,
                contract: PortContract::State {
                    schema: ScalarSchema::Boolean {},
                    freshness_ms: 1500,
                    fallback: Some(PortValue::Boolean(false)),
                },
            }])
            .unwrap(),
        ),
    }
}

#[test]
fn compiler_capture_uses_one_cached_native_snapshot_without_discovery_or_normalization() {
    use osg_profiles::resources::PortExpectation;
    use osg_profiles::store::LocalAssignment;
    let calls = Arc::new(AtomicU64::new(0));
    let observed_calls = calls.clone();
    let runtime = Runtime::new(
        None,
        Arc::new(move |revision| {
            observed_calls.fetch_add(1, Ordering::AcqRel);
            let mut item = observed(1);
            let ports = &mut item.ports.as_mut().unwrap().ports;
            let mut second = ports[0].clone();
            second.id = "button:1".into();
            ports.push(second);
            item.ports = Some(PortUpdate::complete(ports.clone()).unwrap());
            vec![(
                "host".into(),
                Ok(batch("host", "provider", revision, vec![item])),
            )]
        }),
        false,
    );
    runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap();
    let details = runtime.state.lock().unwrap().registry.list(0, 1).unwrap();
    let id = details.resources[0].id.clone();
    let make_assignment = |logical: &str, port: &str| {
        let detail = runtime
            .state
            .lock()
            .unwrap()
            .registry
            .inspect(&id, 0, 128)
            .unwrap();
        let hash = detail
            .ports
            .iter()
            .find(|observed| observed.descriptor.id == port)
            .unwrap()
            .contract_hash
            .clone();
        LocalAssignment {
            resource_id: logical.into(),
            source_id: id.clone(),
            ports: vec![PortExpectation {
                profile_port_id: "pressed".into(),
                observed_port_id: port.into(),
                expected_contract_hash: hash,
            }],
        }
    };
    let assignments = vec![
        make_assignment("wheel-a", "button:0"),
        make_assignment("wheel-b", "button:1"),
    ];
    let capture = runtime.snapshot_for_compile(assignments).unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(capture.inventory_session, details.session);
    assert_eq!(capture.inventory_revision, details.revision);
    assert_eq!(capture.resources.len(), 1);
    assert_eq!(capture.resources[0].ports.len(), 2);
    assert_eq!(capture.resources[0].tokens.len(), 2);
    assert!(capture.resources[0]
        .tokens
        .iter()
        .all(|token| token.access == Access::Read));
    assert!(!capture.backend.available && !capture.backend.qualified);
    assert!(capture.game_context.is_none());
    let bytes = serde_json::to_vec(&capture).unwrap();
    Runtime::adopt(
        &runtime.state,
        vec![(
            "host".into(),
            Ok(batch("host", "provider", 10, vec![observed(2)])),
        )],
    );
    assert_eq!(serde_json::to_vec(&capture).unwrap(), bytes);
    let later = runtime
        .snapshot_for_compile(capture.assignments.clone())
        .unwrap();
    assert_ne!(later.resources[0].tokens, capture.resources[0].tokens);
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[test]
fn compiler_capture_retains_missing_and_read_only_diagnostics_without_write_tokens() {
    use osg_profiles::resources::PortExpectation;
    use osg_profiles::store::LocalAssignment;
    let runtime = Runtime::new(
        None,
        Arc::new(|_| panic!("cached compilation must not enumerate")),
        false,
    );
    let mut item = observed(1);
    item.route.as_mut().unwrap().writable = true;
    item.ports = Some(
        PortUpdate::complete(vec![PortDescriptor {
            id: "action:reset".into(),
            name: "Reset".into(),
            direction: Direction::Input,
            contract: PortContract::Action {
                parameters: vec![],
                semantics: ActionSemantics::OneShot,
                endpoint: "reset".into(),
                readable: false,
                display: None,
                ownership: None,
            },
        }])
        .unwrap(),
    );
    Runtime::adopt(
        &runtime.state,
        vec![("host".into(), Ok(batch("host", "provider", 1, vec![item])))],
    );
    let details = runtime.state.lock().unwrap().registry.list(0, 1).unwrap();
    let id = details.resources[0].id.clone();
    let hash = runtime
        .state
        .lock()
        .unwrap()
        .registry
        .inspect(&id, 0, 1)
        .unwrap()
        .ports[0]
        .contract_hash
        .clone();
    let assignment = LocalAssignment {
        resource_id: "device".into(),
        source_id: id,
        ports: vec![PortExpectation {
            profile_port_id: "reset".into(),
            observed_port_id: "action:reset".into(),
            expected_contract_hash: hash,
        }],
    };
    assert_eq!(
        runtime
            .snapshot_for_compile(vec![assignment.clone()])
            .unwrap()
            .resources[0]
            .tokens[0]
            .access,
        Access::Invoke
    );
    runtime.state.lock().unwrap().read_only = true;
    let missing = LocalAssignment {
        resource_id: "missing".into(),
        source_id: uuid::Uuid::new_v4().to_string(),
        ports: vec![],
    };
    let capture = runtime
        .snapshot_for_compile(vec![assignment.clone(), missing])
        .unwrap();
    assert!(capture.read_only);
    assert!(capture.resources[0].tokens.is_empty());
    assert!(capture
        .inventory_diagnostics
        .iter()
        .any(|message| message.contains("missing")));
    assert!(runtime.snapshot_for_compile(vec![assignment; 65]).is_err());
    runtime.shutdown();
    assert!(runtime.snapshot_for_compile(vec![]).is_err());
}
#[test]
fn ipc_inventory_reads_are_separate_and_old_route_tokens_fail_after_reconnect() {
    let generation = Arc::new(AtomicU64::new(1));
    let current = generation.clone();
    let runtime = Runtime::new(
        None,
        Arc::new(move |revision| {
            vec![(
                "host".into(),
                Ok(batch(
                    "host",
                    "native-session",
                    revision,
                    vec![observed(current.load(Ordering::Acquire))],
                )),
            )]
        }),
        false,
    );
    let listed = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap();
    let id = listed["resources"][0]["id"].as_str().unwrap();
    let token = runtime.request(&json!({"command":"profile_resources_token","resourceId":id,"portId":"button:0","access":"read"})).unwrap()["resourceToken"].clone();
    assert_eq!(runtime.request(&json!({"command":"profile_resources_validate","resourceToken":token,"portId":"button:0","access":"read"})).unwrap()["valid"], true);
    generation.store(2, Ordering::Release);
    assert!(runtime.request(&json!({"command":"profile_resources_validate","resourceToken":token,"portId":"button:0","access":"read"})).is_err());
    let latest = runtime
        .request(&json!({"command":"profile_resources_inspect","resourceId":id}))
        .unwrap();
    assert_eq!(latest["resource"]["id"], id);
    assert!(runtime
        .request(&json!({"command":"profile_resources_list","profileId":"foreign"}))
        .is_err());
    assert!(runtime
        .request(&json!({"command":"profile_resources_invoke"}))
        .is_err());
    runtime.shutdown();
    assert!(runtime
        .request(&json!({"command":"profile_resources_list"}))
        .is_err());
}
#[test]
fn corrupt_inventory_is_preserved_and_reported_without_silent_reset() {
    let temporary = {
        let parent = std::env::temp_dir();
        #[cfg(unix)]
        let parent = parent.canonicalize().unwrap();
        tempfile::tempdir_in(parent).unwrap()
    };
    let path = temporary.path().join("inventory.json");
    std::fs::write(&path, b"corrupt").unwrap();
    let runtime = Runtime::new(Some(temporary.path().into()), Arc::new(|_| vec![]), false);
    let response = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap();
    assert_eq!(response["readOnly"], true);
    assert!(!response["inventoryDiagnostics"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(response["inventoryDiagnostics"][0]
        .as_str()
        .unwrap()
        .contains("Cannot parse resource inventory"));
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
}
#[test]
fn large_action_descriptors_use_short_pages_with_complete_progress() {
    let mut item = observed(1);
    let schema = ScalarSchema::Enum {
        values: (0..64)
            .map(|index| format!("{index:02}{}", "x".repeat(254)))
            .collect(),
    };
    let ports = (0..40)
        .map(|index| PortDescriptor {
            id: format!("action:{index}"),
            name: "Large typed action".into(),
            direction: Direction::Input,
            contract: PortContract::Action {
                parameters: (0..4)
                    .map(|arg| ActionParameter {
                        id: format!("arg{arg}"),
                        name: "Argument".into(),
                        schema: schema.clone(),
                    })
                    .collect(),
                semantics: ActionSemantics::OneShot,
                endpoint: format!("output{index}"),
                readable: false,
                display: None,
                ownership: None,
            },
        })
        .collect();
    item.ports = Some(PortUpdate::complete(ports).unwrap());
    let runtime = Runtime::new(
        None,
        Arc::new(move |revision| {
            vec![(
                "host".into(),
                Ok(batch("host", "session", revision, vec![item.clone()])),
            )]
        }),
        false,
    );
    let id = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap()["resources"][0]["id"]
        .clone();
    let mut offset = 0;
    while offset < 40 {
        let response = runtime.request(&json!({"command":"profile_resources_inspect","resourceId":id,"offset":offset,"limit":128})).unwrap();
        assert!(serde_json::to_vec(&response).unwrap().len() <= 1_900_000);
        assert_eq!(response["total"], 40);
        let count = response["ports"].as_array().unwrap().len();
        assert!(count > 0);
        if offset == 0 {
            assert!(count < 40);
        }
        offset += count;
    }
    assert_eq!(offset, 40);
}

fn temporary() -> tempfile::TempDir {
    {
        let parent = std::env::temp_dir();
        #[cfg(unix)]
        let parent = parent.canonicalize().unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }
}
fn scripted(
    initial: Result<Vec<Observation>, String>,
    root: Option<PathBuf>,
) -> (Runtime, Arc<Mutex<Result<Vec<Observation>, String>>>) {
    let inputs = Arc::new(Mutex::new(initial));
    let feed = inputs.clone();
    let runtime = Runtime::new(
        root,
        Arc::new(move |revision| {
            vec![(
                "host".into(),
                feed.lock()
                    .unwrap()
                    .clone()
                    .map(|items| batch("host", "native-session", revision, items)),
            )]
        }),
        false,
    );
    (runtime, inputs)
}
#[test]
fn partial_capability_failure_retains_metadata_and_invalidates_prior_authority() {
    let (runtime, feed) = scripted(Ok(vec![observed(1)]), None);
    let listed = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap();
    let id = listed["resources"][0]["id"].clone();
    let token=runtime.request(&json!({"command":"profile_resources_token","resourceId":id,"portId":"button:0","access":"read"})).unwrap()["resourceToken"].clone();
    let mut partial = observed(2);
    partial.ports = None;
    partial.diagnostics.push("Catalogue page failed".into());
    *feed.lock().unwrap() = Ok(vec![partial]);
    let inspected = runtime
        .request(&json!({"command":"profile_resources_inspect","resourceId":id}))
        .unwrap();
    assert_eq!(inspected["ports"].as_array().unwrap().len(), 1);
    assert_eq!(inspected["ports"][0]["descriptor"]["id"], "button:0");
    assert!(runtime.request(&json!({"command":"profile_resources_validate","resourceToken":token,"portId":"button:0","access":"read"})).is_err());
    *feed.lock().unwrap() = Err("Capture stopped".into());
    let unavailable = runtime
        .request(&json!({"command":"profile_resources_inspect","resourceId":id}))
        .unwrap();
    assert_eq!(unavailable["ports"].as_array().unwrap().len(), 1);
    assert!(unavailable["resource"]["connections"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(unavailable["resource"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str().unwrap().contains("Capture stopped")));
    assert!(runtime.request(&json!({"command":"profile_resources_token","resourceId":id,"portId":"button:0","access":"read"})).is_err());
}
#[test]
fn failed_inventory_save_is_retried_on_unchanged_provider_data_then_restores_offline() {
    let directory = temporary();
    let (runtime, _feed) = scripted(Ok(vec![observed(1)]), Some(directory.path().into()));
    let target = directory.path().join("inventory.json");
    std::fs::create_dir(&target).unwrap();
    let listed = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap();
    let id = listed["resources"][0]["id"].clone();
    assert!(!listed["inventoryDiagnostics"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(target.is_dir());
    std::fs::remove_dir(&target).unwrap();
    let retried = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap();
    assert!(
        retried["inventoryDiagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{retried}"
    );
    assert!(target.is_file());
    runtime.shutdown();
    let restored = Runtime::new(Some(directory.path().into()), Arc::new(|_| vec![]), false);
    let item = restored
        .request(&json!({"command":"profile_resources_inspect","resourceId":id}))
        .unwrap();
    assert_eq!(item["resource"]["availability"], "offline");
    assert_eq!(item["ports"].as_array().unwrap().len(), 1);
    assert!(restored.request(&json!({"command":"profile_resources_token","resourceId":id,"portId":"button:0","access":"read"})).is_err());
}
#[test]
fn unchanged_polling_preserves_revision_and_complete_removal_retains_offline_identity() {
    let (runtime, feed) = scripted(Ok(vec![observed(1)]), None);
    let first = runtime
        .request(&json!({"command":"profile_resources_list","offset":0,"limit":1}))
        .unwrap();
    let second = runtime
        .request(&json!({"command":"profile_resources_list","offset":0,"limit":1}))
        .unwrap();
    assert_eq!(first["revision"], second["revision"]);
    assert_eq!(first["resources"][0]["id"], second["resources"][0]["id"]);
    let id = first["resources"][0]["id"].clone();
    *feed.lock().unwrap() = Ok(vec![]);
    let gone = runtime
        .request(&json!({"command":"profile_resources_inspect","resourceId":id}))
        .unwrap();
    assert_eq!(gone["resource"]["availability"], "offline");
    assert_eq!(gone["ports"].as_array().unwrap().len(), 1);
}
#[test]
fn inventory_ipc_rejects_unknown_fields_invalid_pages_and_forged_authority() {
    let (runtime, _feed) = scripted(Ok(vec![observed(1)]), None);
    let id = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap()["resources"][0]["id"]
        .clone();
    for request in [
        json!(null),
        json!({}),
        json!({"command":"profile_resources_list","offset":-1}),
        json!({"command":"profile_resources_list","limit":129}),
        json!({"command":"profile_resources_list","offset":40001}),
        json!({"command":"profile_resources_list","limit":"1"}),
        json!({"command":"profile_resources_list","clientId":"unexpected"}),
        json!({"command":"profile_resources_inspect","resourceId":5}),
        json!({"command":"profile_resources_inspect","resourceId":"missing"}),
        json!({"command":"profile_resources_token","resourceId":id,"portId":"button:0","access":"admin"}),
        json!({"command":"profile_resources_token","resourceId":id,"portId":"button:0","access":"invoke"}),
        json!({"command":"profile_resources_validate","resourceToken":{},"portId":"button:0","access":"read"}),
    ] {
        assert!(runtime.request(&request).is_err(), "{request}");
    }
    let token=runtime.request(&json!({"command":"profile_resources_token","resourceId":id,"portId":"button:0","access":"read"})).unwrap()["resourceToken"].clone();
    let mut forged = token.clone();
    forged["access"] = json!("invoke");
    assert!(runtime.request(&json!({"command":"profile_resources_validate","resourceToken":forged,"portId":"button:0","access":"invoke"})).is_err());
    assert!(runtime.request(&json!({"command":"profile_resources_validate","resourceToken":token,"portId":"button:1","access":"read"})).is_err());
}
#[test]
fn corrupt_retained_inventory_allows_inspection_but_blocks_write_tokens() {
    let directory = temporary();
    let path = directory.path().join("inventory.json");
    std::fs::write(&path, b"corrupt").unwrap();
    let mut item = observed(1);
    item.route.as_mut().unwrap().writable = true;
    item.ports = Some(
        PortUpdate::complete(vec![PortDescriptor {
            id: "action:led".into(),
            name: "LED".into(),
            direction: Direction::Input,
            contract: PortContract::Action {
                parameters: vec![ActionParameter {
                    id: "on".into(),
                    name: "On".into(),
                    schema: ScalarSchema::Boolean {},
                }],
                semantics: ActionSemantics::Setter,
                endpoint: "led".into(),
                readable: true,
                display: None,
                ownership: None,
            },
        }])
        .unwrap(),
    );
    let (runtime, _feed) = scripted(Ok(vec![item]), Some(directory.path().into()));
    let listed = runtime
        .request(&json!({"command":"profile_resources_list"}))
        .unwrap();
    let id = listed["resources"][0]["id"].clone();
    assert_eq!(listed["readOnly"], true);
    let inspected = runtime
        .request(&json!({"command":"profile_resources_inspect","resourceId":id}))
        .unwrap();
    assert_eq!(inspected["ports"].as_array().unwrap().len(), 1);
    assert!(runtime.request(&json!({"command":"profile_resources_token","resourceId":id,"portId":"action:led","access":"invoke"})).unwrap_err().contains("Repair"));
    assert_eq!(std::fs::read(path).unwrap(), b"corrupt");
}
#[test]
fn native_inventory_reads_never_call_a_second_discovery_or_acquire_writer() {
    let calls = Arc::new(AtomicU64::new(0));
    let count = calls.clone();
    let runtime = Runtime::new(
        None,
        Arc::new(move |revision| {
            count.fetch_add(1, Ordering::AcqRel);
            vec![(
                "host".into(),
                Ok(batch("host", "background", revision, vec![observed(1)])),
            )]
        }),
        true,
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while calls.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(calls.load(Ordering::Acquire), 1);
    for _ in 0..8 {
        runtime
            .request(&json!({"command":"profile_resources_list"}))
            .unwrap();
    }
    assert_eq!(calls.load(Ordering::Acquire), 1);
    runtime.shutdown();
}

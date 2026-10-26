//! Explicit ignored local proof using the real browser, WSS, native inventory
//! and profile coordinator. It has no production authority API, hardware driver
//! or simulator telemetry source; the visible toggle selects authored presets.
use super::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct NoGames;
impl crate::game_discovery::Platform for NoGames {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Err("No game is part of this browser proof".into())
    }
    fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
        panic!("No game launch")
    }
    fn find(&self, _: &Value) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
        panic!("No game discovery")
    }
    fn alive(&self, _: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        panic!("No game process")
    }
}

struct Live {
    coordinator: crate::profile_session::Coordinator<NativeProfileHost>,
    resources: Arc<crate::profile_resources::Runtime>,
    controllers: Arc<osg_controller_input::ControllerService>,
    games: Arc<crate::games::GamesRuntime>,
    companions: Arc<crate::companion_host::Runtime>,
    hub: Hub,
    publication: Publication,
    epoch: Arc<AtomicU64>,
    directory: PathBuf,
}
impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.coordinator.stop();
        self.resources.shutdown();
        self.games.shutdown();
        self.companions.shutdown();
        self.controllers.shutdown();
    }
}
fn write_json(path: &Path, value: &Value) {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap()).unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.as_file().sync_all().unwrap();
    // Windows readers and scanners may briefly retain the replaced handoff.
    // Keep the already synced file and retry only this test-local atomic move.
    for attempt in 0..20 {
        match file.persist(path) {
            Ok(_) => return,
            Err(error) if error.error.kind() == std::io::ErrorKind::PermissionDenied && attempt < 19 => {
                file = error.file;
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("Cannot replace isolated browser proof handoff: {error}"),
        }
    }
}
fn authored_publication(hub: &Hub) -> Publication {
    use sha2::{Digest, Sha256};
    let mut upload: PublicationUpload = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    upload.canonical_document=upload.canonical_document.replace("Flight instruments","Authored altitude presets — native profile proof")
        .replace("\"id\":\"flight-widget-1\",\"kind\":\"button\",\"label\":\"Button\"","\"id\":\"flight-widget-1\",\"kind\":\"button\",\"label\":\"Authored altitude: 9000 / 12000 ft\"")
        .replace("\"id\":\"flight-widget-7\",\"kind\":\"indicator\",\"label\":\"Indicator\"","\"id\":\"flight-widget-7\",\"kind\":\"indicator\",\"label\":\"Above 3000 m / release below 2900 m\"")
        .replace("\"id\":\"flight-widget-8\",\"kind\":\"text\",\"label\":\"Text\"","\"id\":\"flight-widget-8\",\"kind\":\"text\",\"label\":\"Authored altitude in metres\"");
    upload.publication_hash = format!(
        "sha256:{:x}",
        Sha256::digest(upload.canonical_document.as_bytes())
    );
    upload.capability_fingerprint = None;
    let publication = Publication::validate_legacy_test_fixture(upload.clone()).unwrap();
    let mut command = serde_json::to_value(upload).unwrap();
    command["command"] = json!("companion_publish");
    crate::retain_companion_test_fixture(&hub,&command).unwrap();
    hub.command(&json!({"command":"companion_assign","instanceId":"instance","companionId":"screen","publicationHash":publication.publication_hash})).unwrap();
    publication
}
impl Live {
    fn new(directory: PathBuf) -> (Self, Value) {
        let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
        let clock = controllers.clone();
        let provider = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(move || clock.stream_page("", 0).captured_us),
        ));
        // Pairing uses the real Hub protocol and an isolated ephemeral identity.
        // These setup messages contain no input samples or delivery capabilities.
        let (hub, _, token) = fixture_with_sink(provider.clone());
        let publication = authored_publication(&hub);
        let companions = Arc::new(crate::companion_host::Runtime::with_profile_metadata(
            hub.clone(),
            provider.clone(),
            directory.join("companion"),
        ));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let status=companions.request(&json!({"command":"companion_enable","clientId":"native-browser-proof","companion":{"address":"127.0.0.1","port":port,"advertisedHost":"127.0.0.1","discovery":false}})).unwrap();
        assert_eq!(status["listener"]["enabled"], true, "{status}");
        let observed = provider.clone();
        let metadata = companions.clone();
        let session = uuid::Uuid::new_v4().to_string();
        let collect: crate::profile_resources::Collect = Arc::new(move |revision| {
            let result = observed
                .resource_inventory()
                .into_iter()
                .map(|source| {
                    let id = source.definition.source_id.clone();
                    crate::profile_resource_sources::companion_observation(
                        source,
                        metadata.resource_publication(&id),
                        metadata.resource_transport(&id),
                    )
                })
                .collect::<Result<Vec<_>, String>>()
                .map(|items| {
                    crate::profile_resources::batch("companions", &session, revision, items)
                });
            vec![("companions".into(), result)]
        });
        let resources = Arc::new(crate::profile_resources::Runtime::new(None, collect, true));
        let games = Arc::new(crate::games::GamesRuntime::test_runtime(
            Arc::new(|_| panic!("No hardware/provider IO")),
            crate::games::Paths::in_directory(&directory),
            Arc::new(NoGames),
        ));
        let epoch = Arc::new(AtomicU64::new(1));
        let host = NativeProfileHost::new(
            resources.clone(),
            controllers.clone(),
            games.clone(),
            companions.clone(),
            Arc::new(crate::gateways::ProfilePanelIo::empty()),
            false,
            epoch.clone(),
        );
        let coordinator = crate::profile_session::Coordinator::new(
            host,
            Some(directory.join("profile-session.json")),
        );
        let handoff = json!({"phase":"waiting-browser","browserUrl":status["browserUrl"],"endpoint":status["listener"]["endpoint"],"hostId":hub.host_id(),"companionId":"screen","token":token,"instanceId":"instance","publicationHash":publication.publication_hash,"sourceWidgetId":"flight-widget-1","indicatorWidgetId":"flight-widget-7","textWidgetId":"flight-widget-8","authoredPresetFeet":{"false":9000,"true":12000},"simulatorTelemetry":false});
        (
            Self {
                coordinator,
                resources,
                controllers,
                games,
                companions,
                hub,
                publication,
                epoch,
                directory,
            },
            handoff,
        )
    }
    fn candidate(&self, default: bool) -> Candidate {
        let mut candidate = Candidate { launch_preparation: false,
            document: osg_profiles::default_document(),
            assignments: vec![],
            read_only: false,
            game_id: None,
            admission_epoch: Some(self.epoch.load(Ordering::Acquire)),
            stage: None,
        };
        if !default {
            let document: osg_profiles::ProfileDocument = serde_json::from_str(include_str!(
                "../../tests/fixtures/profiles/complete-rig.json"
            ))
            .unwrap();
            let mut authored =
                osg_profiles::new_document("Authored altitude presets — browser proof").unwrap();
            let nodes = [
                "feet-to-metres",
                "altitude-threshold",
                "altitude-visible",
                "altitude-format",
            ];
            authored.processing_nodes = document
                .processing_nodes
                .into_iter()
                .filter(|node| nodes.contains(&node.id.as_str()))
                .collect();
            let ports = crate::profile_resource_ports::companion(
                &osg_mapping_host::companions::native_definition(
                    self.publication.definition("instance"),
                )
                .unwrap(),
                &self.publication.feedback,
            )
            .unwrap();
            let input = ports
                .iter()
                .find(|port| port.id == "flight-input-1")
                .unwrap();
            let boolean = self
                .publication
                .feedback
                .iter()
                .find(|feedback| feedback.kind == osg_companion_service::FeedbackKind::Boolean)
                .unwrap();
            let text = self
                .publication
                .feedback
                .iter()
                .find(|feedback| feedback.kind == osg_companion_service::FeedbackKind::Text)
                .unwrap();
            let indicator = ports
                .iter()
                .find(|port| {
                    port.id == format!("feedback:{}:{}", boolean.channel_id, boolean.output_ids[0])
                })
                .unwrap();
            let label = ports
                .iter()
                .find(|port| {
                    port.id == format!("feedback:{}:{}", text.channel_id, text.output_ids[0])
                })
                .unwrap();
            let declarations = [
                ("altitude-toggle", input),
                ("warning", indicator),
                ("altitude-label", label),
            ];
            authored.resources.push(osg_profiles::Resource {
                id: "companion-panel".into(),
                name: "Actual browser companion".into(),
                kind: osg_profiles::ResourceKind::Companion,
                required: true,
                ports: declarations
                    .iter()
                    .map(|(id, port)| osg_profiles::Port {
                        id: (*id).into(),
                        name: port.name.clone(),
                        direction: port.direction,
                        contract: port.contract.clone(),
                    })
                    .collect(),
            });
            let mut preset_input = authored.resources[0].ports[0].clone();
            preset_input.id = "active".into();
            preset_input.direction = osg_profiles::Direction::Input;
            let mut preset_output = authored
                .processing_nodes
                .iter()
                .find(|node| node.id == "feet-to-metres")
                .unwrap()
                .inputs[0]
                .clone();
            preset_output.direction = osg_profiles::Direction::Output;
            authored
                .processing_nodes
                .push(osg_profiles::ProcessingNode { draft: None,
                    id: "authored-preset".into(),
                    name: "Explicit authored altitude presets, not simulator telemetry".into(),
                    inputs: vec![preset_input],
                    outputs: vec![preset_output],
                    operation: osg_mapping::program::Operation::Select {
                        when_true: osg_mapping::signal::PortValue::Number(12000.),
                        when_false: osg_mapping::signal::PortValue::Number(9000.),
                    },
                });
            let included = |id: &str| id == "companion-panel" || nodes.contains(&id);
            authored.bindings = document
                .bindings
                .into_iter()
                .filter(|binding| included(&binding.from.node_id) && included(&binding.to.node_id))
                .collect();
            for (id, from_node, from_port, to_node, to_port) in [
                (
                    "native-preset",
                    "companion-panel",
                    "altitude-toggle",
                    "authored-preset",
                    "active",
                ),
                (
                    "authored-feet",
                    "authored-preset",
                    "feet",
                    "feet-to-metres",
                    "feet",
                ),
            ] {
                authored.bindings.push(osg_profiles::Binding {
                    id: id.into(),
                    from: osg_profiles::Endpoint {
                        node_id: from_node.into(),
                        port_id: from_port.into(),
                    },
                    to: osg_profiles::Endpoint {
                        node_id: to_node.into(),
                        port_id: to_port.into(),
                    },
                });
            }
            authored
                .companion_assignments
                .push(osg_profiles::CompanionAssignment { panel_id: None,
                    resource_id: "companion-panel".into(),
                    publication_hash: self.publication.publication_hash.clone(),
                });
            osg_profiles::validate_document(&authored).unwrap();
            let listed = self
                .resources
                .request(&json!({"command":"profile_resources_list"}))
                .unwrap();
            let source = listed["resources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|source| source["resourceType"] == "companion")
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned();
            candidate
                .assignments
                .push(osg_profiles::store::LocalAssignment {
                    resource_id: "companion-panel".into(),
                    source_id: source,
                    ports: declarations
                        .iter()
                        .map(|(logical, port)| osg_profiles::resources::PortExpectation {
                            profile_port_id: (*logical).into(),
                            observed_port_id: port.id.clone(),
                            expected_contract_hash: port.contract_hash().unwrap(),
                        })
                        .collect(),
                });
            candidate.document = authored;
        }
        write_json(
            &self.directory.join("authored-profile.json"),
            &serde_json::to_value(&candidate.document).unwrap(),
        );
        candidate.stage = Some(
            NativeProfileHost::stage(
                &self.resources,
                &self.games,
                &self.companions,
                self.controllers.clone(),
                false,
                &candidate,
            )
            .unwrap(),
        );
        candidate
    }
}

#[test]
#[ignore = "Explicit local real-browser proof; requires built companion assets and OSG_COMPANION_LIVE_DIR"]
fn actual_browser_wss_profile_altitude_feedback_proof() {
    let directory = PathBuf::from(
        std::env::var_os("OSG_COMPANION_LIVE_DIR").expect("Choose an isolated proof directory"),
    );
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    let (mut live, mut handoff) = Live::new(directory.clone());
    let path = directory.join("handoff.json");
    let actions = directory.join("actions.json");
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut sequence = 0u64;
    let mut finished = false;
    while Instant::now() < deadline {
        live.resources.refresh_native().unwrap();
        live.coordinator.poll().unwrap();
        if let Ok(bytes) = std::fs::read(&actions) {
            if bytes.len() <= 4096 {
                if let Ok(command) = serde_json::from_slice::<Value>(&bytes) {
                    if command.as_object().is_some_and(|object| object.len() == 2)
                        && command["sequence"]
                            .as_u64()
                            .is_some_and(|next| next > sequence)
                    {
                        let next = command["sequence"].as_u64().unwrap();
                        let result=match command["action"].as_str().unwrap_or("") {
                            "apply"=>{handoff["phase"]=json!("applying");write_json(&path,&handoff);let candidate=live.candidate(false);live.coordinator.apply(candidate)},
                            "default"=>{let candidate=live.candidate(true);live.coordinator.apply(candidate)},
                            "stop"=>live.coordinator.stop(),
                            "snapshot"=>Ok(()),
                            "finish"=>{finished=true;live.coordinator.stop()},
                            _=>Err("Unknown native proof control; input values are never accepted here".into()),
                        };
                        sequence = next;
                        handoff["lastAction"] = command;
                        handoff["lastError"] = json!(result.err());
                    }
                }
            }
        }
        handoff["phase"] = json!("polling");
        handoff["sequence"] = json!(sequence);
        handoff["profileSession"] = serde_json::to_value(live.coordinator.status()).unwrap();
        handoff["host"] = live
            .hub
            .command(&json!({"command":"companion_status"}))
            .unwrap();
        write_json(&path, &handoff);
        if finished {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    live.coordinator.stop().unwrap();
    handoff["phase"] = json!("stopped");
    handoff["token"] = Value::Null;
    handoff["profileSession"] = serde_json::to_value(live.coordinator.status()).unwrap();
    write_json(&path, &handoff);
    assert!(
        finished,
        "Browser proof exceeded its bounded 180-second window"
    );
}

#[path = "profile_session_companion_dcs_live_tests.rs"]
mod dcs_live;

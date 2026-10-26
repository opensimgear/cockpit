//! Ignored assembled software proof. A labelled process fixture and official-
//! format loopback UDP packets exercise the real catalogue/reader, NativeHost,
//! coordinator, WSS protocol and shared browser renderer. No game is launched.
use super::*;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Mutex;

const RAW_ALTITUDE: &str = "channel:common:CommonData:ALT_MSL_FT:0";

struct FixtureProcess {
    identity: crate::game_discovery::ProcessIdentity,
    present: AtomicBool,
}
impl crate::game_discovery::Platform for FixtureProcess {
    fn integration_setup_supported(&self) -> bool {
        true
    }
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![
            json!({"id":"dcs","name":"DCS protocol fixture (no actual game)","provider":"dcs-bios","installed":true,"executable":self.identity.path,"launchSupported":false,"processMonitoringSupported":true}),
        ]
    }
    fn validate(&self, id: &str, path: &str) -> Result<(), String> {
        if id == "dcs" && path == self.identity.path {
            Ok(())
        } else {
            Err("Only the explicitly labelled private process fixture exists.".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
        panic!("This proof never launches a game")
    }
    fn find(&self, _: &Value) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
        Ok(self
            .present
            .load(Ordering::Acquire)
            .then(|| self.identity.clone()))
    }
    fn alive(&self, identity: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
        Ok(self.present.load(Ordering::Acquire) && identity == &self.identity)
    }
}

#[derive(Clone, Copy, Debug)]
enum FixtureMode {
    Unknown,
    Low,
    High,
    Hysteresis,
    BelowRelease,
    Stale,
    Frozen,
    WrongAircraft,
    None,
}
struct FixtureWire {
    mode: Arc<Mutex<FixtureMode>>,
    counter: Arc<AtomicU32>,
    sent: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
fn fixture_packet(counter: u8, aircraft: &str, altitude: Option<u16>) -> Vec<u8> {
    fn block(bytes: &mut Vec<u8>, address: u16, value: &[u8]) {
        bytes.extend(address.to_le_bytes());
        bytes.extend((value.len() as u16).to_le_bytes());
        bytes.extend(value);
    }
    let mut bytes = vec![0x55; 4];
    let mut name = [0; 24];
    name[..aircraft.len()].copy_from_slice(aircraft.as_bytes());
    block(&mut bytes, 0, &name);
    // Pinned v0.11.7 CommonData ALT_MSL_FT, integer raw0..65535.
    if let Some(altitude) = altitude {
        block(&mut bytes, 1076, &altitude.to_le_bytes());
    }
    // Pinned MetadataEnd completed export counter.
    block(&mut bytes, 65534, &[counter, 0]);
    bytes
}
impl FixtureWire {
    fn new() -> Self {
        let mode = Arc::new(Mutex::new(FixtureMode::Unknown));
        let counter = Arc::new(AtomicU32::new(1));
        let sent = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (current, clock, packets, cancelled) =
            (mode.clone(), counter.clone(), sent.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
            while !cancelled.load(Ordering::Acquire) {
                let mode = *current.lock().unwrap();
                if !matches!(mode, FixtureMode::Stale) {
                    let counter = if matches!(mode, FixtureMode::Frozen) {
                        clock.load(Ordering::Acquire)
                    } else {
                        clock.fetch_add(1, Ordering::AcqRel) + 1
                    } as u8;
                    let (aircraft, altitude) = match mode {
                        FixtureMode::Unknown => ("A-10C", None),
                        FixtureMode::Low => ("A-10C", Some(9000)),
                        FixtureMode::High | FixtureMode::Frozen => ("A-10C", Some(10000)),
                        FixtureMode::Hysteresis => ("A-10C", Some(9700)),
                        FixtureMode::BelowRelease => ("A-10C", Some(9500)),
                        FixtureMode::WrongAircraft => ("F-16C_50", Some(10000)),
                        FixtureMode::None => ("NONE", Some(10000)),
                        FixtureMode::Stale => unreachable!(),
                    };
                    udp.send_to(
                        &fixture_packet(counter, aircraft, altitude),
                        "127.0.0.1:5010",
                    )
                    .unwrap();
                    packets.fetch_add(1, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        });
        Self {
            mode,
            counter,
            sent,
            stop,
            worker: Some(worker),
        }
    }
    fn set(&self, mode: FixtureMode) {
        *self.mode.lock().unwrap() = mode;
    }
    fn status(&self) -> Value {
        json!({"mode":format!("{:?}",*self.mode.lock().unwrap()),"completedExportCounter":self.counter.load(Ordering::Acquire) as u8,"packetsSent":self.sent.load(Ordering::Acquire),"transport":"official-format fixture UDP, loopback127.0.0.1:5010","protocolFixture":true,"actualGame":false,"measurementValidityAdvertised":false})
    }
}
impl Drop for FixtureWire {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

fn fixture_publication(hub: &Hub) -> Publication {
    use sha2::{Digest, Sha256};
    let mut upload: PublicationUpload = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    upload.canonical_document = upload.canonical_document
        .replace("Flight instruments", "Reported DCS-BIOS MSL altitude — protocol fixture")
        .replace("\"id\":\"flight-widget-1\",\"kind\":\"button\",\"label\":\"Button\"", "\"id\":\"flight-widget-1\",\"kind\":\"button\",\"label\":\"Unconnected browser control (not altitude)\"")
        .replace("\"id\":\"flight-widget-7\",\"kind\":\"indicator\",\"label\":\"Indicator\"", "\"id\":\"flight-widget-7\",\"kind\":\"indicator\",\"label\":\"Above 3000 m / release below 2900 m\"")
        .replace("\"id\":\"flight-widget-8\",\"kind\":\"text\",\"label\":\"Text\"", "\"id\":\"flight-widget-8\",\"kind\":\"text\",\"label\":\"Reported MSL altitude in metres\"");
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

struct DcsLive {
    coordinator: crate::profile_session::Coordinator<NativeProfileHost>,
    resources: Arc<crate::profile_resources::Runtime>,
    controllers: Arc<osg_controller_input::ControllerService>,
    games: Arc<crate::games::GamesRuntime>,
    companions: Arc<crate::companion_host::Runtime>,
    usb: Arc<crate::module_usb_fleet::Fleet>,
    process: Arc<FixtureProcess>,
    wire: FixtureWire,
    hub: Hub,
    publication: Publication,
    epoch: Arc<AtomicU64>,
    directory: PathBuf,
}
impl Drop for DcsLive {
    fn drop(&mut self) {
        let _ = self.coordinator.stop();
        self.resources.shutdown();
        self.games.shutdown();
        self.companions.shutdown();
        self.controllers.shutdown();
        self.usb.shutdown();
    }
}
impl DcsLive {
    fn new(directory: PathBuf) -> (Self, Value) {
        // Production setup intentionally permits only the installed DCS-BIOS
        // port. An ordinary exclusive bind detects another owner before the
        // reader's multicast-compatible socket options could permit sharing.
        let available = UdpSocket::bind("127.0.0.1:5010").expect(
            "Loopback UDP5010 is already occupied; preserve that owner and stop this proof",
        );
        drop(available);
        let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
        let clock = controllers.clone();
        let provider = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(move || clock.stream_page("", 0).captured_us),
        ));
        let (hub, _, token) = fixture_with_sink(provider.clone());
        let publication = fixture_publication(&hub);
        let companions = Arc::new(crate::companion_host::Runtime::with_profile_metadata(
            hub.clone(),
            provider.clone(),
            directory.join("companion"),
        ));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let status = companions.request(&json!({"command":"companion_enable","clientId":"native-dcs-browser-proof","companion":{"address":"127.0.0.1","port":port,"advertisedHost":"127.0.0.1","discovery":false}})).unwrap();
        assert_eq!(status["listener"]["enabled"], true, "{status}");

        let user = directory.join("fixture-user");
        std::fs::create_dir_all(&user).unwrap();
        crate::dcs_setup::fixture_module(&user);
        std::fs::write(
            user.join("Scripts/Export.lua"),
            "dofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])\n",
        )
        .unwrap();
        let catalogue = user.join("Scripts/DCS-BIOS/doc/json");
        crate::dcs_setup::fixture_exports(&user);
        // Actual full release documents, not a made-up unitful altitude port.
        // The labelled runtime/process/wire remain fixtures; no Lua executes.
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let archive = std::fs::read(
            std::env::var_os("OSG_DCS_SETUP_ARCHIVE")
                .expect("Set OSG_DCS_SETUP_ARCHIVE to the checksum-pinned release"),
        )
        .unwrap();
        assert_eq!(archive.len(), 8_234_988);
        assert_eq!(
            format!("{:x}", Sha256::digest(&archive)),
            crate::dcs_setup::ARCHIVE_SHA256
        );
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
        for name in [
            "A-10C.json",
            "AircraftAliases.json",
            "MetadataStart.json",
            "MetadataEnd.json",
            "CommonData.json",
        ] {
            let suffix = format!("doc/json/{name}");
            let indices = (0..zip.len())
                .filter(|index| zip.by_index(*index).unwrap().name().ends_with(&suffix))
                .collect::<Vec<_>>();
            assert_eq!(indices.len(), 1, "{name}");
            let mut file = zip.by_index(indices[0]).unwrap();
            assert!(file.size() <= crate::dcs_bios::MAX_DOCUMENT);
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            std::fs::write(catalogue.join(name), bytes).unwrap();
        }
        let process = Arc::new(FixtureProcess {
            identity: crate::game_discovery::ProcessIdentity {
                pid: 4242,
                path: directory
                    .join("FIXTURE-NOT-ACTUAL-DCS.exe")
                    .to_str()
                    .unwrap()
                    .into(),
                epoch: 1,
            },
            present: AtomicBool::new(true),
        });
        let games = Arc::new(crate::games::GamesRuntime::test_runtime(
            Arc::new(|_| panic!("No hardware or retired provider IO")),
            crate::games::Paths::in_directory(&directory),
            process.clone(),
        ));
        let revision =
            games.resource_catalogue()[0]["integrationSetup"]["providerSetupRevision"].clone();
        // Keep production setup defaults. Listen/send only on loopback; never
        // start the gateway service or discover a physical USB/HID endpoint.
        games.request(&json!({"command":"game_integration_settings_save","gameId":"dcs","expectedSetupRevision":revision,"userFolder":user,"aircraftModule":"A-10C","connection":{"cataloguePath":catalogue,"freshnessMs":1500,"commandHost":"127.0.0.1","commandPort":7778,"listenAddress":"127.0.0.1","listenPort":5010,"multicastGroup":"239.255.50.10","multicastInterface":"127.0.0.1"}})).unwrap();
        let ready = games.resource_catalogue();
        assert_eq!(ready[0]["integrationSetup"]["state"], "ready", "{ready:?}");
        let usb = Arc::new(crate::module_usb_fleet::Fleet::new(
            Arc::new(|| panic!("No physical serial enumeration")),
            false,
        ));
        let collect = crate::profile_resource_sources::collector(
            controllers.clone(),
            provider,
            companions.clone(),
            games.clone(),
            Arc::new(Vec::new),
            Arc::new(|_| panic!("No physical panel IO")),
            usb.clone(),
            false,
        );
        let resources = Arc::new(crate::profile_resources::Runtime::new(None, collect, true));
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
        let wire = FixtureWire::new();
        let handoff = json!({"phase":"waiting-browser","browserUrl":status["browserUrl"],"endpoint":status["listener"]["endpoint"],"hostId":hub.host_id(),"companionId":"screen","token":token,"instanceId":"instance","publicationHash":publication.publication_hash,"sourceWidgetId":"flight-widget-1","indicatorWidgetId":"flight-widget-7","textWidgetId":"flight-widget-8","protocolFixture":true,"actualGame":false,"simulatorTelemetry":false,"rawPort":RAW_ALTITUDE,"quantity":"Reported MSL altitude","rawUnit":null,"declaredUnit":"ft","destinationUnit":"m","measurementValidityAdvertised":false,"upstreamFeetDivisor":0.304});
        (
            Self {
                coordinator,
                resources,
                controllers,
                games,
                companions,
                usb,
                process,
                wire,
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
            game_id: (!default).then(|| "dcs".into()),
            admission_epoch: Some(self.epoch.load(Ordering::Acquire)),
            stage: None,
        };
        if !default {
            let template: osg_profiles::ProfileDocument = serde_json::from_str(include_str!(
                "../../tests/fixtures/profiles/complete-rig.json"
            ))
            .unwrap();
            let nodes = [
                "altitude-quantity",
                "feet-to-metres",
                "altitude-threshold",
                "altitude-visible",
                "altitude-format",
            ];
            let mut document = osg_profiles::new_document(
                "Official-format CommonData UDP — actual browser software proof",
            )
            .unwrap();
            document.eligibility = template.eligibility;
            document.processing_nodes = template
                .processing_nodes
                .into_iter()
                .filter(|node| nodes.contains(&node.id.as_str()))
                .collect();
            let catalogue = self.games.resource_integration_catalogue("dcs").unwrap();
            let observed = crate::profile_resource_ports::integration(&catalogue).unwrap();
            let raw = observed
                .iter()
                .find(|port| port.id == RAW_ALTITUDE)
                .unwrap();
            document.resources.push(osg_profiles::Resource {
                id: "dcs".into(),
                name: "DCS-BIOS reported raw data (process/wire fixture)".into(),
                kind: osg_profiles::ResourceKind::Game,
                required: true,
                ports: vec![osg_profiles::Port {
                    id: "altitude".into(),
                    name: raw.name.clone(),
                    direction: raw.direction,
                    contract: raw.contract.clone(),
                }],
            });
            let ports = crate::profile_resource_ports::companion(
                &osg_mapping_host::companions::native_definition(
                    self.publication.definition("instance"),
                )
                .unwrap(),
                &self.publication.feedback,
            )
            .unwrap();
            let feedback = |kind| {
                self.publication
                    .feedback
                    .iter()
                    .find(|feedback| feedback.kind == kind)
                    .unwrap()
            };
            let boolean = feedback(osg_companion_service::FeedbackKind::Boolean);
            let text = feedback(osg_companion_service::FeedbackKind::Text);
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
            let declarations = [("warning", indicator), ("altitude-label", label)];
            document.resources.push(osg_profiles::Resource {
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
            let included = |id: &str| id == "dcs" || id == "companion-panel" || nodes.contains(&id);
            document.bindings = template
                .bindings
                .into_iter()
                .filter(|binding| included(&binding.from.node_id) && included(&binding.to.node_id))
                .collect();
            document
                .companion_assignments
                .push(osg_profiles::CompanionAssignment { panel_id: None,
                    resource_id: "companion-panel".into(),
                    publication_hash: self.publication.publication_hash.clone(),
                });
            osg_profiles::validate_document(&document).unwrap();
            self.resources.refresh_native().unwrap();
            let listed = self
                .resources
                .request(&json!({"command":"profile_resources_list"}))
                .unwrap();
            let all = listed["resources"].as_array().unwrap();
            let companion = all
                .iter()
                .find(|source| source["resourceType"] == "companion")
                .unwrap()["id"]
                .as_str()
                .unwrap();
            let game = all
                .iter()
                .find(|source| source["attributes"]["gameId"] == "dcs")
                .unwrap()["id"]
                .as_str()
                .unwrap();
            candidate
                .assignments
                .push(osg_profiles::store::LocalAssignment {
                    resource_id: "companion-panel".into(),
                    source_id: companion.into(),
                    ports: declarations
                        .iter()
                        .map(|(id, port)| osg_profiles::resources::PortExpectation {
                            profile_port_id: (*id).into(),
                            observed_port_id: port.id.clone(),
                            expected_contract_hash: port.contract_hash().unwrap(),
                        })
                        .collect(),
                });
            candidate
                .assignments
                .push(osg_profiles::store::LocalAssignment {
                    resource_id: "dcs".into(),
                    source_id: game.into(),
                    ports: vec![osg_profiles::resources::PortExpectation {
                        profile_port_id: "altitude".into(),
                        observed_port_id: raw.id.clone(),
                        expected_contract_hash: raw.contract_hash().unwrap(),
                    }],
                });
            candidate.document = document;
            write_json(
                &self.directory.join("observed-game-catalogue.json"),
                &catalogue,
            );
            write_json(
                &self.directory.join("captured-resource-inventory.json"),
                &listed,
            );
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
#[ignore = "Explicit actual-browser software proof; OSG_COMPANION_DCS_LIVE_DIR and built browser assets required"]
fn actual_browser_wss_dcs_commondata_feedback_proof() {
    let directory = PathBuf::from(
        std::env::var_os("OSG_COMPANION_DCS_LIVE_DIR").expect("Choose an isolated proof directory"),
    );
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    let (mut live, mut handoff) = DcsLive::new(directory.clone());
    let path = directory.join("handoff.json");
    let actions = directory.join("actions.json");
    let deadline = Instant::now() + Duration::from_secs(240);
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
                        let result = match command["action"].as_str().unwrap_or("") {
                            "apply" => { handoff["phase"] = json!("applying"); write_json(&path,&handoff); let candidate = live.candidate(false); live.coordinator.apply(candidate) },
                            "default" => { let candidate = live.candidate(true); live.coordinator.apply(candidate) },
                            "stop" => live.coordinator.stop(),
                            "snapshot" => Ok(()),
                            "fixture-low" => { live.wire.set(FixtureMode::Low); Ok(()) },
                            "fixture-high" => { live.wire.set(FixtureMode::High); Ok(()) },
                            "fixture-hysteresis" => { live.wire.set(FixtureMode::Hysteresis); Ok(()) },
                            "fixture-below-release" => { live.wire.set(FixtureMode::BelowRelease); Ok(()) },
                            "fixture-stale" => { live.wire.set(FixtureMode::Stale); Ok(()) },
                            "fixture-frozen" => { live.wire.set(FixtureMode::Frozen); Ok(()) },
                            "fixture-wrong-aircraft" => { live.wire.set(FixtureMode::WrongAircraft); Ok(()) },
                            "fixture-none" => { live.wire.set(FixtureMode::None); Ok(()) },
                            "fixture-context-loss" => { live.process.present.store(false, Ordering::Release); Ok(()) },
                            "fixture-context-restore" => { live.process.present.store(true, Ordering::Release); live.wire.set(FixtureMode::Low); Ok(()) },
                            "finish" => { finished=true; live.coordinator.stop() },
                            _ => Err("Unknown fixed fixture action; numeric telemetry and tokens are never accepted here".into()),
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
        let inspected = live.coordinator.status().owner.as_ref().map(|owner| {
            crate::profile_session::inspection::Request::parse(
                &json!({"command":"profile_runtime_inspect","owner":owner,"offset":0,"limit":64}),
            )
            .and_then(|request| live.coordinator.inspect(&request))
            .and_then(|page| serde_json::to_value(page).map_err(|error| error.to_string()))
        });
        match inspected {
            Some(Ok(page)) => {
                handoff["inspection"] = page;
                handoff["inspectionError"] = Value::Null;
            }
            Some(Err(error)) => {
                handoff["inspection"] = Value::Null;
                handoff["inspectionError"] = json!(error);
            }
            None => {
                handoff["inspection"] = Value::Null;
                handoff["inspectionError"] = Value::Null;
            }
        }
        handoff["fixture"] = live.wire.status();
        handoff["fixtureProcessPresent"] = json!(live.process.present.load(Ordering::Acquire));
        handoff["games"] = live.games.status();
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
        "DCS/browser proof exceeded its bounded240-second window"
    );
}

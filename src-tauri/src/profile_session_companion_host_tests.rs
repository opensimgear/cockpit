use super::*;
use osg_companion_service::{NullSourceSink, Publication, PublicationUpload};
use osg_mapping::signal::ScalarSchema;
use osg_profiles::compiler_models::{BackendCapabilities, CapturedResource};
use osg_profiles::resources::{Availability, Connection, ResourceSummary, ResourceType, Route};

fn snapshot() -> CompileSnapshot {
    CompileSnapshot {
        inventory_session: "native".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    }
}
fn fixture() -> (Hub, Publication) {
    let (hub, publication, _) = fixture_with_sink(Arc::new(NullSourceSink));
    (hub, publication)
}
fn fixture_with_sink(
    sink: Arc<dyn osg_companion_service::SourceSink>,
) -> (Hub, Publication, String) {
    let hub = Hub::new("profile-session-companion".into(), sink);
    let offer = hub
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let pairing = hub.handle_wire(
        json!({"type":"pair","secret":offer["secret"],"companionId":"screen","name":"Screen"}),
        "peer",
        hub.now_ms(),
    );
    hub.command(&json!({"command":"companion_approve","requestId":pairing["requestId"],"companionId":"screen"})).unwrap();
    let secret = hub.handle_wire(
        json!({"type":"pair-status","requestId":pairing["requestId"],"secret":offer["secret"]}),
        "peer",
        hub.now_ms(),
    )["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let upload: PublicationUpload = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    let publication = Publication::validate(upload.clone()).unwrap();
    let mut command = serde_json::to_value(upload).unwrap();
    command["command"] = json!("companion_publish");
    hub.command(&command).unwrap();
    hub.command(&json!({"command":"companion_assign","instanceId":"instance","companionId":"screen","publicationHash":publication.publication_hash})).unwrap();
    (hub, publication, secret)
}
fn sources(hub: &Hub, publication: &Publication) -> CompanionSources {
    let mut plan = osg_profiles::compiler::compile(&osg_profiles::default_document(), &snapshot())
        .plan
        .unwrap();
    plan.companion_assignments
        .push(osg_profiles::CompanionAssignment {
            resource_id: "panel".into(),
            publication_hash: publication.publication_hash.clone(),
        });
    CompanionSources::prepare(
        &plan,
        vec![PreparedCompanion {
            resource_id: "panel".into(),
            proof: hub
                .prepare_profile_assignment("instance", &publication.publication_hash)
                .unwrap(),
        }],
    )
    .unwrap()
}
fn empty_sources() -> CompanionSources {
    CompanionSources::prepare(
        &osg_profiles::compiler::compile(&osg_profiles::default_document(), &snapshot())
            .plan
            .unwrap(),
        vec![],
    )
    .unwrap()
}

#[test]
fn takeover_inherits_original_assignment_and_default_journals_omitted_instance_release() {
    let (hub, publication) = fixture();
    let first = capture(&hub, &sources(&hub, &publication), &[]).unwrap();
    let old = receipts(&first).unwrap();
    hub.acquire_profile_assignment(&old[0].1).unwrap();
    let second = capture(&hub, &sources(&hub, &publication), &first).unwrap();
    let incoming = receipts(&second).unwrap();
    assert_ne!(
        old[0].1.owner(),
        incoming[0].1.owner(),
        "Coordinator session reuse cannot reuse assignment authority"
    );
    hub.acquire_profile_assignment(&incoming[0].1).unwrap();
    assert_eq!(
        hub.restore_profile_assignment(&old[0].1).unwrap(),
        osg_companion_service::profile::AssignmentRestore::OwnershipLost
    );
    let default = capture(&hub, &empty_sources(), &second).unwrap();
    assert_eq!(default.len(), 1);
    assert_eq!(default[0].kind, RELEASE);
    let release = receipts(&default).unwrap();
    restore(&hub, &release, true).unwrap();
    assert_eq!(
        hub.restore_profile_assignment(&incoming[0].1).unwrap(),
        osg_companion_service::profile::AssignmentRestore::AlreadyBaseline
    );
    // Repeated Default and Stop do not acquire or rearm a profile panel.
    assert!(capture(&hub, &empty_sources(), &default)
        .unwrap()
        .is_empty());
    restore(&hub, &release, false).unwrap();
    assert!(hub
        .validate_profile_assignment_owner(&incoming[0].1)
        .is_err());
}

#[test]
fn recovery_preserves_explicit_operator_assignment_and_rejects_malformed_journal_before_effects() {
    let (hub, publication) = fixture();
    let journal = capture(&hub, &sources(&hub, &publication), &[]).unwrap();
    let owned = receipts(&journal).unwrap();
    hub.acquire_profile_assignment(&owned[0].1).unwrap();
    let mut malformed = journal.clone();
    malformed[0].resource_id = "another-instance".into();
    assert!(receipts(&malformed).is_err());
    hub.validate_profile_assignment_owner(&owned[0].1).unwrap();
    let mut duplicate = journal.clone();
    let mut release = journal[0].clone();
    release.kind = RELEASE.into();
    duplicate.push(release);
    assert!(receipts(&duplicate).is_err());
    // An explicit operator takeover clears the stamp even if publication is the same.
    hub.command(&json!({"command":"companion_assign","instanceId":"instance","companionId":"screen","publicationHash":publication.publication_hash})).unwrap();
    let before = hub.assignment_snapshot(&["instance".into()]).unwrap();
    restore(&hub, &owned, false).unwrap();
    assert_eq!(
        hub.assignment_snapshot(&["instance".into()]).unwrap(),
        before
    );
    assert!(capture(&hub, &empty_sources(), &journal)
        .unwrap()
        .is_empty());
}

#[test]
fn prospective_catalogue_uses_retained_desired_publication_without_mutating_observed_metadata() {
    let (hub, publication) = fixture();
    let mut upload: PublicationUpload = serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap();
    upload.canonical_document = upload
        .canonical_document
        .replace("Flight instruments", "Changed instrument panel");
    use sha2::{Digest, Sha256};
    upload.publication_hash = format!(
        "sha256:{:x}",
        Sha256::digest(upload.canonical_document.as_bytes())
    );
    upload.capability_fingerprint = None;
    let desired = Publication::validate(upload.clone()).unwrap();
    let mut command = serde_json::to_value(upload).unwrap();
    command["command"] = json!("companion_publish");
    hub.command(&command).unwrap();
    assert_ne!(desired.publication_hash, publication.publication_hash);
    let mut candidate = Candidate {
        document: osg_profiles::default_document(),
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: Some(1),
        stage: None,
    };
    candidate
        .document
        .companion_assignments
        .push(osg_profiles::CompanionAssignment {
            resource_id: "panel".into(),
            publication_hash: desired.publication_hash.clone(),
        });
    let mut captured = snapshot();
    captured
        .assignments
        .push(osg_profiles::store::LocalAssignment {
            resource_id: "panel".into(),
            source_id: "inventory".into(),
            ports: vec![],
        });
    captured.resources.push(CapturedResource {
        summary: ResourceSummary {
            id: "inventory".into(),
            name: "Screen".into(),
            resource_type: ResourceType::Companion,
            availability: Availability::Online,
            generation: "1".into(),
            capability_revision: "current".into(),
            port_count: 0,
            cached_port_count: 0,
            connections: vec![Connection {
                scope: "companions".into(),
                key: "instance".into(),
                route: Route {
                    id: "companion:instance".into(),
                    transport: Transport::Network,
                    provider_generation: "1".into(),
                    attributes: BTreeMap::new(),
                    baseline_ready: false,
                    writable: false,
                    priority: 1,
                },
            }],
            attributes: BTreeMap::from([(
                "publicationHash".into(),
                publication.publication_hash.clone(),
            )]),
            diagnostics: vec![],
        },
        ports: vec![],
        tokens: vec![],
        provider_setup_revision: None,
        game_id: None,
    });
    let before = hub.assignment_snapshot(&["instance".into()]).unwrap();
    let (catalogues, bindings) = prepare_catalogues(&hub, &captured, &candidate).unwrap();
    assert_eq!(catalogues[0].publication_hash, desired.publication_hash);
    assert!(
        catalogues[0].ports.is_empty(),
        "Unused advertised ports do not consume the capture budget"
    );
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        captured.resources[0].summary.attributes["publicationHash"],
        publication.publication_hash
    );
    assert!(!admitted(&captured, &catalogues[0]));
    assert_eq!(
        hub.assignment_snapshot(&["instance".into()]).unwrap(),
        before
    );
    captured.resources[0].summary.connections.clear();
    assert!(prepare_catalogues(&hub, &captured, &candidate).is_err());
    candidate.document.companion_assignments[0].publication_hash =
        format!("sha256:{}", "f".repeat(64));
    assert!(prepare_catalogues(&hub, &captured, &candidate).is_err());
}

pub(crate) struct NativeFixture {
    _directory: tempfile::TempDir,
    hub: Hub,
    original: Publication,
    desired: Publication,
    resources: Arc<crate::profile_resources::Runtime>,
    controllers: Arc<osg_controller_input::ControllerService>,
    games: Arc<crate::games::GamesRuntime>,
    companions: Arc<crate::companion_host::Runtime>,
    epoch: Arc<AtomicU64>,
    quit: Arc<std::sync::atomic::AtomicBool>,
    peer: Option<std::thread::JoinHandle<()>>,
    reply: Arc<std::sync::Mutex<serde_json::Value>>,
}
impl NativeFixture {
    pub(crate) fn new() -> Self {
        use osg_profiles::resources::{EvidenceKind, IdentityEvidence, Observation, ProviderBatch};
        use osg_profiles::resources_ports::PortUpdate;
        let directory = tempfile::tempdir().unwrap();
        let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
        let clock = controllers.clone();
        let provider = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(move || clock.stream_page("", 0).captured_us),
        ));
        let (hub, original, secret) = fixture_with_sink(provider.clone());
        let mut upload: PublicationUpload = serde_json::from_str(include_str!(
            "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
        ))
        .unwrap();
        upload.canonical_document = upload
            .canonical_document
            .replace("Flight instruments", "Applied profile panel");
        use sha2::{Digest, Sha256};
        upload.publication_hash = format!(
            "sha256:{:x}",
            Sha256::digest(upload.canonical_document.as_bytes())
        );
        upload.capability_fingerprint = None;
        let desired = Publication::validate(upload.clone()).unwrap();
        let mut command = serde_json::to_value(upload).unwrap();
        command["command"] = json!("companion_publish");
        hub.command(&command).unwrap();
        let companions = Arc::new(crate::companion_host::Runtime::with_profile_metadata(
            hub.clone(),
            provider.clone(),
            directory.path().join("companions"),
        ));
        let observed = provider.clone();
        let metadata = companions.clone();
        let collect: crate::profile_resources::Collect = Arc::new(move |revision| {
            let result = (|| {
                let observations = observed
                    .resource_inventory()
                    .into_iter()
                    .map(|source| {
                        let publication =
                            metadata.resource_publication(&source.definition.source_id)?;
                        let ports = crate::profile_resource_ports::companion(
                            &source.definition,
                            &publication.feedback,
                        )?;
                        let transport = metadata.resource_transport(&source.definition.source_id);
                        Ok(Observation {
                            key: source.definition.source_id.clone(),
                            name: source.definition.name.clone(),
                            resource_type: ResourceType::Companion,
                            evidence: vec![IdentityEvidence {
                                kind: EvidenceKind::CompanionInstance,
                                value: source.definition.source_id.clone(),
                            }],
                            attributes: BTreeMap::from([
                                ("publicationHash".into(), publication.publication_hash),
                                (
                                    "capabilityFingerprint".into(),
                                    publication.capability_fingerprint,
                                ),
                            ]),
                            diagnostics: vec![],
                            route: source
                                .online
                                .then_some(transport)
                                .flatten()
                                .map(|transport| Route {
                                    id: format!("companion:{}", source.definition.source_id),
                                    transport: match transport {
                                        osg_companion_service::SourceTransport::Network => {
                                            Transport::Network
                                        }
                                        osg_companion_service::SourceTransport::Usb => {
                                            Transport::Usb
                                        }
                                    },
                                    provider_generation: source.generation.to_string(),
                                    attributes: BTreeMap::new(),
                                    baseline_ready: source.baseline_ready,
                                    writable: source.baseline_ready,
                                    priority: 0,
                                }),
                            ports: Some(PortUpdate::complete(ports)?),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(ProviderBatch {
                    scope: "companions".into(),
                    session: "actual-companion-provider".into(),
                    revision,
                    complete: true,
                    observations,
                })
            })();
            vec![("companions".into(), result)]
        });
        let resources = Arc::new(crate::profile_resources::Runtime::new(None, collect, false));
        let peer_hub = hub.clone();
        let peer_resources = resources.clone();
        let controls = original.controls.clone();
        let quit = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = quit.clone();
        let reply = Arc::new(std::sync::Mutex::new(json!({})));
        let captured_reply = reply.clone();
        let peer = std::thread::spawn(move || {
            let mut opened: Option<serde_json::Value> = None;
            let mut sequence = 0u64;
            while !stop.load(Ordering::Acquire) {
                if opened.is_none() {
                    peer_hub.handle_wire(
                        json!({"type":"auth","token":secret}),
                        "peer",
                        peer_hub.now_ms(),
                    );
                    let current = peer_hub.handle_wire(
                        json!({"type":"open","instanceId":"instance"}),
                        "peer",
                        peer_hub.now_ms(),
                    );
                    if current["ok"] == true {
                        let baseline = json!({"version":1,"type":"baseline","clientId":current["clientId"],"sessionId":current["sessionId"],"generation":current["generation"],"publicationHash":current["publicationHash"],"sequence":"0",
                            "values":controls.iter().map(|c|(c.id.clone(),c.fallback())).collect::<BTreeMap<_,_>>()});
                        if peer_hub.handle_wire(baseline, "peer", peer_hub.now_ms())["ok"] == true {
                            opened = Some(current);
                            sequence = 0;
                        }
                    }
                } else if let Some(current) = &opened {
                    sequence += 1;
                    let heartbeat = json!({"version":1,"type":"heartbeat","clientId":current["clientId"],"sessionId":current["sessionId"],"generation":current["generation"],"publicationHash":current["publicationHash"],"sequence":sequence.to_string()});
                    let received = peer_hub.handle_wire(heartbeat, "peer", peer_hub.now_ms());
                    if received["ok"] != true {
                        opened = None;
                    } else {
                        *captured_reply.lock().unwrap() = received;
                    }
                }
                peer_resources.refresh_native().unwrap();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while resources
            .request(&json!({"command":"profile_resources_list"}))
            .unwrap()["resources"]
            .as_array()
            .is_none_or(Vec::is_empty)
            || resources
                .request(&json!({"command":"profile_resources_list"}))
                .unwrap()["resources"][0]["connections"]
                .as_array()
                .is_none_or(Vec::is_empty)
        {
            assert!(
                Instant::now() < deadline,
                "Actual peer never established its initial native route"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let games = Arc::new(crate::games::GamesRuntime::new(
            Arc::new(|_| json!({"ok":false})),
            directory.path().join("games.json"),
            Arc::new(crate::game_discovery::NativePlatform),
        ));
        Self {
            _directory: directory,
            hub,
            original,
            desired,
            resources,
            controllers,
            games,
            companions,
            epoch: Arc::new(AtomicU64::new(1)),
            quit,
            peer: Some(peer),
            reply,
        }
    }
    pub(crate) fn host(&self) -> NativeProfileHost {
        NativeProfileHost::new(
            self.resources.clone(),
            self.controllers.clone(),
            self.games.clone(),
            self.companions.clone(),
            Arc::new(crate::gateways::ProfilePanelIo::empty()),
            false,
            self.epoch.clone(),
        )
    }
    pub(crate) fn candidate(&self, default: bool) -> Candidate {
        let mut candidate = Candidate {
            document: osg_profiles::default_document(),
            assignments: vec![],
            read_only: false,
            game_id: None,
            admission_epoch: Some(1),
            stage: None,
        };
        if !default {
            candidate.document = osg_profiles::new_document("Assigned companion").unwrap();
            let native = osg_mapping_host::companions::native_definition(
                self.desired.definition("instance"),
            )
            .unwrap();
            let ports =
                crate::profile_resource_ports::companion(&native, &self.desired.feedback).unwrap();
            let input = ports
                .iter()
                .find(|p| {
                    p.direction == osg_profiles::Direction::Output
                        && matches!(
                            p.contract,
                            osg_profiles::PortContract::State {
                                schema: ScalarSchema::Boolean {},
                                ..
                            }
                        )
                })
                .unwrap();
            let output = ports
                .iter()
                .find(|p| {
                    p.direction == osg_profiles::Direction::Input
                        && matches!(
                            p.contract,
                            osg_profiles::PortContract::State {
                                schema: ScalarSchema::Boolean {},
                                ..
                            }
                        )
                })
                .unwrap();
            candidate.document.resources.push(osg_profiles::Resource {
                id: "panel".into(),
                name: "Assigned screen".into(),
                kind: osg_profiles::ResourceKind::Companion,
                required: true,
                ports: vec![
                    osg_profiles::Port {
                        id: "button".into(),
                        name: "Button".into(),
                        direction: input.direction,
                        contract: input.contract.clone(),
                    },
                    osg_profiles::Port {
                        id: "indicator".into(),
                        name: "Indicator".into(),
                        direction: output.direction,
                        contract: output.contract.clone(),
                    },
                ],
            });
            candidate.document.bindings.push(osg_profiles::Binding {
                id: "feedback".into(),
                from: osg_profiles::Endpoint {
                    node_id: "panel".into(),
                    port_id: "button".into(),
                },
                to: osg_profiles::Endpoint {
                    node_id: "panel".into(),
                    port_id: "indicator".into(),
                },
            });
            candidate
                .document
                .companion_assignments
                .push(osg_profiles::CompanionAssignment {
                    resource_id: "panel".into(),
                    publication_hash: self.desired.publication_hash.clone(),
                });
            let id = self
                .resources
                .request(&json!({"command":"profile_resources_list"}))
                .unwrap()["resources"][0]["id"]
                .as_str()
                .unwrap()
                .to_owned();
            candidate
                .assignments
                .push(osg_profiles::store::LocalAssignment {
                    resource_id: "panel".into(),
                    source_id: id,
                    ports: vec![
                        osg_profiles::resources::PortExpectation {
                            profile_port_id: "button".into(),
                            observed_port_id: input.id.clone(),
                            expected_contract_hash: input.contract_hash().unwrap(),
                        },
                        osg_profiles::resources::PortExpectation {
                            profile_port_id: "indicator".into(),
                            observed_port_id: output.id.clone(),
                            expected_contract_hash: output.contract_hash().unwrap(),
                        },
                    ],
                });
        }
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
impl Drop for NativeFixture {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Release);
        if let Some(peer) = self.peer.take() {
            peer.join().unwrap();
        }
        self.resources.shutdown();
        self.games.shutdown();
        self.companions.shutdown();
        self.controllers.shutdown();
    }
}

#[test]
fn actual_host_acquires_desired_panel_reads_native_baseline_and_default_restores_original() {
    let fixture = NativeFixture::new();
    let journal = fixture._directory.path().join("session.json");
    let mut coordinator =
        crate::profile_session::Coordinator::new(fixture.host(), Some(journal.clone()));
    let stamp = || {
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
        saved["active"]["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == ASSIGNMENT)
            .unwrap()["state"]["desired"]["ownership"]
            .clone()
    };
    let candidate = fixture.candidate(false);
    coordinator.apply(candidate).unwrap();
    assert_eq!(
        fixture
            .hub
            .assignment_snapshot(&["instance".into()])
            .unwrap()["assignments"][0]["publicationHash"],
        fixture.desired.publication_hash
    );
    coordinator.poll().unwrap();
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    loop {
        coordinator.poll().unwrap();
        if fixture.reply.lock().unwrap()["feedback"]["values"][0]["value"] == false {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Native feedback did not reach the actual wire peer"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        fixture.reply.lock().unwrap()["feedback"]["values"][0]["provenance"],
        "local"
    );
    assert!(
        fixture
            .hub
            .clear_feedback(
                "instance",
                &fixture.desired.publication_hash,
                fixture.hub.now_ms()
            )
            .is_err(),
        "The legacy global feedback path cannot clear an owned panel"
    );
    let old_owner = coordinator.status().owner.clone().unwrap();
    let old_stamp = stamp();
    assert!(old_stamp["owner"].is_string());
    coordinator.apply(fixture.candidate(false)).unwrap();
    assert_eq!(
        coordinator.status().owner.as_ref().unwrap().session,
        old_owner.session
    );
    assert!(coordinator.status().owner.as_ref().unwrap().generation > old_owner.generation);
    assert_ne!(
        stamp(),
        old_stamp,
        "Each takeover has a distinct assignment receipt owner"
    );
    coordinator.apply(fixture.candidate(true)).unwrap();
    let restored = fixture
        .hub
        .assignment_snapshot(&["instance".into()])
        .unwrap();
    assert_eq!(
        restored["assignments"][0]["publicationHash"],
        fixture.original.publication_hash
    );
    coordinator.stop().unwrap();
    assert_eq!(
        fixture
            .hub
            .assignment_snapshot(&["instance".into()])
            .unwrap(),
        restored
    );
}

#[test]
fn actual_host_restart_restores_only_owned_original_panel_without_replaying_the_profile() {
    let fixture = NativeFixture::new();
    let journal = fixture._directory.path().join("profile-session.json");
    let mut coordinator =
        crate::profile_session::Coordinator::new(fixture.host(), Some(journal.clone()));
    coordinator.apply(fixture.candidate(false)).unwrap();
    coordinator.poll().unwrap();
    drop(coordinator); // Process loss leaves the durable intent and protected stamp.
    assert_eq!(
        fixture
            .hub
            .assignment_snapshot(&["instance".into()])
            .unwrap()["assignments"][0]["publicationHash"],
        fixture.desired.publication_hash
    );
    let mut restarted = crate::profile_session::Coordinator::new(fixture.host(), Some(journal));
    assert_eq!(
        restarted.status().state,
        crate::profile_session::State::RecoveryNeeded
    );
    assert!(restarted.status().owner.is_none());
    restarted.recover().unwrap();
    assert_eq!(
        restarted.status().state,
        crate::profile_session::State::Stopped
    );
    assert!(restarted.status().owner.is_none());
    assert_eq!(
        fixture
            .hub
            .assignment_snapshot(&["instance".into()])
            .unwrap()["assignments"][0]["publicationHash"],
        fixture.original.publication_hash
    );
    restarted.poll().unwrap();
    assert!(
        restarted.status().owner.is_none(),
        "Recovery never restarts the profile"
    );
}

#[test]
fn stale_native_preparation_fails_before_takeover_and_keeps_existing_panel_owner_renewing() {
    let fixture = NativeFixture::new();
    let mut coordinator = crate::profile_session::Coordinator::new(fixture.host(), None);
    coordinator.apply(fixture.candidate(false)).unwrap();
    let owner = coordinator.status().owner.clone().unwrap();
    let candidate = fixture.candidate(false);
    // Native setup changed while the candidate was staged. This update is
    // unrelated to the current panel, so failure must not revoke its run.
    let offer = fixture
        .hub
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let paired = fixture.hub.handle_wire(
        json!({"type":"pair","secret":offer["secret"],"companionId":"other-screen","name":"Other"}),
        "other-peer",
        fixture.hub.now_ms(),
    );
    fixture.hub.command(&json!({"command":"companion_approve","requestId":paired["requestId"],"companionId":"other-screen"})).unwrap();
    let error = coordinator.apply(candidate).unwrap_err();
    assert!(
        error.contains("changed after profile preparation"),
        "{error}"
    );
    assert_eq!(coordinator.status().owner.as_ref(), Some(&owner));
    coordinator.poll().unwrap();
    assert_eq!(coordinator.status().owner.as_ref(), Some(&owner));
    assert_eq!(
        fixture
            .hub
            .assignment_snapshot(&["instance".into()])
            .unwrap()["assignments"][0]["publicationHash"],
        fixture.desired.publication_hash
    );
    coordinator.stop().unwrap();
    assert_eq!(
        fixture
            .hub
            .assignment_snapshot(&["instance".into()])
            .unwrap()["assignments"][0]["publicationHash"],
        fixture.original.publication_hash
    );
}

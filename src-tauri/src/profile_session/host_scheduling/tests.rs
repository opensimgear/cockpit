use super::*;
use crate::game_discovery::{Platform, ProcessIdentity};
use osg_virtual_controller::{feeder::Message, session::{Channel, Factory, Session}};
use serde_json::Value;
use std::sync::Mutex;

struct NoGame;
impl Platform for NoGame {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> { vec![] }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> { Err("No game".into()) }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> { panic!("No launch") }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> { Ok(None) }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> { Ok(false) }
}

#[test]
fn capture_service_requires_retained_wakes_and_rejects_external_adapter_documents() {
    let directory = crate::test_directory();
    let resources = Arc::new(crate::profile_resources::Runtime::new(None, Arc::new(|_| vec![]), false));
    resources.refresh_native().unwrap();
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let games = Arc::new(crate::games::GamesRuntime::test_runtime(Arc::new(|_| panic!("No provider I/O")),
        crate::games::Paths { installations:directory.path().join("installations.json"),
            provider_setup:directory.path().join("setup.json"), recovery:directory.path().join("recovery.json") },
        Arc::new(NoGame)));
    let companions = Arc::new(crate::companion_host::Runtime::new(None,
        Arc::new(osg_mapping_host::companions::CompanionProvider::new(Arc::new(|| 0)))));
    let wake = Arc::new(osg_streams::NativeWake::default());
    let callback = wake.clone();
    let mut host = NativeProfileHost::new(resources.clone(), controllers.clone(), games.clone(), companions.clone(),
        Arc::new(crate::gateways::ProfilePanelIo::empty()), false, Arc::new(AtomicU64::new(7)))
        .with_native_wake(wake).with_input_wake(Arc::new(move || callback.notify()));
    let mut candidate = Candidate { document:osg_profiles::default_document(), assignments:vec![],
        virtual_slots:Default::default(), read_only:false, game_id:None, launch_preparation:false,
        admission_epoch:Some(7), stage:None };
    candidate.stage = Some(NativeProfileHost::stage(&resources, &games, &companions,
        controllers.clone(), false, &candidate).unwrap());
    let owner = RunOwner {session:uuid::Uuid::new_v4().to_string(), generation:1,
        profile_id:candidate.document.id.clone(),profile_revision:candidate.document.revision,game_id:None};
    let prepared = host.prepare(&candidate, &owner).unwrap();
    let mut run = OwnedRun { owner:owner.clone(),
        snapshot:osg_profiles::session::AppliedSnapshot::capture(&candidate.document).unwrap().summary().clone(),
        program_hash:prepared.program_hash.clone(),capture_fingerprint:prepared.capture_fingerprint.clone(),
        receipts:vec![],attempted:vec![] };
    let inputs = crate::profile_session_inputs::HidInputs::capture(&prepared.plan, controllers.as_ref(), None, |_| Ok(())).unwrap().0;
    let state = host.run(&owner).unwrap();
    state.inputs = Some(inputs);
    state.delivery = Some(prepared.plan.clone());
    assert!(!hold_uses_notifications(&host, &host.prepared[&key(&owner)]));
    host.native = true;
    assert_eq!(hold_uses_notifications(&host, &host.prepared[&key(&owner)]), cfg!(windows));
    let token = osg_mapping::engine::RunToken {
        session: owner.session.clone(), generation: Counter(owner.generation),
    };
    let factory: Factory = Arc::new(|| Ok(Box::new(Receipt(Arc::default()))));
    let output = Session::start(factory, token, vec![], Counter(0)).unwrap();
    let deadline = output.renewal_deadline();
    assert!(deadline.is_some());
    host.run(&owner).unwrap().output = Some(output);
    assert_eq!(host.hold_deadline(&run), deadline);
    assert_eq!(host.hold_uses_notifications(&run), cfg!(windows));
    let output = host.run(&owner).unwrap().output.as_mut().unwrap();
    output.stop().unwrap();
    assert_eq!(host.hold_deadline(&run), None);
    assert!(!host.hold_uses_notifications(&run));
    host.run(&owner).unwrap().output.take();
    host.native = false;
    let state = host.run(&owner).unwrap();
    state.reducer = Some(Phase::new(prepared.plan.program.clone(), Counter(1), owner.session.clone()).unwrap());
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run), "Non-native hosts keep service");
    host.native = true; // No native adapter is opened by this scheduling-policy test.
    assert!(!host.hold_uses_notifications(&run), "A run with a reducer cannot qualify as a hold");
    let supported = || capture_driven(&host, &host.prepared[&key(&owner)], &run);
    assert_eq!(supported(), cfg!(windows));
    host.run(&owner).unwrap().document.overrides.push(osg_profiles::RuntimeOverride {
        resource_id: "fixture-module".into(), enabled: false,
    });
    assert_eq!(capture_driven(&host, &host.prepared[&key(&owner)], &run), cfg!(windows));
    host.run(&owner).unwrap().document.overrides.clear();
    {
        use crate::profile_session_visibility::{tests::{plan, snapshot, Fake}, Visibility};
        let io = Fake::new();
        let snapshot = snapshot();
        let mut visibility = Visibility::stage(&plan(&snapshot), &snapshot, io.clone())
            .unwrap().bind(&owner).unwrap();
        visibility.start().unwrap();
        let state = host.run(&owner).unwrap();
        state.document.visibility.push(osg_profiles::VisibilityIntent {
            resource_id: "fixture-controller".into(), mode: osg_profiles::VisibilityMode::HiddenWhileActive,
        });
        let original = std::mem::replace(&mut state.visibility, visibility);
        assert_eq!(capture_driven(&host, &host.prepared[&key(&owner)], &run), cfg!(windows));
        let before = host.run(&owner).unwrap().reducer.as_ref().unwrap().at_us();
        let owned = io.owned.lock().unwrap().replace(uuid::Uuid::new_v4().to_string());
        host.authority = Some(owner.clone());
        assert!(host.health(&run).err().unwrap().contains("Another native visibility owner"));
        host.authority = None;
        let state = host.run(&owner).unwrap();
        assert_eq!(state.reducer.as_ref().unwrap().at_us(), before);
        *io.owned.lock().unwrap() = owned;
        state.visibility.stop().unwrap();
        state.visibility = original;
        state.document.visibility.clear();
    }
    let plan = host.run(&owner).unwrap().delivery.as_mut().unwrap();
    plan.sinks.push(osg_profiles::compiler_models::SinkPlan {
        endpoint: osg_profiles::Endpoint { node_id: "panel".into(), port_id: "led".into() },
        adapter: SinkAdapter::PhysicalState,
        inventory_resource_id: None, observed_port_id: None,
        contract: osg_profiles::resources_ports::PortContract::State {
            schema: osg_profiles::resources_ports::ScalarSchema::Boolean {},
            freshness_ms: 1000, fallback: None,
        },
        token: None, preparation: None, route: None, readiness: PlanReadiness::Ready,
    });
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run));
    host.run(&owner).unwrap().delivery.as_mut().unwrap().sinks.clear();
    let state = host.run(&owner).unwrap();
    state.document.desktop_outputs = serde_json::from_value(serde_json::json!([{
        "id":"keyboard","name":"Keyboard","required":true,"scope":{"kind":"desktop-wide"},
        "ports":[],"actions":[]
    }])).unwrap();
    assert!(document_supported(&state.document, false), "Desktop documents can qualify with retained native scope wakes");
    assert!(!desktop_notifications(state), "A declared desktop adapter without a live subscription cannot opt out of service");
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run));
    host.run(&owner).unwrap().document.desktop_outputs.clear();
    host.input_wake.take();
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run));
    let state = host.run(&owner).unwrap();
    state.document.eligibility.game_ids.push("dcs".into());
    assert!(!document_supported(&state.document, false));
    assert!(document_supported(&state.source_document, false));
    state.document.resources.push(osg_profiles::Resource { id:"game".into(),name:"Game".into(),
        kind:osg_profiles::ResourceKind::Game, required:false,ports:vec![] });
    assert!(document_supported(&state.document, true), "A watched game can qualify its telemetry resource");
    assert!(!document_supported(&state.document, false));
    state.document.resources.clear();
    state.document.eligibility.game_ids.clear();
    state.document.resources.push(osg_profiles::Resource { id:"companion".into(),name:"Companion".into(),
        kind:osg_profiles::ResourceKind::Companion, required:false,ports:vec![] });
    assert!(document_supported(&state.document, false), "Companion declarations still require a live notification-qualified adapter");
    let (game, proof, close) = crate::profile_session_games::watched_assetto_scheduling_fixture(
        &prepared.plan, host.native_wake.clone().unwrap(),
    );
    host.input_wake = Some(Arc::new(|| {}));
    run.owner.game_id = Some(proof.proof.game_id.clone());
    let state = host.run(&owner).unwrap();
    state.owner.game_id = run.owner.game_id.clone();
    state.game_preparation = Some(proof);
    state.game = game;
    state.source_document.vehicle_tabs.push(osg_profiles::VehicleTab {
        id: "fixture-vehicle".into(), game_id: run.owner.game_id.clone().unwrap(),
        vehicle_id: "fixture".into(), vehicle_name: "Fixture".into(), node_ids: vec![],
    });
    assert!(!document_supported(&state.source_document, false));
    assert_eq!(capture_driven(&host, &host.prepared[&key(&owner)], &run), cfg!(windows));
    host.prepared.get_mut(&key(&owner)).unwrap().vehicle_paused = true;
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run));
    host.prepared.get_mut(&key(&owner)).unwrap().vehicle_paused = false;
    host.prepared.get_mut(&key(&owner)).unwrap().source_document.vehicle_tabs.clear();
    host.prepared.get_mut(&key(&owner)).unwrap().game_publication.deferred = true;
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run));
    host.prepared.get_mut(&key(&owner)).unwrap().game_publication.deferred = false;
    close();
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run));
    let wake = host.native_wake.clone().unwrap();
    let (game, proof, publish) = crate::profile_session_games::watched_xplane_scheduling_fixture(
        &prepared.plan, &games, wake.clone(),
    );
    let state = host.prepared.get_mut(&key(&owner)).unwrap();
    run.owner.game_id = Some(proof.proof.game_id.clone());
    state.owner.game_id = run.owner.game_id.clone();
    state.game_preparation = Some(proof);
    state.game = game;
    assert_eq!(capture_driven(&host, &host.prepared[&key(&owner)], &run), cfg!(windows));
    let observed = wake.observed();
    publish(42);
    assert!(wake.observed() > observed, "A native X-Plane commit must wake the profile host");
    host.prepared.get_mut(&key(&owner)).unwrap().game.stop();
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run));
    let (game, proof, _catalogue) = crate::profile_session_games::watched_dcs_scheduling_fixture(
        &games, host.native_wake.clone().unwrap(),
    );
    let state = host.prepared.get_mut(&key(&owner)).unwrap();
    run.owner.game_id = Some(proof.proof.game_id.clone());
    state.owner.game_id = run.owner.game_id.clone();
    state.game_preparation = Some(proof);
    state.game = game;
    assert_eq!(capture_driven(&host, &host.prepared[&key(&owner)], &run), cfg!(windows));
    let state = host.prepared.get_mut(&key(&owner)).unwrap();
    state.delivery.as_mut().unwrap().sinks.push(osg_profiles::compiler_models::SinkPlan {
        endpoint: osg_profiles::Endpoint { node_id: "game".into(), port_id: "command".into() },
        adapter: SinkAdapter::SimulatorCommand,
        inventory_resource_id: None, observed_port_id: None,
        contract: osg_profiles::resources_ports::PortContract::Action {
            parameters: vec![], endpoint: "command".into(), readable: false,
            semantics: osg_profiles::resources_ports::ActionSemantics::OneShot,
            display: None, ownership: None,
        },
        token: None, preparation: None, route: None, readiness: PlanReadiness::Dormant,
    });
    assert_eq!(host.service_interval(&run).is_none(), cfg!(windows));
    let state = host.prepared.get_mut(&key(&owner)).unwrap();
    let proof = state.game_preparation.as_mut().unwrap();
    let integration = std::mem::replace(&mut proof.integration_key, "x-plane".into());
    assert!(host.service_interval(&run).is_some(), "Other integrations cannot qualify a simulator command sink");
    let state = host.prepared.get_mut(&key(&owner)).unwrap();
    state.game_preparation.as_mut().unwrap().integration_key = integration;
    state.game.stop();
    assert!(host.service_interval(&run).is_some(), "Losing the DCS watch restores fallback service");
    host.prepared.clear();
    games.shutdown(); companions.shutdown(); resources.shutdown(); controllers.shutdown();
}

struct Receipt(Arc<Mutex<Vec<u64>>>);
impl Channel for Receipt {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        if let Message::Report { report, .. } = message { self.0.lock().unwrap().push(report.sequence.0); }
        Ok(())
    }
    fn alive(&mut self) -> bool { true }
    fn terminate(&mut self) {}
}

#[test]
fn unfinished_output_drain_runs_before_renewal_without_moving_the_lease() {
    use osg_mapping::engine::{Report, ReportReason, RunToken, TargetState};
    let received = Arc::new(Mutex::new(vec![]));
    let peer = received.clone();
    let factory: Factory = Arc::new(move || Ok(Box::new(Receipt(peer.clone()))));
    let token = RunToken { session:uuid::Uuid::new_v4().to_string(), generation:Counter(1) };
    let mut output = Session::start(factory, token.clone(), vec![], Counter(0)).unwrap();
    let mut recording = RecordingSink::default();
    recording.record((1..=300).map(|sequence| Report { session:token.session.clone(),run_generation:token.generation,
        sequence:Counter(sequence),at_us:Counter(sequence),target:"fixture".into(),reason:ReportReason::Change,
        state:TargetState {buttons:vec![sequence % 2 == 0],axes:vec![],hats:vec![]} }).collect());
    output.poll(&recording).unwrap();
    output.poll(&recording).unwrap();
    assert!(output.cursor().0 <= 256 && output.cursor().0 < recording.cursor().0);
    let renewal = output.renewal_deadline();
    let due = output_deadline(output.cursor(), recording.cursor(), renewal).unwrap();
    assert!(due <= Instant::now());
    assert_eq!(output.renewal_deadline(), renewal, "Querying pending dispatch cannot renew authority");
    for _ in 0..300 {
        if output.cursor() == recording.cursor() { break; }
        output.poll(&recording).unwrap();
    }
    assert_eq!(output.cursor(), recording.cursor());
    assert_eq!(*received.lock().unwrap(), (1..=300).collect::<Vec<_>>(), "A stopped burst must drain every ordered edge");
    assert_eq!(output_deadline(output.cursor(), recording.cursor(), output.renewal_deadline()), output.renewal_deadline());
    output.stop().unwrap();
    assert_eq!(output_deadline(Counter(300), Counter(300), None), None);
}

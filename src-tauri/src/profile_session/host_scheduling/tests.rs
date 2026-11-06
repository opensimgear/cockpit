use super::*;
use std::time::Duration;
use crate::game_discovery::{Platform, ProcessIdentity};
use serde_json::Value;

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
    state.reducer = Some(Phase::new(prepared.plan.program.clone(), Counter(1), owner.session.clone()).unwrap());
    assert!(!capture_driven(&host, &host.prepared[&key(&owner)], &run), "Non-native hosts keep service");
    host.native = true; // No native adapter is opened by this scheduling-policy test.
    let supported = || capture_driven(&host, &host.prepared[&key(&owner)], &run);
    assert_eq!(supported(), cfg!(windows));
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
    assert_eq!(capture_driven(&host, &host.prepared[&key(&owner)], &run), cfg!(windows));
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
    host.prepared.clear();
    games.shutdown(); companions.shutdown(); resources.shutdown(); controllers.shutdown();
}

#[test]
fn unfinished_output_drain_runs_before_renewal_without_moving_the_lease() {
    use osg_virtual_controller::{feeder::Message, session::{Channel, Factory, Session}};
    use osg_mapping::engine::{Report, ReportReason, RunToken, TargetState};
    use std::sync::Mutex;
    struct Receipt(Arc<Mutex<Vec<u64>>>);
    impl Channel for Receipt {
        fn request(&mut self, message: &Message) -> Result<(), String> {
            if let Message::Report { report, .. } = message { self.0.lock().unwrap().push(report.sequence.0); }
            Ok(())
        }
        fn alive(&mut self) -> bool { true }
        fn terminate(&mut self) {}
    }
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

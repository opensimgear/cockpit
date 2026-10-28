use super::*;
use crate::{game_discovery::Platform, profile_session::Stage};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

struct PlatformFixture {
    process: Mutex<Option<ProcessIdentity>>,
    spawns: AtomicUsize,
    fail: AtomicBool,
    appears: AtomicBool,
    alive: AtomicBool,
}
impl PlatformFixture {
    fn new() -> Self {
        Self {
            process: Mutex::new(None),
            spawns: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            appears: AtomicBool::new(true),
            alive: AtomicBool::new(true),
        }
    }
    fn process() -> ProcessIdentity {
        ProcessIdentity {
            pid: 42,
            epoch: 987,
            path: "C:\\Games\\DCS\\bin\\DCS.exe".into(),
        }
    }
}
impl Platform for PlatformFixture {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![json!({"id":"dcs","installed":true,"launchSupported":true,
            "processMonitoringSupported":true,"executable":"C:\\Games\\DCS\\bin\\DCS.exe"}),
            json!({"id":"x-plane-12","installed":false,"launchSupported":false,"processMonitoringSupported":true})]
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        if path.starts_with("C:\\") {
            Ok(())
        } else {
            Err("invalid executable".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("the blocking legacy launcher must not be used")
    }
    fn spawn_process(&self, _: &Value) -> Result<(), String> {
        self.spawns.fetch_add(1, Ordering::AcqRel);
        if self.appears.load(Ordering::Acquire) {
            *self.process.lock().unwrap() = Some(Self::process());
        }
        if self.fail.load(Ordering::Acquire) {
            Err("uncertain native spawn".into())
        } else {
            Ok(())
        }
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(self.process.lock().unwrap().clone())
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(self.alive.load(Ordering::Acquire))
    }
}
fn fixture() -> (
    GamesRuntime,
    Arc<PlatformFixture>,
    Candidate,
    tempfile::TempDir,
) {
    let directory = crate::test_directory();
    let platform = Arc::new(PlatformFixture::new());
    let runtime = GamesRuntime::test_runtime(
        Arc::new(|_| panic!("launch helper must not touch hardware/provider commands")),
        {
            let fixture_metadata_path = directory.path().join("games.json");
            crate::games::Paths {
                installations: fixture_metadata_path.with_extension("installations-v1.json"),
                provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                recovery: fixture_metadata_path.with_extension("recovery.json"),
            }
        },
        platform.clone());
    let mut candidate = Candidate { virtual_slots: Default::default(), launch_preparation: false,
        document: osg_profiles::default_document(),
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: Some(41),
        stage: None,
    };
    candidate.launch_preparation=true;
    candidate.stage = Some(Stage::new(&candidate, ()).unwrap());
    (runtime, platform, candidate, directory)
}
fn admission(candidate: &Candidate) -> LaunchAdmission {
    LaunchAdmission {
        candidate: candidate.clone(),
        admission_epoch: 42,
        locks: Locks::default(),
        status: Status {
            state: State::AwaitingGame,
            applied_snapshot: Some(
                osg_profiles::session::AppliedSnapshot::capture(&candidate.document)
                    .unwrap()
                    .summary()
                    .clone(),
            ),
            owner: Some(RunOwner {
                session: "11111111-1111-4111-8111-111111111111".into(),
                generation: 2,
                profile_id: candidate.document.id.clone(),
                profile_revision: candidate.document.revision,
                game_id: None,
            }),
            degradation: vec![],
            fault: None,
        },
    }
}
fn arm(runtime: &GamesRuntime, candidate: &Candidate) -> ArmedLaunch {
    runtime
        .arm_profile_launch(
            runtime.prepare_profile_launch("dcs", candidate).unwrap(),
            &admission(candidate),
        )
        .unwrap()
}
fn spawn(runtime: &GamesRuntime, candidate: &Candidate) -> SpawnedLaunch {
    runtime
        .spawn_profile_launch(arm(runtime, candidate), |spawn| spawn(admission(candidate)))
        .unwrap_or_else(|error| panic!("{}", error.message))
}

#[test]
fn actual_native_stage_and_exact_hardware_run_precede_one_short_spawn_and_read_only_wait() {
    let (runtime, platform, candidate, _directory) = fixture();
    let prepared = runtime.prepare_profile_launch("dcs", &candidate).unwrap();
    assert_eq!(platform.spawns.load(Ordering::Acquire), 0);
    assert!(!prepared.enumeration.requires_startup_notice());
    // The actual Apply consumes its payload; the launch proof cannot replay it.
    candidate
        .stage
        .as_ref()
        .unwrap()
        .take::<()>(&candidate)
        .unwrap();
    let armed = runtime
        .arm_profile_launch(prepared, &admission(&candidate))
        .unwrap();
    let spawned = runtime
        .spawn_profile_launch(armed, |spawn| spawn(admission(&candidate)))
        .unwrap_or_else(|e| panic!("{}", e.message));
    let expected_owner = admission(&candidate).status.owner.unwrap();
    let started = runtime
        .wait_profile_launch(spawned, |owner, epoch| {
            assert_eq!(owner, &expected_owner);
            assert_eq!(epoch, 42);
            Ok(())
        })
        .unwrap();
    assert_eq!(started.process, PlatformFixture::process());
    assert_eq!(started.identity.game_id, "dcs");
    assert_eq!(started.owner, expected_owner);
    assert_eq!(platform.spawns.load(Ordering::Acquire), 1);
    runtime.shutdown();
}
#[test]
fn unstaged_read_only_game_scoped_ineligible_and_already_running_candidates_do_not_spawn() {
    let (runtime, platform, candidate, _directory) = fixture();
    for variant in 0..6 {
        let mut invalid = candidate.clone();
        match variant {
            0 => invalid.stage = None,
            1 => invalid.read_only = true,
            2 => invalid.game_id = Some("dcs".into()),
            3 => invalid.admission_epoch = None,
            4 => invalid.document.eligibility.game_ids = vec!["x-plane-12".into()],
            _ => invalid.launch_preparation=false,
        }
        assert!(runtime.prepare_profile_launch("dcs", &invalid).is_err());
    }
    *platform.process.lock().unwrap() = Some(PlatformFixture::process());
    assert!(runtime.prepare_profile_launch("dcs", &candidate).is_err());
    assert_eq!(platform.spawns.load(Ordering::Acquire), 0);
    runtime.shutdown();
}
#[test]
fn arming_requires_exact_ready_snapshot_and_respects_every_native_operator_latch() {
    let (runtime, platform, candidate, _directory) = fixture();
    for variant in 0..11 {
        let mut state = admission(&candidate);
        match variant {
            0 => state.status.state = State::Degraded,
            1 => state.status.state = State::Transitioning,
            2 => state.locks.emergency_stopped = true,
            3 => state.locks.recovery_needed = true,
            4 => state.locks.faulted = true,
            5 => state.status.fault = Some("fault".into()),
            6 => state.status.owner.as_mut().unwrap().game_id = Some("dcs".into()),
            7 => {
                state
                    .status
                    .applied_snapshot
                    .as_mut()
                    .unwrap()
                    .semantic_hash = "sha256:changed".into()
            }
            8 => state.status.owner.as_mut().unwrap().profile_revision += 1,
            9 => state.status.state = State::Running,
            _ => state.candidate.document.revision += 1,
        }
        let prepared = runtime.prepare_profile_launch("dcs", &candidate).unwrap();
        assert!(
            runtime.arm_profile_launch(prepared, &state).is_err(),
            "variant {variant}"
        );
    }
    let mut prepared = runtime.prepare_profile_launch("dcs", &candidate).unwrap();
    prepared.prepared_at = Instant::now() - PREPARATION_TTL - Duration::from_secs(1);
    assert!(runtime
        .arm_profile_launch(prepared, &admission(&candidate))
        .is_err());
    assert_eq!(platform.spawns.load(Ordering::Acquire), 0);
    runtime.shutdown();
}

#[test]
fn started_process_proof_is_reverified_without_optional_integration_setup() {
    let (runtime, platform, candidate, _directory) = fixture();
    let started = runtime
        .wait_profile_launch(spawn(&runtime, &candidate), |_, _| Ok(()))
        .unwrap();
    runtime.validate_started_profile_game(&started).unwrap();
    runtime.inner.activation.observe(
        platform.as_ref(),
        &runtime.inner.settings.lock().unwrap().catalogue,
    );
    runtime.inner.activation.observe(
        platform.as_ref(),
        &runtime.inner.settings.lock().unwrap().catalogue,
    );
    assert!(runtime
        .confirm_started_profile_activation(&started)
        .unwrap()
        .is_some());
    platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
    assert!(runtime.validate_started_profile_game(&started).is_err());
    assert!(runtime
        .confirm_started_profile_activation(&started)
        .is_err());
    *platform.process.lock().unwrap() = Some(started.process.clone());
    runtime.validate_started_profile_game(&started).unwrap();
    platform.alive.store(false, Ordering::Release);
    assert!(runtime.validate_started_profile_game(&started).is_err());
    runtime.shutdown();
}
#[test]
fn externally_started_process_after_preparation_is_preserved_without_a_second_spawn() {
    let (runtime, platform, candidate, _directory) = fixture();
    let armed = arm(&runtime, &candidate);
    let external = PlatformFixture::process();
    *platform.process.lock().unwrap() = Some(external.clone());
    let error = runtime
        .spawn_profile_launch(armed, |spawn| spawn(admission(&candidate)))
        .err()
        .unwrap();
    assert!(error.message.contains("appeared after preparation"));
    assert!(!error.process_may_have_started);
    assert_eq!(error.cleanup_owner, admission(&candidate).status.owner);
    assert_eq!(platform.spawns.load(Ordering::Acquire), 0);
    assert_eq!(*platform.process.lock().unwrap(), Some(external));
    runtime.shutdown();
}

#[test]
fn effective_native_context_controls_eligibility_without_fabricating_a_process_for_the_request() {
    let (runtime, platform, mut candidate, _directory) = fixture();
    candidate.document.eligibility.game_ids = vec!["x-plane-12".into()];
    candidate.stage = Some(Stage::new(&candidate, ()).unwrap());
    assert!(runtime.prepare_profile_launch("dcs", &candidate).is_err());
    runtime
        .prepare_profile_launch_for_selection("dcs", &candidate, None)
        .unwrap();
    runtime
        .prepare_profile_launch_for_selection("dcs", &candidate, Some("x-plane-12"))
        .unwrap();
    assert!(runtime
        .prepare_profile_launch_for_selection("dcs", &candidate, Some("unknown"))
        .is_err());
    assert_eq!(platform.spawns.load(Ordering::Acquire), 0);
    runtime.shutdown();
}
#[test]
fn replacement_run_or_post_commit_operator_epoch_invalidates_armed_launch() {
    let (runtime, platform, candidate, _directory) = fixture();
    for variant in 0..3 {
        let armed = arm(&runtime, &candidate);
        let mut state = admission(&candidate);
        match variant {
            0 => state.admission_epoch += 1,
            1 => state.status.owner.as_mut().unwrap().generation += 1,
            _ => state
                .candidate
                .assignments
                .push(osg_profiles::store::LocalAssignment {
                    resource_id: "changed".into(),
                    source_id: "native-id".into(),
                    ports: vec![],
                }),
        }
        let error = runtime
            .spawn_profile_launch(armed, |spawn| spawn(state))
            .err()
            .unwrap();
        assert!(!error.process_may_have_started);
        assert!(error.cleanup_owner.is_none());
    }
    assert_eq!(platform.spawns.load(Ordering::Acquire), 0);
    runtime.shutdown();
}
#[test]
fn changed_installation_and_missing_native_spawn_admission_are_not_launched() {
    let (runtime, platform, candidate, _directory) = fixture();
    let armed = arm(&runtime, &candidate);
    runtime.inner.settings.lock().unwrap().catalogue[0]["executable"] = json!("C:\\Other\\DCS.exe");
    assert!(runtime
        .spawn_profile_launch(armed, |spawn| spawn(admission(&candidate)))
        .is_err());
    let armed = arm(&runtime, &candidate);
    let error = runtime
        .spawn_profile_launch(armed, |_| Ok(()))
        .err()
        .unwrap();
    assert!(!error.process_may_have_started);
    assert!(error.cleanup_owner.is_none());
    assert_eq!(platform.spawns.load(Ordering::Acquire), 0);
    runtime.shutdown();
}
#[test]
fn repeated_or_uncertain_spawn_has_one_effect_and_only_exact_owned_cleanup_disposition() {
    let (runtime, platform, candidate, _directory) = fixture();
    let armed = arm(&runtime, &candidate);
    let error = runtime
        .spawn_profile_launch(armed, |spawn| {
            spawn(admission(&candidate))?;
            let _ignored = spawn(admission(&candidate));
            Ok(())
        })
        .err()
        .unwrap();
    assert!(error.message.contains("twice"));
    assert_eq!(error.cleanup_owner, admission(&candidate).status.owner);
    assert!(error.process_may_have_started);
    assert_eq!(platform.spawns.load(Ordering::Acquire), 1);
    *platform.process.lock().unwrap() = None;
    platform.fail.store(true, Ordering::Release);
    let armed = arm(&runtime, &candidate);
    let error = runtime
        .spawn_profile_launch(armed, |spawn| {
            let _ignored = spawn(admission(&candidate));
            Ok(())
        })
        .err()
        .unwrap();
    assert!(error.message.contains("uncertain"));
    assert_eq!(error.cleanup_owner, admission(&candidate).status.owner);
    assert_eq!(platform.spawns.load(Ordering::Acquire), 2);
    runtime.shutdown();
}
#[test]
fn stop_during_native_process_read_discards_late_success_without_relaunch_or_killing_process() {
    let (runtime, platform, candidate, _directory) = fixture();
    let spawned = spawn(&runtime, &candidate);
    let mut checks = 0;
    let error = runtime
        .wait_profile_launch(spawned, |_, _| {
            checks += 1;
            if checks > 1 {
                Err("Stop revoked this owner".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(error.message.contains("Stop"));
    assert_eq!(error.cleanup_owner, admission(&candidate).status.owner);
    assert!(platform.process.lock().unwrap().is_some());
    assert_eq!(platform.spawns.load(Ordering::Acquire), 1);
    runtime.shutdown();
}
#[test]
fn deadline_and_lost_or_changed_process_never_replay_launch() {
    let (runtime, platform, candidate, _directory) = fixture();
    platform.appears.store(false, Ordering::Release);
    let error = runtime
        .wait_profile_launch_with(
            spawn(&runtime, &candidate),
            |_, _| Ok(()),
            Duration::ZERO,
            |_| panic!("expired wait cannot sleep"),
        )
        .unwrap_err();
    assert!(error.message.contains("deadline"));
    platform.appears.store(true, Ordering::Release);
    let spawned = spawn(&runtime, &candidate);
    runtime.inner.settings.lock().unwrap().catalogue[0]["launchSupported"] = json!(false);
    assert!(runtime.wait_profile_launch(spawned, |_, _| Ok(())).is_err());
    assert_eq!(platform.spawns.load(Ordering::Acquire), 2);
    runtime.shutdown();
}
#[test]
fn wait_is_off_admission_and_requires_a_live_valid_native_process() {
    let (runtime, platform, candidate, _directory) = fixture();
    let spawned = spawn(&runtime, &candidate);
    platform.alive.store(false, Ordering::Release);
    let mut paused = false;
    let started = runtime
        .wait_profile_launch_with(
            spawned,
            |_, _| Ok(()),
            Duration::from_secs(1),
            |_| {
                // This runs outside the short admission callback; a real Manager can
                // admit Stop while the worker waits. No locks are held by this helper.
                paused = true;
                platform.alive.store(true, Ordering::Release);
            },
        )
        .unwrap();
    assert!(paused);
    assert_eq!(started.process.epoch, 987);
    *platform.process.lock().unwrap() = None;
    let spawned = spawn(&runtime, &candidate);
    platform.process.lock().unwrap().as_mut().unwrap().pid = 0;
    assert!(runtime
        .wait_profile_launch(spawned, |_, _| Ok(()))
        .unwrap_err()
        .message
        .contains("process ID"));
    runtime.shutdown();
}
#[test]
fn restart_notice_follows_explicit_visibility_or_virtual_enumeration_intents_only() {
    let document = osg_profiles::ProfileDocument::parse(include_bytes!(
        "../../tests/fixtures/profiles/mixed.json"
    ))
    .unwrap();
    let intents = enumeration_intents(&document);
    assert!(intents.requires_startup_notice());
    assert!(!intents.virtual_controllers.is_empty());
    assert_eq!(
        enumeration_intents(&osg_profiles::default_document()),
        EnumerationIntents::default()
    );
}

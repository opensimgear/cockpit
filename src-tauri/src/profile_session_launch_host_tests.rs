use super::*;
use crate::game_discovery::{Platform, ProcessIdentity};
use osg_profiles::{compiler_models::*, resources::*, resources_ports::*};
use std::sync::Mutex;

struct Process(
    Mutex<Option<ProcessIdentity>>,
    std::sync::atomic::AtomicBool,
    std::sync::atomic::AtomicBool,
);
fn secondary_process() -> ProcessIdentity {
    ProcessIdentity {
        pid: 43,
        path: "C:\\Fixture\\X-Plane11.exe".into(),
        epoch: 20,
    }
}
impl Platform for Process {
    fn integration_setup_supported(&self) -> bool {
        self.1.load(Ordering::Acquire)
    }
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<serde_json::Value> {
        vec![
            json!({"id":"x-plane-12","name":"X-Plane 12","provider":"x-plane","installed":true,"executable":"C:\\Fixture\\X-Plane.exe","launchSupported":true,"processMonitoringSupported":true}),
            json!({"id":"x-plane-11","name":"X-Plane 11","provider":"x-plane","installed":true,"executable":"C:\\Fixture\\X-Plane11.exe","launchSupported":true,"processMonitoringSupported":true}),
        ]
    }
    fn validate(&self, _: &str, path: &str) -> Result<(), String> {
        if matches!(
            path,
            "C:\\Fixture\\X-Plane.exe" | "C:\\Fixture\\X-Plane11.exe"
        ) {
            Ok(())
        } else {
            Err("Wrong image".into())
        }
    }
    fn launch(&self, _: &serde_json::Value) -> Result<ProcessIdentity, String> {
        Err("Host must not spawn".into())
    }
    fn find(&self, game: &serde_json::Value) -> Result<Option<ProcessIdentity>, String> {
        if game["id"] == "x-plane-11" {
            return Ok(self.2.load(Ordering::Acquire).then(secondary_process));
        }
        Ok(self.0.lock().unwrap().clone())
    }
    fn alive(&self, p: &ProcessIdentity) -> Result<bool, String> {
        Ok(self.0.lock().unwrap().as_ref() == Some(p)
            || self.2.load(Ordering::Acquire) && p == &secondary_process())
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    host: NativeProfileHost,
    process: Arc<Process>,
    run: OwnedRun,
    candidate: Candidate,
}
impl Fixture {
    fn new() -> Self {
        let directory = crate::test_directory();
        let process = Arc::new(Process(
            Mutex::new(Some(ProcessIdentity {
                pid: 42,
                path: "C:\\Fixture\\X-Plane.exe".into(),
                epoch: 10,
            })),
            std::sync::atomic::AtomicBool::new(true),
            std::sync::atomic::AtomicBool::new(false),
        ));
        let games = Arc::new(crate::games::GamesRuntime::test_runtime(
            Arc::new(
                |_| json!({"ok":true,"state":{"integration":{"running":false},"dcsBios":{"running":false}}}),
            ),
            {
                let fixture_metadata_path = directory.path().join("games.json");
                crate::games::Paths {
                    installations: fixture_metadata_path.with_extension("installations-v1.json"),
                    provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                    recovery: fixture_metadata_path.with_extension("recovery.json"),
                }
            },
            process.clone(),
        ));
        let revision = games.resource_catalogue()[0]["integrationSetup"]["providerSetupRevision"]
            .as_str()
            .unwrap()
            .to_owned();
        let response=games.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":crate::xplane::defaults(12)}));
        assert!(response.is_ok(), "{response:?}");
        let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
        let resources = Arc::new(crate::profile_resources::Runtime::new(
            None,
            Arc::new(|_| vec![]),
            false,
        ));
        let provider = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(|| 0),
        ));
        let companions = Arc::new(crate::companion_host::Runtime::new(None, provider));
        let mut host = NativeProfileHost::new(
            resources.clone(),
            controllers.clone(),
            games.clone(),
            companions.clone(),
            Arc::new(crate::gateways::ProfilePanelIo::empty()),
            false,
            Arc::new(AtomicU64::new(1)),
        );
        let mut candidate = Candidate { virtual_slots: Default::default(), launch_preparation: false,
            document: osg_profiles::default_document(),
            assignments: vec![],
            read_only: false,
            game_id: None,
            admission_epoch: Some(1),
            stage: None,
        };
        candidate.launch_preparation=true;
        candidate.stage = Some(
            NativeProfileHost::stage(
                &resources,
                &games,
                &companions,
                controllers,
                false,
                &candidate,
            )
            .unwrap(),
        );
        let owner = RunOwner {
            session: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            profile_id: candidate.document.id.clone(),
            profile_revision: 1,
            game_id: None,
        };
        let prepared = host.prepare(&candidate, &owner).unwrap();
        let receipts = host.capture(&prepared, &owner, None).unwrap();
        let mut run = OwnedRun {
            owner,
            snapshot: osg_profiles::session::AppliedSnapshot::capture(&candidate.document)
                .unwrap()
                .summary()
                .clone(),
            program_hash: prepared.program_hash.clone(),
            capture_fingerprint: prepared.capture_fingerprint.clone(),
            receipts,
            attempted: vec![],
        };
        for phase in [
            ApplyPhase::Overrides,
            ApplyPhase::Acquire,
            ApplyPhase::Baseline,
            ApplyPhase::Visibility,
        ] {
            run.attempted.push(phase);
            host.enter(phase, &prepared, &run).unwrap();
        }
        Self {
            _directory: directory,
            host,
            process,
            run,
            candidate,
        }
    }
    fn bind(&mut self) {
        self.host.admission_epoch.store(2, Ordering::Release);
        self.host.bind_launch_epoch(&self.run, 2).unwrap();
    }
    fn game_candidate(&self) -> Candidate {
        let mut c = self.candidate.clone();
        c.launch_preparation=false;
        c.game_id = Some("x-plane-12".into());
        c.admission_epoch = Some(2);
        c.stage = None;
        c
    }
    fn started(&self) -> crate::games::launch::StartedGame {
        crate::games::launch::StartedGame {
            identity: osg_profiles::activation::GameIdentity {
                game_id: "x-plane-12".into(),
                executable: "C:\\Fixture\\X-Plane.exe".into(),
            },
            owner: self.run.owner.clone(),
            process: self.process.0.lock().unwrap().clone().unwrap(),
        }
    }
    fn stage(&self) -> Candidate {
        let mut c = self.game_candidate();
        c.stage = Some(
            NativeProfileHost::stage_launch_context(
                &self.host.resources,
                &self.host.games,
                &self.host.controllers,
                false,
                &c,
                self.started(),
            )
            .unwrap(),
        );
        c
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.host.set_authority(None);
        let _ = self.host.neutralize(&self.run);
        let _ = self.host.restore(&self.run, false);
        self.host.resources.shutdown();
        self.host.games.shutdown();
        self.host.companions.shutdown();
        self.host.controllers.shutdown();
    }
}

/// An older concurrent refresh must not retire a newer exact preparation.
/// Gate the real observer at process reads; do not retry preparation, sleep,
/// fabricate a proof or widen the production launch deadline.
#[test]
fn a_newer_setup_observation_preserves_its_preparation_when_an_old_refresh_completes_late() {
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    use std::time::Duration;

    struct GatedProcess {
        process: Process,
        observer: Mutex<Option<std::thread::ThreadId>>,
        captured: SyncSender<()>,
        release_old: Mutex<Receiver<()>>,
        published: SyncSender<()>,
        release_next: Mutex<Receiver<()>>,
    }
    impl Platform for GatedProcess {
        fn integration_setup_supported(&self) -> bool {
            self.process.integration_setup_supported()
        }
        fn catalogue(&self, manual: &BTreeMap<String, String>) -> Vec<serde_json::Value> {
            self.process.catalogue(manual)
        }
        fn validate(&self, id: &str, path: &str) -> Result<(), String> {
            self.process.validate(id, path)
        }
        fn launch(&self, game: &serde_json::Value) -> Result<ProcessIdentity, String> {
            self.process.launch(game)
        }
        fn find(&self, game: &serde_json::Value) -> Result<Option<ProcessIdentity>, String> {
            if game["id"] == "x-plane-12" && game.get("integrationSetup").is_some() {
                let current = std::thread::current().id();
                let mut observer = self.observer.lock().unwrap();
                if observer.is_none() && game["integrationSetup"]["state"] == "setup-required" {
                    *observer = Some(current);
                    drop(observer);
                    // A failed assertion can leave this notification unread.
                    // Never block observer shutdown on a full fixture channel.
                    self.captured
                        .try_send(())
                        .map_err(|error| error.to_string())?;
                    self.release_old
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3))
                        .map_err(|error| error.to_string())?;
                } else if *observer == Some(current) {
                    drop(observer);
                    // Reaching this observer's next scan proves its previous
                    // setup-required snapshot attempted publication. Hold this
                    // newer scan before publication so the assertion observes
                    // exactly that late completion, independent of scheduling.
                    self.published
                        .try_send(())
                        .map_err(|error| error.to_string())?;
                    self.release_next
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3))
                        .map_err(|error| error.to_string())?;
                    *self.observer.lock().unwrap() = None;
                }
            }
            self.process.find(game)
        }
        fn alive(&self, process: &ProcessIdentity) -> Result<bool, String> {
            self.process.alive(process)
        }
    }

    let directory = crate::test_directory();
    let (captured, old_captured) = sync_channel(1);
    let (release_old, old_release) = sync_channel(1);
    let (published, old_published) = sync_channel(1);
    let (release_next, next_release) = sync_channel(1);
    let platform = Arc::new(GatedProcess {
        process: Process(
            Mutex::new(Some(ProcessIdentity {
                pid: 42,
                path: "C:\\Fixture\\X-Plane.exe".into(),
                epoch: 10,
            })),
            std::sync::atomic::AtomicBool::new(true),
            std::sync::atomic::AtomicBool::new(false),
        ),
        observer: Mutex::new(None),
        captured,
        release_old: Mutex::new(old_release),
        published,
        release_next: Mutex::new(next_release),
    });
    let games = crate::games::GamesRuntime::test_runtime(
        Arc::new(|request| panic!("Read-only process proof must not invoke an output: {request}")),
        crate::games::Paths::in_directory(directory.path()),
        platform,
    );
    old_captured.recv_timeout(Duration::from_secs(3)).unwrap();
    let revision = games.resource_catalogue()[0]["integrationSetup"]["providerSetupRevision"]
        .as_str()
        .unwrap()
        .to_owned();
    games.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":crate::xplane::defaults(12)})).unwrap();
    let prepared = games
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .unwrap();
    games.validate_game_preparation(&prepared.proof).unwrap();
    release_old.send(()).unwrap();
    old_published.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        games.resource_catalogue()[0]["integrationContext"]["status"],
        "waiting-profile"
    );
    games.validate_game_preparation(&prepared.proof).unwrap();
    assert!(games.profile_game_context("x-plane-12").is_err());
    // Explicit reads retain the same preparation, but cannot grant a game
    // context without a matching applied owner. No launch or Apply occurs.
    let fresh = games
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .unwrap();
    assert_eq!(fresh.proof, prepared.proof);
    games.validate_game_preparation(&fresh.proof).unwrap();
    assert!(games.profile_game_context("x-plane-12").is_err());
    release_next.send(()).unwrap();
    games.shutdown();
}

#[test]
fn actual_empty_hardware_hold_has_no_reducer_or_optional_services_and_epoch_is_one_use() {
    let mut f = Fixture::new();
    assert!(f.host.supports_launch_hold());
    f.bind();
    for _ in 0..4 {
        assert!(f.host.hold_health(&f.run).unwrap().stop.is_none());
    }
    let state = f.host.run(&f.run.owner).unwrap();
    assert!(
        state.reducer.is_none() && state.baselines.is_empty() && state.child_game_owner.is_none()
    );
    assert!(f.host.authority.is_none());
    assert!(f.host.bind_launch_epoch(&f.run, 2).is_err());
    f.host.admission_epoch.store(3, Ordering::Release);
    assert!(f
        .host
        .hold_health(&f.run)
        .unwrap_err()
        .contains("cancelled"));
}
#[test]
fn real_native_process_proof_installs_child_then_final_start_keeps_hardware_owner_and_stop_revokes_it(
) {
    let mut f = Fixture::new();
    f.bind();
    let c = f.stage();
    assert!(f.host.games.profile_game_context("x-plane-12").is_err());
    let prepared = f.host.install_launch_context(&f.run, &c).unwrap();
    assert!(f.host.authority.is_none());
    assert!(f.host.run(&f.run.owner).unwrap().reducer.is_none());
    assert!(
        f.host.hold_health(&f.run).is_err(),
        "The game install ends the pre-context hold"
    );
    f.host.validate_prepared(&prepared, &f.run.owner).unwrap();
    f.host
        .enter(ApplyPhase::Baseline, &prepared, &f.run)
        .unwrap();
    f.host.set_authority(Some(&f.run.owner)).unwrap();
    let token = f.host.games.profile_game_context("x-plane-12").unwrap();
    assert_eq!(token.owner.session, f.run.owner.session);
    assert_eq!(token.owner.run_generation, f.run.owner.generation);
    assert!(f.run.owner.game_id.is_none());
    f.host.enter(ApplyPhase::Start, &prepared, &f.run).unwrap();
    assert!(f.host.run(&f.run.owner).unwrap().reducer.is_some());
    f.host.set_authority(None).unwrap();
    assert!(f.host.games.validate_profile_game_context(&token).is_err());
    f.host.set_authority(None).unwrap();
    assert!(f.host.games.profile_game_context("x-plane-12").is_err());
    assert!(f.host.install_launch_context(&f.run, &c).is_err());
}
#[test]
fn newly_opened_game_reader_can_wait_for_a_baseline_before_inventory_publication() {
    let mut f=Fixture::new();
    let (reader,_address,_catalogue)=crate::profile_session_games::test_baseline_reader();
    let resources=f.host.resources.clone();let games=f.host.games.clone();
    let state=f.host.run(&f.run.owner).unwrap();state.game=reader;
    let pending=NativeProfileHost::game_poll(&resources,&games,state,||1_000_000).unwrap();
    assert!(!pending.context_lost,"Waiting for advancing telemetry must not retire the fresh reader: {:?}",pending.diagnostics);
    assert!(!pending.fresh);
    assert!(!state.game.can_command());
    assert!(!state.game.has_delivery_publication());
}
#[test]
fn closing_and_restarting_game_replaces_context_without_stopping_the_hardware_run() {
    let mut f=Fixture::new();f.bind();
    let candidate=f.stage();
    let prepared=f.host.install_launch_context(&f.run,&candidate).unwrap();
    f.host.set_authority(Some(&f.run.owner)).unwrap();
    f.host.enter(ApplyPhase::Start,&prepared,&f.run).unwrap();
    let original=f.host.games.profile_game_context("x-plane-12").unwrap();
    let process=f.process.0.lock().unwrap().clone().unwrap();
    *f.process.0.lock().unwrap()=None;
    let resources=f.host.resources.clone();let games=f.host.games.clone();
    let state=f.host.run(&f.run.owner).unwrap();
    let closed=NativeProfileHost::game_poll(&resources,&games,state,||1_000_000).unwrap();
    assert!(closed.context_lost);
    assert!(state.reducer.is_some(),"Game loss must preserve the active hardware reducer");
    assert_eq!(state.owner,f.run.owner);
    let mut restarted=process;restarted.epoch+=1;
    *f.process.0.lock().unwrap()=Some(restarted.clone());
    let state=f.host.run(&f.run.owner).unwrap();
    let deadline=Instant::now()+Duration::from_secs(3);
    let returned=loop {
        let sample=NativeProfileHost::game_poll(&resources,&games,state,||2_000_000).unwrap();
        if !sample.context_lost{break sample;}
        assert!(Instant::now()<deadline,"{:?}",sample.diagnostics);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(!returned.context_lost,"{:?}",returned.diagnostics);
    assert!(state.reducer.is_some());
    assert_eq!(state.owner,f.run.owner);
    let current=games.profile_game_context("x-plane-12").unwrap();
    assert_eq!(current.process,restarted);
    assert_ne!(current.session,original.session);
    assert!(games.validate_profile_game_context(&original).is_err(),"Retired process authority remains revoked");
}
#[test]
fn replaced_process_late_candidate_or_changed_document_cannot_install_or_arm() {
    let mut f = Fixture::new();
    f.bind();
    let c = f.stage();
    f.process.0.lock().unwrap().as_mut().unwrap().epoch += 1;
    assert!(f.host.install_launch_context(&f.run, &c).is_err());
    assert!(f.host.run(&f.run.owner).unwrap().child_game_owner.is_none());
    let c = f.stage();
    let mut changed = c.clone();
    changed.document.name = "Changed while waiting".into();
    assert!(f.host.install_launch_context(&f.run, &changed).is_err());
    f.host.admission_epoch.store(3, Ordering::Release);
    assert!(f.host.install_launch_context(&f.run, &c).is_err());
    assert!(f.host.run(&f.run.owner).unwrap().reducer.is_none());
}
#[test]
fn launch_capture_rejects_missing_native_game_readonly_reused_or_uncancelled_candidates() {
    let f = Fixture::new();
    let base = f.game_candidate();
    for case in 0..4 {
        let mut c = base.clone();
        match case {
            0 => c.read_only = true,
            1 => c.admission_epoch = None,
            2 => c.stage = f.candidate.stage.clone(),
            _ => c.game_id = Some("unknown".into()),
        };
        assert!(NativeProfileHost::stage_launch_context(
            &f.host.resources,
            &f.host.games,
            &f.host.controllers,
            false,
            &c,
            f.started()
        )
        .is_err());
    }
    let started = f.started();
    *f.process.0.lock().unwrap() = None;
    assert!(NativeProfileHost::stage_launch_context(
        &f.host.resources,
        &f.host.games,
        &f.host.controllers,
        false,
        &base,
        started
    )
    .is_err());
}

#[test]
fn hardware_launch_completes_with_no_selected_context_or_no_optional_provider() {
    for select_context in [false, true] {
        let mut f = Fixture::new();
        f.bind();
        if select_context {
            f.process.1.store(false, Ordering::Release);
        }
        let mut candidate = f.game_candidate();
        if !select_context {
            candidate.game_id = None;
        }
        candidate.stage = Some(
            NativeProfileHost::stage_launch_context(
                &f.host.resources,
                &f.host.games,
                &f.host.controllers,
                false,
                &candidate,
                f.started(),
            )
            .unwrap(),
        );
        let prepared = f.host.install_launch_context(&f.run, &candidate).unwrap();
        assert_eq!(f.host.run(&f.run.owner).unwrap().child_game_owner.is_some(),select_context,"The selected context remains available for keyboard scope and later integration readiness");
        assert!(f.host.run(&f.run.owner).unwrap().game_preparation.is_none());
        f.host.validate_prepared(&prepared, &f.run.owner).unwrap();
        f.host
            .enter(ApplyPhase::Baseline, &prepared, &f.run)
            .unwrap();
        f.host.set_authority(Some(&f.run.owner)).unwrap();
        f.host.enter(ApplyPhase::Start, &prepared, &f.run).unwrap();
        assert!(f.host.run(&f.run.owner).unwrap().reducer.is_some());
        assert!(f.host.games.profile_game_context("x-plane-12").is_err());
    }
}
#[test]
fn desktop_launch_scope_uses_verified_started_process_independently_of_selected_telemetry(){
    use osg_desktop_output::Scope;
    let f=Fixture::new();let started=f.started();
    let other=Scope::Application{identity:crate::profile_session_desktop::identity(&secondary_process())};
    let expected=Scope::Application{identity:crate::profile_session_desktop::identity(&started.process)};
    assert_eq!(launched_desktop_scope(None,true,&started.process),Some(expected.clone()));
    assert_eq!(launched_desktop_scope(Some(&other),true,&started.process),Some(other));
    assert_eq!(launched_desktop_scope(Some(&Scope::DesktopWide),true,&started.process),Some(Scope::DesktopWide));
    assert_eq!(launched_desktop_scope(None,false,&started.process),None);
    // The actual held NativeRun remains unarmed while process identity is
    // selected. Scope selection creates no reducer, worker or output handles.
    let state=f.host.prepared.get(&key(&f.run.owner)).unwrap();assert!(state.reducer.is_none());assert!(state.desktop.is_none());
    assert_eq!(state.document.revision,f.run.snapshot.revision);
}

#[test]
fn verified_policy_context_may_be_another_running_game_without_authorizing_the_requested_spawn() {
    let mut f = Fixture::new();
    f.bind();
    f.process.2.store(true, Ordering::Release);
    let revision = f
        .host
        .games
        .resource_catalogue()
        .into_iter()
        .find(|g| g["id"] == "x-plane-11")
        .unwrap()["integrationSetup"]["providerSetupRevision"]
        .as_str()
        .unwrap()
        .to_owned();
    f.host.games.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-11","expectedSetupRevision":revision,"connection":crate::xplane::defaults(11)})).unwrap();
    let mut candidate = f.game_candidate();
    candidate.game_id = Some("x-plane-11".into());
    let started = f.started();
    assert_eq!(started.identity.game_id, "x-plane-12");
    candidate.stage = Some(
        NativeProfileHost::stage_launch_context(
            &f.host.resources,
            &f.host.games,
            &f.host.controllers,
            false,
            &candidate,
            started,
        )
        .unwrap(),
    );
    let prepared = f.host.install_launch_context(&f.run, &candidate).unwrap();
    assert_eq!(
        f.host
            .run(&f.run.owner)
            .unwrap()
            .game_preparation
            .as_ref()
            .unwrap()
            .proof
            .process,
        secondary_process()
    );
    f.host
        .enter(ApplyPhase::Baseline, &prepared, &f.run)
        .unwrap();
    f.host.set_authority(Some(&f.run.owner)).unwrap();
    f.host.enter(ApplyPhase::Start, &prepared, &f.run).unwrap();
    assert_eq!(
        f.host
            .games
            .profile_game_context("x-plane-11")
            .unwrap()
            .owner
            .game_id
            .as_deref(),
        Some("x-plane-11")
    );
    assert!(f.host.games.profile_game_context("x-plane-12").is_err());
}
#[test]
fn a_child_context_cannot_be_transferred_to_another_generation_or_wait_under_authority() {
    let mut f = Fixture::new();
    f.host.set_authority(Some(&f.run.owner)).unwrap();
    assert!(f.host.bind_launch_epoch(&f.run, 1).is_err());
    assert!(f.host.hold_health(&f.run).is_err());
    f.host.set_authority(None).unwrap();
    f.bind();
    let c = f.stage();
    f.host.install_launch_context(&f.run, &c).unwrap();
    f.host
        .run(&f.run.owner)
        .unwrap()
        .child_game_owner
        .as_mut()
        .unwrap()
        .run_generation += 1;
    assert!(f.host.set_authority(Some(&f.run.owner)).is_err());
    assert!(f.host.authority.is_none());
    let mut other = f.run.owner.clone();
    other.generation += 1;
    assert!(f
        .host
        .bind_launch_epoch(
            &OwnedRun {
                owner: other,
                ..f.run.clone()
            },
            2
        )
        .is_err());
}

fn registry_port() -> (Registry, ResourceToken, PortPreparation) {
    let mut registry = Registry::new();
    let port = PortDescriptor {
        id: "button:0".into(),
        name: "Button".into(),
        direction: osg_profiles::Direction::Output,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(PortValue::Boolean(false)),
        },
    };
    registry
        .reconcile(ProviderBatch {
            scope: "host-controllers".into(),
            session: "provider-1".into(),
            revision: 1,
            complete: true,
            observations: vec![Observation {
                key: "physical".into(),
                name: "Physical".into(),
                resource_type: ResourceType::HostController,
                evidence: vec![],
                attributes: BTreeMap::new(),
                diagnostics: vec![],
                route: Some(Route {
                    id: "hid:physical".into(),
                    transport: Transport::Hid,
                    provider_generation: "1".into(),
                    attributes: BTreeMap::new(),
                    baseline_ready: true,
                    writable: false,
                    priority: 1,
                }),
                ports: Some(PortUpdate::complete(vec![port.clone()]).unwrap()),
            }],
        })
        .unwrap();
    let id = registry.list(0, 1).unwrap().resources[0].id.clone();
    let token = registry.token(&id, &port.id, Access::Read).unwrap();
    let pin = registry
        .prepare_port(&id, &port.id, &port.contract_hash().unwrap(), Access::Read)
        .unwrap();
    (registry, token, pin)
}
#[test]
fn exact_acquired_hardware_pin_checks_provider_contract_route_and_stale_delivery() {
    let (mut registry, token, pin) = registry_port();
    let route = pin.resolved().route.clone();
    hardware_pin(Some(&token), Some(&route), Some(&pin), Some(&route), |t| {
        registry.validate_token(t, &t.port_id, t.access).map(|_| ())
    })
    .unwrap();
    for case in 0..6 {
        let mut t = token.clone();
        match case {
            0 => t.provider_session = "other".into(),
            1 => t.provider_generation = "2".into(),
            2 => t.contract_hash = "changed".into(),
            3 => t.inventory_session = "other".into(),
            4 => t.resource_id = "alias".into(),
            _ => t.capability_revision = "changed".into(),
        };
        assert!(
            hardware_pin(Some(&t), Some(&route), Some(&pin), Some(&route), |_| Ok(())).is_err()
        );
    }
    let mut other = route.clone();
    other.transport = Transport::Usb;
    assert!(hardware_pin(
        Some(&token),
        Some(&route),
        Some(&pin),
        Some(&other),
        |_| Ok(())
    )
    .is_err());
    assert!(hardware_pin(None, Some(&route), Some(&pin), Some(&route), |_| Ok(())).is_err());
    assert!(hardware_pin(Some(&token), Some(&route), None, Some(&route), |_| Ok(())).is_err());
    hardware_pin(None, None, None, None, |_| panic!("No delivery token")).unwrap();
    registry.provider_failed("host-controllers", "Removed");
    assert!(
        hardware_pin(Some(&token), Some(&route), Some(&pin), Some(&route), |t| {
            registry.validate_token(t, &t.port_id, t.access).map(|_| ())
        })
        .is_err()
    );
}
#[test]
fn compile_admission_preserves_structural_resource_and_optional_dormant_diagnostics() {
    let f = Fixture::new();
    let snapshot = CompileSnapshot {
        inventory_session: "fixture".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let report = osg_profiles::compiler::compile(&f.candidate.document, &snapshot);
    assert!(prepared_report(&report)
        .unwrap()
        .admission
        .blockers
        .is_empty());
    let mut invalid = report.clone();
    invalid.readiness = PlanReadiness::Invalid;
    invalid.diagnostics.push(CompileDiagnostic {
        code: "structural".into(),
        severity: DiagnosticSeverity::Error,
        message: "Invalid graph".into(),
        location: GraphLocation::default(),
    });
    assert_eq!(
        prepared_report(&invalid).unwrap().admission.blockers[0].kind,
        BlockerKind::Structural
    );
    invalid.readiness = PlanReadiness::Blocked;
    assert_eq!(
        prepared_report(&invalid).unwrap().admission.blockers[0].kind,
        BlockerKind::RequiredResource
    );
    invalid.diagnostics.clear();
    assert!(prepared_report(&invalid).is_err());
    invalid.plan = None;
    assert!(prepared_report(&invalid).is_err());
    let mut missing_hash = report;
    missing_hash.program_hash = None;
    assert!(prepared_report(&missing_hash).is_err());
}

#[test]
fn hardware_plan_comparison_checks_sources_destinations_and_program_without_allowing_alias_transfer(
) {
    let f = Fixture::new();
    let (_, token, pin) = registry_port();
    let mut before = f.host.prepared[&key(&f.run.owner)]
        .delivery
        .clone()
        .unwrap();
    let mut document = f.candidate.document.clone();
    document.resources.push(osg_profiles::Resource {
        id: "physical".into(),
        name: "Physical".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: vec![],
    });
    let endpoint = osg_profiles::Endpoint {
        node_id: "physical".into(),
        port_id: "button".into(),
    };
    before.sources.push(SourcePlan {
        endpoint: endpoint.clone(),
        inventory_resource_id: Some(token.resource_id.clone()),
        observed_port_id: Some(token.port_id.clone()),
        contract: pin.resolved().port.contract.clone(),
        token: Some(token.clone()),
        preparation: None,
        route: Some(pin.resolved().route.clone()),
        readiness: PlanReadiness::Ready,
    });
    before.sinks.push(SinkPlan {
        endpoint: osg_profiles::Endpoint {
            node_id: "virtual".into(),
            port_id: "button".into(),
        },
        adapter: SinkAdapter::VirtualController,
        inventory_resource_id: None,
        observed_port_id: None,
        contract: pin.resolved().port.contract.clone(),
        token: None,
        preparation: None,
        route: None,
        readiness: PlanReadiness::Ready,
    });
    let mut after = before.clone();
    after.sources[0].token = None;
    after.sources[0].preparation = Some(pin);
    unchanged_hardware(&document, &before, &after, |_| Ok(())).unwrap();
    for case in 0..6 {
        let mut changed = after.clone();
        match case {
            0 => changed.sources.clear(),
            1 => changed.sources[0].inventory_resource_id = Some("alias".into()),
            2 => changed.sinks.clear(),
            3 => changed.sinks[0].adapter = SinkAdapter::PhysicalState,
            4 => changed.sources[0].readiness = PlanReadiness::Dormant,
            _ => changed
                .program
                .sources
                .push(osg_mapping::program::SourcePort {
                    endpoint: osg_mapping::program::Endpoint {
                        node_id: "extra".into(),
                        port_id: "extra".into(),
                    },
                    contract: before.sources[0].contract.clone(),
                }),
        };
        assert!(
            unchanged_hardware(&document, &before, &changed, |_| Ok(())).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn actual_companion_hold_compares_current_acquired_publication_not_its_old_prospective_pin() {
    let fixture = super::super::companion::tests::NativeFixture::new();
    let mut host = fixture.host();
    let candidate = fixture.candidate(false);
    let owner = RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: candidate.document.id.clone(),
        profile_revision: candidate.document.revision,
        game_id: None,
    };
    let prepared = host.prepare(&candidate, &owner).unwrap();
    let receipts = host.capture(&prepared, &owner, None).unwrap();
    let mut run = OwnedRun {
        owner,
        snapshot: osg_profiles::session::AppliedSnapshot::capture(&candidate.document)
            .unwrap()
            .summary()
            .clone(),
        program_hash: prepared.program_hash.clone(),
        capture_fingerprint: prepared.capture_fingerprint.clone(),
        receipts,
        attempted: vec![],
    };
    for phase in [
        ApplyPhase::Overrides,
        ApplyPhase::Acquire,
        ApplyPhase::Baseline,
        ApplyPhase::Visibility,
    ] {
        run.attempted.push(phase);
        host.enter(phase, &prepared, &run).unwrap();
    }
    host.admission_epoch.store(2, Ordering::Release);
    host.bind_launch_epoch(&run, 2).unwrap();
    host.hold_health(&run).unwrap();
    let (mut snapshot, pins) = host
        .resources
        .snapshot_for_preparation(candidate.assignments.clone())
        .unwrap();
    snapshot.backend = host.prepared[&key(&run.owner)].backend.clone();
    let next =
        osg_profiles::compiler::compile_for_preparation(&candidate.document, &snapshot, &pins)
            .plan
            .unwrap();
    assert!(prepared.plan.sources[0]
        .preparation
        .as_ref()
        .unwrap()
        .companion_publication()
        .is_some());
    assert!(next.sources[0]
        .preparation
        .as_ref()
        .unwrap()
        .companion_publication()
        .is_none());
    unchanged_hardware(
        &candidate.document,
        host.prepared[&key(&run.owner)].delivery.as_ref().unwrap(),
        &next,
        |token| NativeProfileHost::validate_token(&host.resources, token),
    )
    .unwrap();
    host.neutralize(&run).unwrap();
    host.restore(&run, false).unwrap();
}

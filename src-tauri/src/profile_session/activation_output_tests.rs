use super::*;
fn activation_host_fixture() -> (
    tempfile::TempDir,
    NativeProfileHost,
    RunOwner,
    Arc<Mutex<DriverState>>,
    Arc<AtomicU64>,
) {
    use crate::game_discovery::{Platform, ProcessIdentity};
    struct NoGame;
    impl Platform for NoGame {
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<serde_json::Value> {
            vec![]
        }
        fn validate(&self, _: &str, _: &str) -> Result<(), String> {
            Err("No real game".into())
        }
        fn launch(&self, _: &serde_json::Value) -> Result<ProcessIdentity, String> {
            panic!("No launch")
        }
        fn find(&self, _: &serde_json::Value) -> Result<Option<ProcessIdentity>, String> {
            Ok(None)
        }
        fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
            Ok(false)
        }
    }

    let directory = crate::test_directory();
    let resources = Arc::new(crate::profile_resources::Runtime::new(
        None,
        Arc::new(|_| vec![]),
        false,
    ));
    resources.refresh_native().unwrap();
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let games = Arc::new(crate::games::GamesRuntime::test_runtime(
        Arc::new(|_| panic!("No physical I/O")),
        crate::games::Paths {
            installations: directory.path().join("installations.json"),
            provider_setup: directory.path().join("setup.json"),
            recovery: directory.path().join("recovery.json"),
        },
        Arc::new(NoGame),
    ));
    let companions = Arc::new(crate::companion_host::Runtime::new(
        None,
        Arc::new(osg_mapping_host::companions::CompanionProvider::new(Arc::new(|| 0))),
    ));
    let epoch = Arc::new(AtomicU64::new(7));
    let mut host = NativeProfileHost::new(
        resources.clone(),
        controllers.clone(),
        games.clone(),
        companions.clone(),
        Arc::new(crate::gateways::ProfilePanelIo::empty()),
        false,
        epoch.clone(),
    );
    let mut candidate = candidate(1);
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
    let owner = owner(&candidate);
    host.prepare(&candidate, &owner).unwrap();

    let (factory, driver) = factory_with_renewal_clock(true);
    let binding = output::bindings(
        &[controller("combined", 0)],
        &[driver.lock().unwrap().slot.clone()],
    )
    .unwrap()
    .remove(0);
    let session = start(&factory, directory.path().join("activation-output.json"), &owner, binding.clone());
    let state = host.run(&owner).unwrap();
    state.output = Some(session);
    state.bindings = vec![binding];
    (directory, host, owner, driver, epoch)
}

fn renewal_count(driver: &Arc<Mutex<DriverState>>) -> usize {
    driver
        .lock()
        .unwrap()
        .messages
        .iter()
        .filter(|(_, message)| matches!(message, Message::Renew { .. }))
        .count()
}

#[test]
fn activation_without_output_preserves_admission_and_deadline_guards() {
    let (_directory, mut host, owner, _, epoch) = activation_host_fixture();
    host.run(&owner).unwrap().output.take().unwrap().stop().unwrap();

    let result = host.with_prepared_heartbeat(&owner, Duration::ZERO, true, |_| {
        panic!("Expired activation must never enter synchronous work")
    });
    let result: Result<(), String> = result;
    assert!(result.unwrap_err().contains("exceeded its deadline"));

    let result = host.with_activation_heartbeat(&owner, |_| {
        epoch.fetch_add(1, Ordering::AcqRel);
        Ok(())
    });
    assert!(result.unwrap_err().contains("cancelled by a newer native control"));
}

#[test]
fn activation_keeps_real_output_lease_alive_while_synchronous_work_runs() {
    let (_directory, mut host, owner, driver, _) = activation_host_fixture();
    let minimum_renewals = (LEASE_US / RENEW_US + 1) as usize;
    let started = Instant::now();
    host.with_activation_heartbeat(&owner, |_host| {
        while started.elapsed() < Duration::from_millis(300)
            || renewal_count(&driver) < minimum_renewals
        {
            if started.elapsed() > Duration::from_secs(2) {
                return Err("Actual output feeder did not renew during activation".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    })
    .unwrap();

    let state = host.run(&owner).unwrap();
    state.output.as_mut().unwrap().keep_alive().unwrap();
    assert!(state.output.as_mut().unwrap().handoff_snapshot().unwrap().is_some());
    assert!(state.reducer.is_none(), "Activation work must not arm the reducer");
    assert!(!driver
        .lock()
        .unwrap()
        .messages
        .iter()
        .any(|(_, message)| matches!(message, Message::Report { .. })));
    assert_eq!(driver.lock().unwrap().slot.status, SlotStatus::Busy);
    output::neutralize_output(state.output.as_mut().unwrap(), &state.recording).unwrap();
    assert_eq!(driver.lock().unwrap().slot.status, SlotStatus::Free);
}

#[test]
fn activation_epoch_cancellation_stops_output_renewals() {
    let (_directory, mut host, owner, driver, epoch) = activation_host_fixture();
    let before = renewal_count(&driver);
    let mut saw_renewal = false;
    let result = host.with_activation_heartbeat(&owner, |_host| {
        let deadline = Instant::now() + Duration::from_secs(2);
        while renewal_count(&driver) == before {
            if Instant::now() >= deadline {
                return Err("Actual output feeder did not renew during activation".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        saw_renewal = true;
        epoch.fetch_add(1, Ordering::AcqRel);
        Ok(())
    });
    assert!(saw_renewal, "Cancellation should follow an observed renewal");
    assert!(result.unwrap_err().to_lowercase().contains("cancel"));
    let stopped_count = renewal_count(&driver);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(renewal_count(&driver), stopped_count, "Renewals continued after cancellation returned");

    let state = host.run(&owner).unwrap();
    assert!(state.reducer.is_none());
    assert!(!driver
        .lock()
        .unwrap()
        .messages
        .iter()
        .any(|(_, message)| matches!(message, Message::Report { .. })));
    state.output.as_mut().unwrap().stop().unwrap();
}

#[test]
fn phase_storage_keeps_real_neutral_output_leased_until_the_write_returns() {
    let (_directory, mut host, owner, driver, _) = activation_host_fixture();
    let minimum_renewals = (LEASE_US / RENEW_US + 1) as usize;
    host.activation_storage(Some(&owner), || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(300)
            || renewal_count(&driver) < minimum_renewals
        {
            if started.elapsed() > Duration::from_secs(2) {
                return Err("Phase journal write starved the actual output feeder".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }).unwrap();
    assert!(renewal_count(&driver) >= minimum_renewals);
    let completed_count = renewal_count(&driver);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(renewal_count(&driver), completed_count, "Storage renewals survived the write");
    let state = host.run(&owner).unwrap();
    assert!(state.reducer.is_none());
    assert_eq!(driver.lock().unwrap().slot.status, SlotStatus::Busy);
    assert!(!driver.lock().unwrap().messages.iter()
        .any(|(_, message)| matches!(message, Message::Report { .. })));
    state.output.as_mut().unwrap().stop().unwrap();
}

#[test]
fn activation_guard_rejects_armed_reducers_and_storage_uses_the_normal_path() {
    let (_directory, mut host, owner, driver, _) = activation_host_fixture();
    let state = host.run(&owner).unwrap();
    let program = state.report.plan.as_ref().unwrap().program.clone();
    state.reducer = Some(Phase::new_at(program, Counter(owner.generation), owner.session.clone(), Counter(1)).unwrap());
    let before = renewal_count(&driver);
    let mut prepared_work_ran = false;
    let error = host.with_activation_heartbeat(&owner, |_| {
        prepared_work_ran = true;
        Ok(())
    }).unwrap_err();
    assert!(error.contains("Only prepared neutral outputs"));
    assert!(!prepared_work_ran);
    let mut stored = false;
    host.activation_storage(Some(&owner), || {
        stored = true;
        Ok(())
    }).unwrap();
    assert!(stored, "An armed owner must still persist through its ordinary storage path");
    assert_eq!(renewal_count(&driver), before, "An armed owner reused the neutral activation pump");
    host.run(&owner).unwrap().output.as_mut().unwrap().stop().unwrap();
}

#[test]
fn cleanup_storage_still_writes_after_output_stop_and_epoch_cancellation() {
    let (_directory, mut host, owner, driver, epoch) = activation_host_fixture();
    host.run(&owner).unwrap().output.as_mut().unwrap().stop().unwrap();
    epoch.fetch_add(1, Ordering::AcqRel);
    let before = renewal_count(&driver);
    let mut stored = false;
    host.activation_storage(Some(&owner), || {
        stored = true;
        Ok(())
    }).unwrap();
    assert!(stored, "Cancelled activation must still record completed compensation");
    assert_eq!(renewal_count(&driver), before);
}

#[test]
fn stalled_simulator_command_and_panel_cache_do_not_hold_host_output_or_stop() {
    use osg_mapping::program::{Endpoint as MappingEndpoint, Program, Sink, SinkKind, SourcePort};
    use osg_mapping::profile::OutputControl;
    use osg_profiles::{compiler_models::SourcePlan, Endpoint, Port, PortContract, Resource, ResourceKind};
    use std::sync::mpsc;

    let (_directory, mut host, owner, driver, _) = activation_host_fixture();
    let mut candidate = candidate(1);
    candidate.document.id = owner.profile_id.clone();
    let run = run(&candidate, owner.clone());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    host.set_authority(Some(&owner)).unwrap();
    host.run(&owner).unwrap().game.submit_test_command(move |cancelled| {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        done_tx.send(cancelled.load(Ordering::Acquire)).unwrap();
        Ok(())
    }).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let panels = host.panels.clone();
    let (panel_held_tx, panel_held_rx) = mpsc::channel();
    let (release_panel, panel_release_rx) = mpsc::channel();
    let panel_holder = std::thread::spawn(move || panels.with_sessions_held_for_test(|| {
        panel_held_tx.send(()).unwrap();
        let _ = panel_release_rx.recv_timeout(Duration::from_secs(10));
    }));
    panel_held_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let before = renewal_count(&driver);
    host.with_activation_heartbeat(&owner, |_| {
        let deadline = Instant::now() + Duration::from_secs(2);
        while renewal_count(&driver) == before {
            if Instant::now() >= deadline { return Err("Virtual output did not renew".into()); }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }).unwrap();

    let controllers = host.controllers.clone();
    let now = controllers.monotonic_us();
    let state = host.run(&owner).unwrap();
    let contract = PortContract::State {
        schema: osg_mapping::signal::ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: Some(osg_mapping::signal::PortValue::Boolean(false)),
    };
    state.document.resources.push(Resource {
        id: "debug".into(),
        name: "Debug controller".into(),
        kind: ResourceKind::DebugController,
        required: false,
        ports: vec![Port { id: "button-1".into(), name: "Button".into(),
            direction: osg_profiles::Direction::Output, contract: contract.clone() }],
    });
    let mut plan = state.report.plan.clone().unwrap();
    plan.sources.push(SourcePlan {
        endpoint: Endpoint { node_id: "debug".into(), port_id: "button-1".into() },
        inventory_resource_id: None,
        observed_port_id: None,
        contract: contract.clone(),
        token: None,
        preparation: None,
        route: None,
        readiness: PlanReadiness::Ready,
    });
    let source = MappingEndpoint { node_id: "debug".into(), port_id: "button-1".into() };
    let program = Program {
        sources: vec![SourcePort { endpoint: source.clone(), contract: contract.clone() }],
        nodes: vec![],
        sinks: vec![Sink {
            id: "button".into(),
            endpoint: MappingEndpoint { node_id: "combined".into(), port_id: "button-0".into() },
            from: source,
            contract,
            kind: SinkKind::Virtual {
                controller_id: "combined".into(), control: OutputControl::Button { index: 0 },
            },
        }],
        controllers: vec![controller("combined", 0)],
    };
    let (debug, baseline) = crate::profile_debug_controller::DebugInputs::capture(
        &state.document, &plan, owner.generation, now,
    ).unwrap();
    state.debug_inputs = debug;
    let mut reducer = Phase::new_at(program, Counter(owner.generation), owner.session.clone(), Counter(now)).unwrap();
    reducer.accept_batch(baseline, Counter(now)).unwrap();
    state.recording.record(reducer.drain_reports());
    state.reducer = Some(reducer);
    state.inputs = Some(crate::profile_session_inputs::HidInputs::capture(&plan, controllers.as_ref(), None, |_| Ok(())).unwrap().0);
    state.panels.acquire(now).unwrap();
    state.panels.baseline(&plan, |_| Ok(())).unwrap().unwrap();
    state.panels.start().unwrap();
    let (guarded, capture, _) = dcs_hid::tests::fixture();
    state.dcs_hid = dcs_hid::Guard::capture(&guarded, &capture);
    plan.branches.extend(guarded.branches);
    plan.sinks.extend(guarded.sinks);
    state.delivery = Some(plan);

    let request = crate::profile_debug_controller::Request::parse(&json!({
        "command":"profile_debug_controller_input", "owner": owner, "nodeId":"debug",
        "streamId":uuid::Uuid::new_v4().to_string(), "sequence":"1",
        "changes":[{"portId":"button-1","value":true}]
    })).unwrap();
    host.debug_controller(&run, &request).unwrap();
    let health = host.health(&run).unwrap();
    assert!(health.stop.is_none());
    assert!(health.degraded.iter().any(|message| message.contains("waiting for current HID publication proof")));
    assert!(driver.lock().unwrap().messages.iter().any(|(_, message)|
        matches!(message, Message::Report { report, .. } if report.state.buttons[0])));
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));

    host.set_authority(None).unwrap();
    host.neutralize(&run).unwrap();
    assert_eq!(driver.lock().unwrap().slot.status, SlotStatus::Free);
    host.restore(&run, false).unwrap();
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(!panel_holder.is_finished(), "Stop waited for the locked panel cache");
    release_panel.send(()).unwrap();
    panel_holder.join().unwrap();
    release_tx.send(()).unwrap();
    assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
}

#[test]
fn neutralization_releases_vjoy_before_game_cleanup_progress_can_block() {
    use std::sync::mpsc;

    let (_directory, mut host, owner, driver, _) = activation_host_fixture();
    let mut candidate = candidate(1);
    candidate.document.id = owner.profile_id.clone();
    let run = run(&candidate, owner.clone());
    let state = host.run(&owner).unwrap();
    state.recording = held(&owner, &state.bindings[0]);
    state.output.as_mut().unwrap().poll(&state.recording).unwrap();
    let (pid, slot) = {
        let state = driver.lock().unwrap();
        let pid = state.slot.owner_pid.unwrap();
        assert_eq!(state.calls.last(), Some(&format!("submit:{pid}:1:23456")));
        (pid, state.slot.id)
    };
    state.game_preparation = Some(crate::games::PreparedGame {
        proof: crate::game_context::GamePreparation {
            session: uuid::Uuid::new_v4().to_string(),
            game_id: "dcs".into(),
            process: crate::game_discovery::ProcessIdentity {
                pid: 42,
                path: "fixture.exe".into(),
                epoch: 1,
            },
            provider_setup_revision: "fixture-setup".into(),
            catalogue_revision: "fixture-catalogue".into(),
            vehicle_id: None,
        },
        integration_key: "dcs".into(),
        connection: json!({}),
        catalogue: json!({}),
        aircraft_module: None,
    });
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let host = host.with_progress(Arc::new(move |_, step, _| {
        if step == "cleanup-disconnecting-game" {
            entered_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
        }
    }));
    let cleanup = std::thread::spawn(move || {
        let mut host = host;
        host.neutralize(&run)
    });

    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("game cleanup progress callback did not block");
    let (status, stop_sent, fallback_released) = {
        let state = driver.lock().unwrap();
        (state.slot.status.clone(),
            state.messages.iter().any(|(_, message)| matches!(message, Message::Stop { .. })),
            state.calls.ends_with(&[format!("submit:{pid}:0:0"), format!("release:{pid}:{slot}")]))
    };
    release_tx.send(()).unwrap();
    cleanup.join().unwrap().unwrap();
    assert_eq!(status, SlotStatus::Free, "Game cleanup began before vJoy released the slot");
    assert!(stop_sent && fallback_released, "vJoy must receive Stop and apply fallback before game cleanup");
}

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

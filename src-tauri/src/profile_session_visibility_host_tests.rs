use super::*;
use crate::profile_session_visibility::tests::{owner, plan, snapshot, Fake};

fn visibility(io: Arc<Fake>) -> Visibility {
    let snapshot = snapshot();
    Visibility::stage(&plan(&snapshot), &snapshot, io)
        .unwrap()
        .bind(&owner())
        .unwrap()
}

#[test]
fn actual_journal_start_and_async_restore_renew_without_restarting_worker() {
    let io = Fake::new();
    let before = io.driver.lock().unwrap().clone();
    let mut visibility = visibility(io.clone());
    wait(
        &mut visibility,
        false,
        Duration::from_secs(1),
        || Ok(()),
        || Ok(()),
    )
    .unwrap();
    assert!(io.driver.lock().unwrap().active);
    active(&mut visibility).unwrap();
    *io.asynchronous.lock().unwrap() = true;
    let mut renewals = 0;
    wait(
        &mut visibility,
        true,
        Duration::from_secs(1),
        || Ok(()),
        || {
            renewals += 1;
            *io.asynchronous.lock().unwrap() = false;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(renewals, 1);
    assert_eq!(*io.driver.lock().unwrap(), before);
    assert!(!visibility.needs_cleanup());
    assert_eq!(
        io.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(op, _)| op == "start")
            .count(),
        1
    );
}

#[test]
fn cancelled_pending_application_is_never_admitted_and_remains_owned_for_cleanup() {
    let io = Fake::new();
    let before = io.driver.lock().unwrap().clone();
    *io.start_pending.lock().unwrap() = true;
    let mut visibility = visibility(io.clone());
    let mut checks = 0;
    let error = wait(
        &mut visibility,
        false,
        Duration::from_secs(1),
        || {
            checks += 1;
            if checks == 1 {
                Ok(())
            } else {
                Err("Stop changed admission epoch".into())
            }
        },
        || Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("admission epoch"));
    assert!(visibility.needs_cleanup());
    assert!(io.driver.lock().unwrap().active);
    wait(
        &mut visibility,
        true,
        Duration::from_secs(1),
        || Ok(()),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(*io.driver.lock().unwrap(), before);
}

#[test]
fn deadline_keeps_same_restore_worker_and_second_attempt_finishes_it() {
    let io = Fake::new();
    let before = io.driver.lock().unwrap().clone();
    let mut visibility = visibility(io.clone());
    visibility.start().unwrap();
    *io.asynchronous.lock().unwrap() = true;
    assert!(
        wait(&mut visibility, true, Duration::ZERO, || Ok(()), || Ok(()))
            .unwrap_err()
            .contains("explicit recovery")
    );
    assert!(visibility.needs_cleanup());
    let first = io.calls.lock().unwrap().len();
    *io.asynchronous.lock().unwrap() = false;
    wait(
        &mut visibility,
        true,
        Duration::from_secs(1),
        || Ok(()),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(io.calls.lock().unwrap()[first].0, "poll");
    assert_eq!(*io.driver.lock().unwrap(), before);
}

#[test]
fn failed_renewal_or_foreign_owner_preserves_cleanup_authority() {
    let io = Fake::new();
    *io.start_pending.lock().unwrap() = true;
    let mut visibility = visibility(io.clone());
    assert!(wait(
        &mut visibility,
        false,
        Duration::from_secs(1),
        || Ok(()),
        || { Err("Output renewal failed".into()) }
    )
    .unwrap_err()
    .contains("renewal failed"));
    assert!(visibility.needs_cleanup());
    let actual = io.driver.lock().unwrap().clone();
    let old = io
        .owned
        .lock()
        .unwrap()
        .replace(uuid::Uuid::new_v4().to_string());
    assert!(active(&mut visibility).is_err());
    assert!(wait(
        &mut visibility,
        true,
        Duration::from_secs(1),
        || Ok(()),
        || Ok(())
    )
    .is_err());
    assert_eq!(*io.driver.lock().unwrap(), actual);
    *io.owned.lock().unwrap() = old;
    wait(
        &mut visibility,
        true,
        Duration::from_secs(1),
        || Ok(()),
        || Ok(()),
    )
    .unwrap();
}

#[test]
fn visible_empty_plan_skips_native_dependencies_guards_and_renewals() {
    let io = Fake::new();
    let snapshot = snapshot();
    let mut plan = plan(&snapshot);
    plan.visibility.clear();
    let mut visibility = Visibility::stage(&plan, &snapshot, io.clone())
        .unwrap()
        .bind(&owner())
        .unwrap();
    for restoring in [false, true] {
        wait(
            &mut visibility,
            restoring,
            Duration::ZERO,
            || panic!("Empty plan guard"),
            || panic!("Empty plan renewal"),
        )
        .unwrap();
    }
    active(&mut visibility).unwrap();
    assert!(io.calls.lock().unwrap().is_empty());
}

#[test]
fn actual_host_retains_exact_cleanup_after_conflict_and_releases_it_after_restore() {
    use crate::profile_session_visibility::tests::owned_run;
    let directory = tempfile::tempdir().unwrap();
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let games = Arc::new(crate::games::GamesRuntime::new(
        Arc::new(|_| json!({"ok":false})),
        directory.path().join("games.json"),
        Arc::new(crate::game_discovery::NativePlatform),
    ));
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
        resources,
        controllers,
        games,
        companions,
        Arc::new(crate::gateways::ProfilePanelIo::empty()),
        false,
        Arc::new(AtomicU64::new(1)),
    );
    let io = Fake::new();
    let before = io.driver.lock().unwrap().clone();
    let owner = owner();
    let snapshot = snapshot();
    let mut visibility = Visibility::stage(&plan(&snapshot), &snapshot, io.clone())
        .unwrap()
        .bind(&owner)
        .unwrap();
    let mut run = owned_run(owner.clone(), visibility.receipts().unwrap());
    visibility.start().unwrap();
    host.recovery_visibility.insert(key(&owner), visibility);
    let own = io
        .owned
        .lock()
        .unwrap()
        .replace(uuid::Uuid::new_v4().to_string());
    let current = io.driver.lock().unwrap().clone();
    assert!(host.restore_visibility(&run).is_err());
    assert!(host.recovery_visibility.contains_key(&key(&owner)));
    assert_eq!(*io.driver.lock().unwrap(), current);
    *io.owned.lock().unwrap() = own;
    host.restore_visibility(&run).unwrap();
    assert!(host.recovery_visibility.is_empty());
    assert_eq!(*io.driver.lock().unwrap(), before);
    run.receipts.clear();
    host.restore_visibility(&run).unwrap();
    run.attempted.clear();
    host.restore_visibility(&run).unwrap();
}

#[test]
fn actual_host_final_admission_validates_applied_hide_owner_before_any_neutralization() {
    use crate::profile_session_visibility::tests::owned_run;
    let directory = tempfile::tempdir().unwrap();
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let resources = Arc::new(crate::profile_resources::Runtime::new(
        None,
        Arc::new(|_| vec![]),
        false,
    ));
    let games = Arc::new(crate::games::GamesRuntime::new(
        Arc::new(|_| json!({"ok":false})),
        directory.path().join("games.json"),
        Arc::new(crate::game_discovery::NativePlatform),
    ));
    let companions = Arc::new(crate::companion_host::Runtime::new(
        None,
        Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(|| 0),
        )),
    ));
    let mut host = NativeProfileHost::new(
        resources.clone(),
        controllers.clone(),
        games.clone(),
        companions.clone(),
        Arc::new(crate::gateways::ProfilePanelIo::empty()),
        false,
        Arc::new(AtomicU64::new(1)),
    );
    let io = Fake::new();
    let snapshot = snapshot();
    let policy_plan = plan(&snapshot);
    let old_owner = owner();
    let before = io.driver.lock().unwrap().clone();
    let mut old = Visibility::stage(&policy_plan, &snapshot, io.clone())
        .unwrap()
        .bind(&old_owner)
        .unwrap();
    let mut outgoing = owned_run(old_owner.clone(), old.receipts().unwrap());
    outgoing.attempted.push(ApplyPhase::Start);
    old.start().unwrap();
    let incoming_owner = owner();
    let mut candidate = Candidate {
        document: osg_profiles::default_document(),
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: Some(1),
        stage: None,
    };
    candidate.stage = Some(
        NativeProfileHost::stage_with_handoff(
            &resources,
            &games,
            &companions,
            controllers,
            false,
            &candidate,
            Some(outgoing.clone()),
        )
        .unwrap(),
    );
    let prepared = host.prepare(&candidate, &incoming_owner).unwrap();
    // Only driver metadata is substituted. Capture/neutralize/Visibility/restore
    // remain the real Host operations; native qualification remains unchanged.
    host.run(&incoming_owner).unwrap().visibility =
        Visibility::stage_with_handoff(&policy_plan, &snapshot, io.clone(), Some(&outgoing))
            .unwrap()
            .bind(&incoming_owner)
            .unwrap();
    let active = io.driver.lock().unwrap().clone();
    assert!(host.capture(&prepared, &incoming_owner, None).is_err());
    let mut changed = outgoing.clone();
    changed.owner.generation += 1;
    assert!(host
        .capture(&prepared, &incoming_owner, Some(&changed))
        .is_err());
    assert_eq!(*io.driver.lock().unwrap(), active);
    let receipts = host
        .capture(&prepared, &incoming_owner, Some(&outgoing))
        .unwrap();
    assert_eq!(receipts.len(), 1);
    host.recovery_visibility.insert(key(&old_owner), old);
    host.neutralize(&outgoing).unwrap();
    assert_eq!(*io.driver.lock().unwrap(), before);
    let incoming = owned_run(incoming_owner, receipts);
    host.enter(ApplyPhase::Visibility, &prepared, &incoming)
        .unwrap();
    assert_ne!(*io.driver.lock().unwrap(), before);
    host.neutralize(&incoming).unwrap();
    host.restore(&incoming, false).unwrap();
    assert_eq!(*io.driver.lock().unwrap(), before);
    resources.shutdown();
    games.shutdown();
}

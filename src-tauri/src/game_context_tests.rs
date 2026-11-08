use super::*;

fn owner(game: Option<&str>) -> ProfileOwner {
    ProfileOwner {
        session: "96d54c8b-a2a4-433c-be95-eb0b91ee22d3".into(),
        profile_id: "e37f3e1f-ae05-4f26-a506-040be2849309".into(),
        applied_revision: 1,
        run_generation: 1,
        game_id: game.map(str::to_owned),
    }
}
fn observed() -> Observation {
    Observation {
        installation: json!({"executable":"/games/DCS.exe"}),
        process: Some(ProcessIdentity {
            pid: 42,
            path: "/games/DCS.exe".into(),
            epoch: 10,
        }),
        supported: true,
        setup_ready: true,
        provider_setup_revision: format!("sha256:{}", "a".repeat(64)),
        catalogue_revision: Some(format!("sha256:{}", "b".repeat(64))),
        vehicle_id: None,
        error: None,
    }
}
fn observe(broker: &mut Broker, id: &str, observation: Observation) -> Result<(), String> {
    let epoch = broker.begin_observation()?;
    broker.observe(id, epoch, observation, false)
}

#[test]
fn committed_preparation_changes_wake_reacquisition() {
    let mut broker = Broker::default();
    let signal = std::sync::Arc::new(crate::game_read_models::signals::Signal::default());
    let routing = std::sync::Arc::new(osg_streams::NativeWake::default());
    signal.watch_vehicle_routing(&routing);
    broker.watch_changes(signal.clone());
    let mut waiting = observed();
    waiting.setup_ready = false;
    observe(&mut broker, "dcs", waiting.clone()).unwrap();
    let waiting_revision = signal.game_reacquisition_revision();
    let waiting_wake = routing.observed();
    observe(&mut broker, "dcs", waiting).unwrap();
    assert_eq!(signal.game_reacquisition_revision(), waiting_revision);
    assert_eq!(routing.observed(), waiting_wake);
    observe(&mut broker, "dcs", observed()).unwrap();
    assert!(broker.preparation("dcs").unwrap().is_some());
    assert_ne!(signal.game_reacquisition_revision(), waiting_revision);
    assert_ne!(routing.observed(), waiting_wake);
}

#[test]
fn unavailable_inventory_retains_an_exact_binding_and_does_not_mint_a_new_owner() {
    let mut broker = Broker::default();
    let active = owner(Some("dcs"));
    broker.set_owner(Some(active.clone())).unwrap();
    observe(&mut broker, "dcs", observed()).unwrap();
    let token = broker.token("dcs").unwrap();
    let mut unavailable = observed();
    unavailable.process = None;
    unavailable.error = Some("Inventory busy".into());

    let old = broker.begin_observation().unwrap();
    let current = broker.begin_observation().unwrap();
    broker.observe("dcs", current, unavailable.clone(), true).unwrap();
    broker.observe("dcs", old, observed(), false).unwrap();
    assert_eq!(broker.token("dcs").unwrap(), token);
    assert_eq!(broker.view("dcs")["processVerified"], false);
    assert!(broker.ensure_observed_process("dcs").is_err());

    let mut replacement = active;
    replacement.run_generation += 1;
    broker.set_owner(Some(replacement.clone())).unwrap();
    assert!(broker.token("dcs").is_err());
    observe(&mut broker, "dcs", observed()).unwrap();
    assert_eq!(broker.token("dcs").unwrap().owner, replacement);

    unavailable.installation = json!({"executable":"/other/DCS.exe"});
    let current = broker.begin_observation().unwrap();
    broker.observe("dcs", current, unavailable, true).unwrap();
    assert!(broker.token("dcs").is_err());
    assert!(broker.preparation("dcs").unwrap().is_none());
}
#[test]
fn older_observations_cannot_replace_a_new_preparation_or_applied_context() {
    let mut broker = Broker::default();
    let old = broker.begin_observation().unwrap();
    let mut old_snapshot = observed();
    old_snapshot.setup_ready = false;
    observe(&mut broker, "dcs", observed()).unwrap();
    let prepared = broker.preparation("dcs").unwrap().unwrap();
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    let token = broker.token("dcs").unwrap();
    broker.observe("dcs", old, old_snapshot, false).unwrap();
    assert_eq!(broker.preparation("dcs").unwrap(), Some(prepared.clone()));
    assert_eq!(broker.token("dcs").unwrap(), token);
    let mut malformed = observed();
    malformed.provider_setup_revision = "stale invalid data".into();
    broker.observe("dcs", old, malformed, false).unwrap();
    assert!(broker.validate_preparation(&prepared).is_ok());
    assert!(broker.validate(&token).is_ok());

    // Ordering is per game: a slow capture of another game still commits.
    broker.observe("x-plane-12", old, observed(), false).unwrap();
    assert!(broker.preparation("x-plane-12").unwrap().is_some());
    assert!(broker.token("x-plane-12").is_err());
}
#[test]
fn explicit_current_invalidation_fences_inflight_results_but_allows_fresh_evidence() {
    let mut broker = Broker::default();
    observe(&mut broker, "dcs", observed()).unwrap();
    let prepared = broker.preparation("dcs").unwrap().unwrap();
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    let token = broker.token("dcs").unwrap();
    let inflight = broker.begin_observation().unwrap();
    assert!(broker.invalidate_preparation(&prepared, "Current process exited".into()));
    broker.observe("dcs", inflight, observed(), false).unwrap();
    assert!(broker.preparation("dcs").unwrap().is_none());
    assert!(broker.validate(&token).is_err());
    observe(&mut broker, "dcs", observed()).unwrap();
    assert_ne!(broker.preparation("dcs").unwrap(), Some(prepared));
    assert!(broker.validate(&token).is_err());
    assert!(broker.token("dcs").is_ok());
}

#[test]
fn invalidation_wakes_only_when_a_visible_game_record_changes() {
    let mut broker = Broker::default();
    let changes = std::sync::Arc::new(crate::game_read_models::signals::Signal::default());
    let activity = std::sync::Arc::new(osg_streams::NativeWake::default());
    let resources = std::sync::Arc::new(osg_streams::NativeWake::default());
    changes.watch_activity(&activity);
    changes.watch_resources(&resources);
    broker.watch_changes(changes.clone());

    let activity_before_unknown = activity.observed();
    let resources_before_unknown = resources.observed();
    let initial_epoch = broker.observation_epoch;
    broker.invalidate("unknown-game", "Process exited".into());
    assert_eq!(activity.observed(), activity_before_unknown);
    assert_eq!(resources.observed(), resources_before_unknown);
    assert_eq!(broker.observation_epoch, initial_epoch + 1);
    assert_eq!(broker.observation_commits.get("unknown-game"), Some(&broker.observation_epoch));

    observe(&mut broker, "dcs", observed()).unwrap();
    let activity_before = activity.observed();
    let resources_before = resources.observed();
    let epoch_before = broker.observation_epoch;
    broker.invalidate("dcs", "Process exited".into());
    assert_ne!(activity.observed(), activity_before);
    assert_ne!(resources.observed(), resources_before);
    assert_eq!(broker.observation_epoch, epoch_before + 1);
    assert_eq!(broker.observation_commits.get("dcs"), Some(&broker.observation_epoch));

    let activity_after_change = activity.observed();
    let resources_after_change = resources.observed();
    let epoch_after_change = broker.observation_epoch;
    broker.invalidate("dcs", "Process exited".into());
    assert_eq!(activity.observed(), activity_after_change);
    assert_eq!(resources.observed(), resources_after_change);
    assert_eq!(broker.observation_epoch, epoch_after_change + 1);

    observe(&mut broker, "dcs", observed()).unwrap();
    let proof = broker.preparation("dcs").unwrap().unwrap();
    let activity_before_preparation = activity.observed();
    let resources_before_preparation = resources.observed();
    let epoch_before_preparation = broker.observation_epoch;
    assert!(broker.invalidate_preparation(&proof, "Preparation failed".into()));
    assert_ne!(activity.observed(), activity_before_preparation);
    assert_ne!(resources.observed(), resources_before_preparation);
    assert_eq!(broker.observation_epoch, epoch_before_preparation + 1);
}

#[test]
fn invalid_or_exhausted_observation_tickets_cannot_grant_or_restore_authority() {
    let mut broker = Broker::default();
    assert!(broker.observe("dcs", 0, observed(), false).is_err());
    assert!(broker.observe("dcs", 1, observed(), false).is_err());
    observe(&mut broker, "dcs", observed()).unwrap();
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    let token = broker.token("dcs").unwrap();
    broker.observation_epoch = u64::MAX;
    assert!(broker.begin_observation().is_err());
    assert!(broker.preparation("dcs").unwrap().is_none());
    assert!(broker.validate(&token).is_err());
    assert!(broker.observe("dcs", u64::MAX, observed(), false).is_err());
    assert!(broker.begin_observation().is_err());
    assert_eq!(broker.view("dcs")["status"], "error");
}
#[test]
fn preparation_proves_observation_without_granting_applied_authority() {
    let mut broker = Broker::default();
    observe(&mut broker, "dcs", observed()).unwrap();
    let proof = broker.preparation("dcs").unwrap().unwrap();
    assert!(broker.token("dcs").is_err());
    assert!(broker.validate_preparation(&proof).is_ok());
    broker.set_owner(Some(owner(Some("x-plane-12")))).unwrap();
    assert!(broker.token("dcs").is_err());
    assert!(broker.validate_preparation(&proof).is_ok());
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    let token = broker.token("dcs").unwrap();
    assert!(token.matches_preparation(&proof));
    broker.set_owner(None).unwrap();
    assert!(broker.token("dcs").is_err());
    assert!(broker.validate_preparation(&proof).is_ok());
}
#[test]
fn preparation_rejects_forgery_replacement_and_failed_new_evidence() {
    let mut broker = Broker::default();
    observe(&mut broker, "dcs", observed()).unwrap();
    let proof = broker.preparation("dcs").unwrap().unwrap();
    let mut forged = proof.clone();
    forged.session = uuid::Uuid::new_v4().to_string();
    assert!(broker.validate_preparation(&forged).is_err());
    let mut replacement = observed();
    replacement.process.as_mut().unwrap().epoch += 1;
    observe(&mut broker, "dcs", replacement).unwrap();
    assert!(broker.validate_preparation(&proof).is_err());
    let new = broker.preparation("dcs").unwrap().unwrap();
    let mut malformed = observed();
    malformed.provider_setup_revision = "bad".into();
    assert!(observe(&mut broker, "dcs", malformed).is_err());
    assert!(broker.validate_preparation(&new).is_err());
    assert!(broker.preparation("Invalid ID").is_err());
}
#[test]
fn late_preparation_failure_cannot_revoke_replacement_owner_but_current_failure_does() {
    let mut broker = Broker::default();
    observe(&mut broker, "dcs", observed()).unwrap();
    let old = broker.preparation("dcs").unwrap().unwrap();
    let mut replacement = observed();
    replacement.process.as_mut().unwrap().epoch += 1;
    observe(&mut broker, "dcs", replacement).unwrap();
    let current = broker.preparation("dcs").unwrap().unwrap();
    let mut next = owner(Some("dcs"));
    next.run_generation += 1;
    broker.set_owner(Some(next)).unwrap();
    let token = broker.token("dcs").unwrap();
    assert!(!broker.invalidate_preparation(&old, "Old preflight failed".into()));
    assert!(broker.validate(&token).is_ok());
    let mut forged = current.clone();
    forged.provider_setup_revision = format!("sha256:{}", "f".repeat(64));
    assert!(!broker.invalidate_preparation(&forged, "Wrong setup".into()));
    assert!(broker.validate(&token).is_ok());
    assert!(broker.invalidate_preparation(&current, "Current process inspection failed".into()));
    assert!(broker.validate(&token).is_err());
    assert!(broker.preparation("dcs").unwrap().is_none());
}
#[test]
fn closed_wrong_and_unapplied_game_contexts_cannot_mint_capabilities() {
    let mut broker = Broker::default();
    observe(&mut broker, "dcs", observed()).unwrap();
    assert_eq!(broker.view("dcs")["status"], "waiting-profile");
    assert_eq!(broker.view("dcs")["processVerified"], true);
    assert!(broker.token("dcs").is_err());
    broker.set_owner(Some(owner(Some("x-plane-12")))).unwrap();
    assert!(broker.token("dcs").is_err());
    broker.set_owner(Some(owner(None))).unwrap();
    assert!(broker.token("dcs").is_err());
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    assert!(broker.token("dcs").is_ok());
    let mut missing = observed();
    missing.process = None;
    observe(&mut broker, "dcs", missing).unwrap();
    assert_eq!(broker.view("dcs")["status"], "waiting-game");
    assert!(broker.token("dcs").is_err());
}
#[test]
fn process_exit_replacement_and_pid_reuse_revoke_old_source_and_command_tokens() {
    let mut broker = Broker::default();
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    observe(&mut broker, "dcs", observed()).unwrap();
    let original = broker.token("dcs").unwrap();
    observe(&mut broker, "dcs", observed()).unwrap();
    assert_eq!(broker.token("dcs").unwrap(), original);
    let mut replacement = observed();
    replacement.process.as_mut().unwrap().epoch += 1;
    observe(&mut broker, "dcs", replacement).unwrap();
    assert!(broker.validate(&original).is_err());
    let replacement = broker.token("dcs").unwrap();
    let mut exit = observed();
    exit.process = None;
    observe(&mut broker, "dcs", exit).unwrap();
    assert!(broker.validate(&replacement).is_err());
    observe(&mut broker, "dcs", observed()).unwrap();
    assert!(broker.validate(&original).is_err());
}
#[test]
fn profile_stop_setup_and_capability_changes_require_new_admission() {
    let mut broker = Broker::default();
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    observe(&mut broker, "dcs", observed()).unwrap();
    let first = broker.token("dcs").unwrap();
    let mut setup = observed();
    setup.provider_setup_revision = format!("sha256:{}", "c".repeat(64));
    observe(&mut broker, "dcs", setup).unwrap();
    assert!(broker.validate(&first).is_err());
    let setup = broker.token("dcs").unwrap();
    let mut catalogue = observed();
    catalogue.catalogue_revision = Some(format!("sha256:{}", "d".repeat(64)));
    observe(&mut broker, "dcs", catalogue).unwrap();
    assert!(broker.validate(&setup).is_err());
    let catalogue = broker.token("dcs").unwrap();
    broker.set_owner(None).unwrap();
    assert!(broker.validate(&catalogue).is_err());
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    assert!(broker.validate(&catalogue).is_err());
}
#[test]
fn aircraft_identity_change_retires_authority_even_when_module_catalogue_is_unchanged() {
    let mut broker = Broker::default();
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    let mut aircraft = observed();
    aircraft.vehicle_id = Some("F-14B".into());
    observe(&mut broker, "dcs", aircraft.clone()).unwrap();
    let first = broker.preparation("dcs").unwrap().unwrap();
    let token = broker.token("dcs").unwrap();
    aircraft.vehicle_id = Some("F-14A-135-GR".into());
    observe(&mut broker, "dcs", aircraft).unwrap();
    let second = broker.preparation("dcs").unwrap().unwrap();
    assert_eq!(first.catalogue_revision, second.catalogue_revision);
    assert_ne!(first.session, second.session);
    assert!(broker.validate_preparation(&first).is_err());
    assert!(broker.validate(&token).is_err());
    assert!(!token.matches_preparation(&second));
}
#[test]
fn missing_setup_unsupported_platform_and_native_errors_are_distinct() {
    let mut broker = Broker::default();
    let mut observation = observed();
    observation.setup_ready = false;
    observe(&mut broker, "dcs", observation).unwrap();
    assert_eq!(broker.view("dcs")["status"], "setup-required");
    let mut observation = observed();
    observation.supported = false;
    observe(&mut broker, "dcs", observation).unwrap();
    assert_eq!(broker.view("dcs")["status"], "unsupported-platform");
    let mut observation = observed();
    observation.error = Some("Process inspection failed".into());
    observe(&mut broker, "dcs", observation).unwrap();
    assert_eq!(broker.view("dcs")["status"], "error");
    assert_eq!(broker.view("unknown")["status"], "waiting-game");
}
#[test]
fn malformed_authority_and_forged_token_cannot_admit() {
    let mut broker = Broker::default();
    let mut invalid = owner(Some("dcs"));
    invalid.session = "editor-selected-profile".into();
    assert!(broker.set_owner(Some(invalid)).is_err());
    assert!(observe(&mut broker, "Invalid ID", observed()).is_err());
    let mut invalid = observed();
    invalid.provider_setup_revision = "draft".into();
    assert!(observe(&mut broker, "dcs", invalid).is_err());
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    observe(&mut broker, "dcs", observed()).unwrap();
    let mut token = broker.token("dcs").unwrap();
    token.session = uuid::Uuid::new_v4().to_string();
    assert!(broker.validate(&token).is_err());
}
#[test]
fn malformed_new_evidence_withdraws_an_already_admitted_context() {
    let mut broker = Broker::default();
    broker.set_owner(Some(owner(Some("dcs")))).unwrap();
    observe(&mut broker, "dcs", observed()).unwrap();
    let token = broker.token("dcs").unwrap();
    let mut invalid = observed();
    invalid.process.as_mut().unwrap().epoch = 0;
    assert!(observe(&mut broker, "dcs", invalid).is_err());
    assert!(broker.validate(&token).is_err());
    assert_eq!(broker.view("dcs")["status"], "error");
    assert_eq!(broker.view("dcs")["processVerified"], false);
    observe(&mut broker, "dcs", observed()).unwrap();
    assert!(broker.validate(&token).is_err());
}
#[test]
fn generic_title_requires_verified_integration_evidence() {
    let title = "discovered-title";
    let mut broker = Broker::default();
    broker.set_owner(Some(owner(Some(title)))).unwrap();
    assert!(broker.preparation(title).unwrap().is_none());
    let mut evidence = observed();
    evidence.supported = false;
    observe(&mut broker, title, evidence).unwrap();
    assert!(broker.token(title).is_err());
    observe(&mut broker, title, observed()).unwrap();
    assert!(broker.token(title).is_ok());
    assert!(broker.set_owner(Some(owner(Some("Invalid ID")))).is_err());
}

#[test]
fn detached_binding_retires_proof_and_fences_in_flight_capture() {
    let mut broker = Broker::default();
    broker
        .set_owner(Some(owner(Some("discovered-title"))))
        .unwrap();
    observe(&mut broker, "discovered-title", observed()).unwrap();
    let token = broker.token("discovered-title").unwrap();
    let old_epoch = broker.begin_observation().unwrap();
    broker.retain_bindings(&[]);
    broker
        .observe("discovered-title", old_epoch, observed(), false)
        .unwrap();
    assert!(broker.validate(&token).is_err());
    assert!(broker.preparation("discovered-title").unwrap().is_none());
}

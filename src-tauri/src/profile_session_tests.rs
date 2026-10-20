use super::*;
use osg_profiles::compiler_models::{BackendCapabilities, CompileSnapshot};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Probe {
    calls: Vec<String>,
    discarded: Vec<RunOwner>,
    fail: Option<String>,
    owner: Option<RunOwner>,
    value: u32,
    hidden: bool,
    health: Health,
    reject_admission: bool,
}
struct Fake(Arc<Mutex<Probe>>);
fn checkpoint(probe: &mut Probe, name: String) -> Result<(), String> {
    probe.calls.push(name.clone());
    if probe.fail.as_ref() == Some(&name) {
        probe.fail = None;
        Err(format!("uncertain {name}"))
    } else {
        Ok(())
    }
}
impl Host for Fake {
    fn prepare(&mut self, candidate: &Candidate, _: &RunOwner) -> Result<Prepared, String> {
        let mut probe = self.0.lock().unwrap();
        checkpoint(&mut probe, "prepare".into())?;
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
        let report = osg_profiles::compiler::compile(&candidate.document, &snapshot);
        Ok(Prepared {
            plan: report.plan.unwrap(),
            admission: Admission {
                blockers: if probe.reject_admission {
                    vec![AdmissionBlocker {
                        kind: BlockerKind::RequiredResource,
                        message: "The native preflight found an unavailable required device."
                            .into(),
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            },
            program_hash: report.program_hash.unwrap(),
            capture_fingerprint: report.capture_fingerprint,
        })
    }
    fn validate_prepared(&mut self, _: &Prepared, _: &RunOwner) -> Result<(), String> {
        checkpoint(&mut self.0.lock().unwrap(), "validate".into())
    }
    fn capture(
        &mut self,
        _: &Prepared,
        _: &RunOwner,
        previous: Option<&OwnedRun>,
    ) -> Result<Vec<Receipt>, String> {
        let mut probe = self.0.lock().unwrap();
        checkpoint(&mut probe, "capture".into())?;
        // The baseline is transferred across runs rather than treating the
        // previous profile's override as the user's original configuration.
        Ok(previous.map(|run| run.receipts.clone()).unwrap_or_else(|| {
            vec![Receipt {
                kind: "fixture".into(),
                resource_id: "physical-fixture".into(),
                state: json!({"baseline":probe.value}),
            }]
        }))
    }
    fn set_authority(&mut self, owner: Option<&RunOwner>) -> Result<(), String> {
        let mut probe = self.0.lock().unwrap();
        probe.owner = owner.cloned();
        checkpoint(
            &mut probe,
            if owner.is_some() {
                "authorize"
            } else {
                "revoke"
            }
            .into(),
        )
    }
    fn neutralize(&mut self, _: &OwnedRun) -> Result<(), String> {
        let mut probe = self.0.lock().unwrap();
        probe.hidden = false;
        checkpoint(&mut probe, "neutralize".into())
    }
    fn enter(&mut self, phase: ApplyPhase, _: &Prepared, run: &OwnedRun) -> Result<(), String> {
        let mut probe = self.0.lock().unwrap();
        match phase {
            ApplyPhase::Overrides => probe.value = run.owner.profile_revision as u32 + 100,
            ApplyPhase::Visibility => probe.hidden = true,
            _ => {}
        }
        checkpoint(&mut probe, format!("enter:{phase:?}"))
    }
    fn restore(&mut self, run: &OwnedRun, recovery: bool) -> Result<(), String> {
        let mut probe = self.0.lock().unwrap();
        probe.value = run.receipts[0].state["baseline"].as_u64().unwrap() as u32;
        probe.hidden = false;
        checkpoint(&mut probe, format!("restore:{recovery}"))
    }
    fn health(&mut self, _: &OwnedRun) -> Result<Health, String> {
        let mut probe = self.0.lock().unwrap();
        checkpoint(&mut probe, "health".into())?;
        Ok(probe.health.clone())
    }
    fn discard_preparation(&mut self, owner: &RunOwner) {
        let mut probe=self.0.lock().unwrap();
        probe.calls.push("discard".into());
        probe.discarded.push(owner.clone());
    }
}
fn candidate() -> Candidate {
    Candidate { launch_preparation: false,
        document: osg_profiles::default_document(),
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: None,
        stage: None,
    }
}
fn coordinator(path: Option<PathBuf>) -> (Coordinator<Fake>, Arc<Mutex<Probe>>) {
    let probe = Arc::new(Mutex::new(Probe {
        value: 42,
        ..Default::default()
    }));
    (Coordinator::new(Fake(probe.clone()), path), probe)
}
fn temp() -> tempfile::TempDir {
    crate::test_directory()
}

#[test]
fn startup_is_zero_effect_and_default_needs_no_optional_backend() {
    let (mut runtime, probe) = coordinator(None);
    assert_eq!(runtime.status().state, State::Stopped);
    assert!(probe.lock().unwrap().calls.is_empty());
    runtime.apply(candidate()).unwrap();
    assert_eq!(runtime.status().state, State::Running);
    let owner = runtime.status().owner.clone().unwrap();
    assert!(runtime.validates(&owner));
    runtime.stop().unwrap();
    assert!(!runtime.validates(&owner));
    assert_eq!(probe.lock().unwrap().value, 42);
    assert!(!probe.lock().unwrap().hidden);
}
#[test]
fn prepare_capture_and_initial_journal_failures_preserve_previous_run() {
    for failure in ["prepare", "validate", "capture", "journal"] {
        let (mut runtime, probe) = coordinator(None);
        runtime.apply(candidate()).unwrap();
        let previous = runtime.status().owner.clone().unwrap();
        let baseline_calls = probe.lock().unwrap().calls.len();
        if failure == "journal" {
            runtime.store.fail_save = true;
        } else {
            probe.lock().unwrap().fail = Some(failure.into());
        }
        assert!(runtime.apply(candidate()).is_err(), "{failure}");
        assert!(runtime.validates(&previous), "{failure}");
        let probe = probe.lock().unwrap();
        assert_eq!(probe.owner.as_ref(), Some(&previous));
        assert!(!probe.calls[baseline_calls..]
            .iter()
            .any(|call| call == "revoke"));
    }
}
#[test]
fn successful_takeover_discards_only_the_retired_owner_after_durable_commit() {
    let (mut runtime,probe)=coordinator(None);
    runtime.apply(candidate()).unwrap();
    let old=runtime.status().owner.clone().unwrap();
    assert!(probe.lock().unwrap().discarded.is_empty());
    runtime.apply(candidate()).unwrap();
    let current=runtime.status().owner.clone().unwrap();
    assert_eq!(probe.lock().unwrap().discarded,vec![old]);
    assert!(runtime.validates(&current));
    runtime.store.fail_save=true;
    assert!(runtime.apply(candidate()).is_err());
    assert!(runtime.validates(&current));
    assert!(!probe.lock().unwrap().discarded.contains(&current));
}
#[test]
fn every_uncertain_activation_phase_is_compensated_and_never_replayed() {
    for phase in APPLY_PHASES {
        let (mut runtime, probe) = coordinator(None);
        runtime.apply(candidate()).unwrap();
        let previous = runtime.status().owner.clone().unwrap();
        probe.lock().unwrap().fail = Some(format!("enter:{phase:?}"));
        assert!(runtime.apply(candidate()).is_err());
        assert_eq!(runtime.status().state, State::RecoveryNeeded);
        assert!(!runtime.validates(&previous));
        let observed = probe.lock().unwrap();
        assert!(observed.owner.is_none());
        assert_eq!(observed.value, 42);
        assert!(!observed.hidden);
        drop(observed);
        let calls = probe.lock().unwrap().calls.len();
        runtime.recover().unwrap();
        assert!(!probe.lock().unwrap().calls[calls..]
            .iter()
            .any(|call| call.starts_with("enter:")));
        assert_eq!(runtime.status().state, State::Stopped);
    }
}
#[test]
fn legacy_paused_journal_opens_stopped_and_new_save_drops_the_latch() {
    let folder = temp();
    let path = folder.path().join("session.json");
    std::fs::write(&path, br#"{"schemaVersion":1,"paused":true,"active":null,"pending":null}"#).unwrap();
    let (mut runtime, probe) = coordinator(Some(path.clone()));
    assert_eq!(runtime.status().state, State::Stopped);
    assert!(probe.lock().unwrap().calls.is_empty());
    runtime.apply(candidate()).unwrap();
    assert_eq!(runtime.status().state, State::Running);
    runtime.stop().unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(saved.get("paused").is_none());
}

#[test]
fn restart_requires_explicit_owned_recovery_without_starting_workers_or_rehiding() {
    let folder = temp();
    let path = folder.path().join("session.json");
    let (mut runtime, _) = coordinator(Some(path.clone()));
    runtime.apply(candidate()).unwrap();
    drop(runtime);
    let (mut restarted, probe) = coordinator(Some(path));
    assert_eq!(restarted.status().state, State::RecoveryNeeded);
    assert!(probe.lock().unwrap().calls.is_empty());
    assert!(restarted.apply(candidate()).is_err());
    restarted.recover().unwrap();
    assert!(probe
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|call| call == "restore:true"));
    assert!(!probe
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|call| call.starts_with("enter:")));
    assert_eq!(restarted.status().state, State::Stopped);
}
#[test]
fn owned_setup_is_restored_across_switches_and_stale_workers_are_fenced() {
    let (mut runtime, probe) = coordinator(None);
    runtime.apply(candidate()).unwrap();
    let old = runtime.status().owner.clone().unwrap();
    let mut next = candidate();
    next.document.revision = 2;
    runtime.apply(next).unwrap();
    let new = runtime.status().owner.clone().unwrap();
    assert!(!runtime.validates(&old));
    assert!(runtime.validates(&new));
    assert!(new.generation > old.generation);
    runtime.stop().unwrap();
    assert_eq!(probe.lock().unwrap().value, 42);
}
#[test]
fn provider_loss_degrades_without_guessing_a_replacement_and_game_exit_stops_authority() {
    let (mut runtime, probe) = coordinator(None);
    runtime.apply(candidate()).unwrap();
    let owner = runtime.status().owner.clone().unwrap();
    probe.lock().unwrap().health.degraded =
        vec!["Assigned input disconnected; branch fallback is active.".into()];
    runtime.poll().unwrap();
    assert_eq!(runtime.status().state, State::Degraded);
    assert!(runtime.validates(&owner));
    probe.lock().unwrap().health.stop = Some("Verified game process exited.".into());
    runtime.poll().unwrap();
    assert_eq!(runtime.status().state, State::Faulted);
    assert!(!runtime.validates(&owner));
    assert!(probe.lock().unwrap().owner.is_none());
}
#[test]
fn restoration_failure_retains_receipts_until_explicit_recovery() {
    let folder = temp();
    let path = folder.path().join("session.json");
    let (mut runtime, probe) = coordinator(Some(path.clone()));
    runtime.apply(candidate()).unwrap();
    probe.lock().unwrap().fail = Some("restore:false".into());
    assert!(runtime.stop().is_err());
    assert_eq!(runtime.status().state, State::RecoveryNeeded);
    drop(runtime);
    let (mut restarted, _) = coordinator(Some(path));
    assert_eq!(restarted.status().state, State::RecoveryNeeded);
    restarted.recover().unwrap();
}
#[test]
fn overlapping_owner_external_edits_and_corrupt_journals_preserve_evidence() {
    let folder = temp();
    let path = folder.path().join("session.json");
    let (mut runtime, _) = coordinator(Some(path.clone()));
    runtime.apply(candidate()).unwrap();
    let (mut second, probe) = coordinator(Some(path.clone()));
    assert_eq!(second.status().state, State::RecoveryNeeded);
    assert!(second.recover().is_err());
    assert!(probe
        .lock()
        .unwrap()
        .calls
        .iter()
        .all(|call| call == "revoke"));
    drop(second);
    std::fs::write(&path, b"external").unwrap();
    assert!(runtime.stop().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"external");
    drop(runtime);
    let (mut corrupt, probe) = coordinator(Some(path.clone()));
    assert_eq!(corrupt.status().state, State::RecoveryNeeded);
    assert!(corrupt.recover().is_err());
    assert!(probe
        .lock()
        .unwrap()
        .calls
        .iter()
        .all(|call| call == "revoke"));
    assert_eq!(std::fs::read(path).unwrap(), b"external");
}
#[test]
fn independent_edits_are_guarded_and_emergency_stop_is_available() {
    let (mut runtime, _) = coordinator(None);
    runtime.apply(candidate()).unwrap();
    for command in [
        "mapping_start",
        "mapping_configure",
        "companion_feedback_routes",
        "game_activate",
        "claim",
        "config_apply",
        "action_invoke",
    ] {
        assert!(runtime.guard_request(command).is_err(), "{command}");
    }
    for command in [
        "profile_status",
        "profile_get",
        "mapping_stop",
        "controller_visibility_restore",
    ] {
        assert!(runtime.guard_request(command).is_ok(), "{command}");
    }
}

#[test]
fn recovery_envelope_rejects_missing_fields_forged_phases_and_oversized_data() {
    let folder = temp();
    let path = folder.path().join("session.json");
    let (mut runtime, _) = coordinator(Some(path.clone()));
    runtime.apply(candidate()).unwrap();
    drop(runtime);
    let original: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut cases = vec![];
    let mut value = original.clone();
    value.as_object_mut().unwrap().remove("active");
    cases.push(value);
    let mut value = original.clone();
    value["active"]["owner"]
        .as_object_mut()
        .unwrap()
        .remove("gameId");
    cases.push(value);
    let mut value = original.clone();
    value["active"]["attempted"] = json!(["start"]);
    cases.push(value);
    let mut value = original.clone();
    value["active"]["snapshot"]["execute"] = json!(true);
    cases.push(value);
    let mut value = original.clone();
    value["active"]["receipts"][0]["state"]["large"] = json!("x".repeat(128 * 1024));
    cases.push(value);
    let mut value = original.clone();
    value["active"]["receipts"] = json!([
        value["active"]["receipts"][0],
        value["active"]["receipts"][0]
    ]);
    cases.push(value);
    for value in cases {
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let (mut runtime, probe) = coordinator(Some(path.clone()));
        assert_eq!(runtime.status().state, State::RecoveryNeeded);
        assert!(runtime.recover().is_err());
        assert!(probe
            .lock()
            .unwrap()
            .calls
            .iter()
            .all(|call| call == "revoke"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    std::fs::write(&path, vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
    let (mut runtime, _) = coordinator(Some(path.clone()));
    assert!(runtime.recover().is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 2 * 1024 * 1024 + 1);
}

#[test]
fn legacy_pause_does_not_bypass_owned_recovery_or_block_apply_after_cleanup() {
    let folder = temp();
    let path = folder.path().join("session.json");
    let (mut runtime, _) = coordinator(Some(path.clone()));
    runtime.apply(candidate()).unwrap();
    drop(runtime);
    let mut saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    saved["paused"] = serde_json::json!(true);
    std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let (mut restarted, probe) = coordinator(Some(path));
    assert_eq!(restarted.status().state, State::RecoveryNeeded);
    assert!(probe.lock().unwrap().calls.is_empty());
    assert!(restarted.apply(candidate()).is_err());
    restarted.recover().unwrap();
    assert_eq!(restarted.status().state, State::Stopped);
    restarted.apply(candidate()).unwrap();
    assert_eq!(restarted.status().state, State::Running);
}

#[test]
fn stop_revokes_workers_and_hiding_even_when_durable_storage_fails() {
    let (mut runtime, probe) = coordinator(None);
    runtime.apply(candidate()).unwrap();
    let owner = runtime.status().owner.clone().unwrap();
    runtime.store.fail_save = true;
    assert!(runtime.stop().is_err());
    assert_eq!(runtime.status().state, State::RecoveryNeeded);
    assert!(!runtime.validates(&owner));
    assert!(probe.lock().unwrap().owner.is_none());
    assert!(!probe.lock().unwrap().hidden);
    assert!(runtime.apply(candidate()).is_err());
    runtime.store.fail_save = false;
    runtime.recover().unwrap();
    assert_eq!(runtime.status().state, State::Stopped);
}

#[test]
fn native_stage_is_bound_to_the_complete_candidate_and_consumed_once() {
    let original = candidate();
    let mutations: Vec<Box<dyn Fn(&mut Candidate)>> = vec![
        Box::new(|value| value.document.name = "Changed draft".into()),
        Box::new(|value| value.document.revision += 1),
        Box::new(|value| value.read_only = true),
        Box::new(|value| value.game_id = Some("dcs".into())),
        Box::new(|value| value.launch_preparation = true),
        Box::new(|value| value.admission_epoch = Some(7)),
        Box::new(|value| {
            value
                .assignments
                .push(osg_profiles::store::LocalAssignment {
                    resource_id: "logical".into(),
                    source_id: "physical".into(),
                    ports: vec![],
                })
        }),
    ];
    for mutate in mutations {
        let stage = Stage::new(&original, 42u64).unwrap();
        let mut changed = original.clone();
        mutate(&mut changed);
        assert!(stage.take::<u64>(&changed).is_err());
        assert_eq!(stage.take::<u64>(&original).unwrap(), 42);
        assert!(stage.take::<u64>(&original).is_err());
    }
    let stage = Stage::new(&original, 42u64).unwrap();
    assert!(stage.take::<String>(&original).is_err());
    assert!(stage.take::<u64>(&original).is_err());
    let mut invalid = original.clone();
    invalid.document.name.clear();
    assert!(Stage::new(&invalid, ()).is_err());
    assert!(!format!("{stage:?}").contains("42"));
}

#[test]
fn blocked_native_preflight_does_not_block_active_health_or_change_the_owner() {
    use std::{sync::mpsc, time::Duration};
    let (mut runtime, probe) = coordinator(None);
    runtime.apply(candidate()).unwrap();
    let owner = runtime.status().owner.clone().unwrap();
    let runtime = Arc::new(Mutex::new(runtime));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let staged = runtime.clone();
    let job = std::thread::spawn(move || {
        apply_staged(&staged, candidate(), |candidate| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Stage::new(candidate, ())
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // Separate bounded thread proves the health path acquires the mutex while
    // preflight is still blocked, instead of only after it eventually returns.
    let (renewed_tx, renewed_rx) = mpsc::channel();
    let polling = runtime.clone();
    let poll_owner = owner.clone();
    let poll = std::thread::spawn(move || {
        for _ in 0..3 {
            let mut runtime = polling.lock().unwrap();
            runtime.poll().unwrap();
            assert!(runtime.validates(&poll_owner));
        }
        renewed_tx.send(()).unwrap();
    });
    let renewed = renewed_rx.recv_timeout(Duration::from_secs(2));
    probe.lock().unwrap().reject_admission = true;
    release_tx.send(()).unwrap();
    assert!(job.join().unwrap().is_err());
    poll.join().unwrap();
    assert!(
        renewed.is_ok(),
        "Health must renew before slow preflight is released."
    );
    assert!(runtime.lock().unwrap().validates(&owner));
    assert_eq!(probe.lock().unwrap().owner.as_ref(), Some(&owner));
    assert_eq!(
        probe
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| *call == "health")
            .count(),
        3
    );
}

#[test]
fn staged_apply_rechecks_stop_admission_and_drops_unarmed_payloads() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Unarmed(Arc<AtomicUsize>);
    impl Drop for Unarmed {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let (mut runtime, probe) = coordinator(None);
    runtime.apply(candidate()).unwrap();
    let runtime = Mutex::new(runtime);
    let dropped = Arc::new(AtomicUsize::new(0));
    let result = apply_staged_admitted(&runtime, candidate(), |candidate| {
        let stage = Stage::new(candidate, Unarmed(dropped.clone()))?;
        // Stop revokes the run while preparation is off-lock. Manager's
        // admission fence rejects its late result before ownership can return.
        runtime.lock().unwrap().stop()?;
        Ok(stage)
    }, |_| Err::<(), _>("Stop cancelled the staged admission.".to_owned()), || {});
    assert!(result.is_err());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.lock().unwrap().status().state, State::Stopped);
    assert!(probe.lock().unwrap().owner.is_none());
    let (runtime, _) = coordinator(None);
    let runtime = Mutex::new(runtime);
    let mut read_only = candidate();
    read_only.read_only = true;
    assert!(apply_staged(&runtime, read_only, |_| panic!("Read-only must not stage")).is_err());
    let mut reused = candidate();
    reused.stage = Some(Stage::new(&reused, ()).unwrap());
    assert!(apply_staged(&runtime, reused, |_| panic!(
        "A supplied stage must not replay"
    ))
    .is_err());
    apply_staged(&runtime, candidate(), |candidate| Stage::new(candidate, ())).unwrap();
    assert_eq!(runtime.lock().unwrap().status().state, State::Running);
}

#[test]
fn visibility_handoff_source_is_passive_exact_active_provenance_and_never_pending_recovery() {
    let (mut runtime, probe) = coordinator(None);
    assert!(runtime.visibility_handoff_source().unwrap().is_none());
    runtime.apply(candidate()).unwrap();
    let calls = probe.lock().unwrap().calls.clone();
    let mut captured = runtime.visibility_handoff_source().unwrap().unwrap();
    assert_eq!(captured.owner, runtime.status().owner.clone().unwrap());
    assert_eq!(captured.snapshot, runtime.status().applied_snapshot.clone().unwrap());
    captured.receipts.clear();
    assert!(!runtime.visibility_handoff_source().unwrap().unwrap().receipts.is_empty());
    assert_eq!(probe.lock().unwrap().calls, calls);
    runtime.status.state = State::Degraded;
    assert!(runtime.visibility_handoff_source().unwrap().is_some());
    runtime.status.owner.as_mut().unwrap().generation += 1;
    assert!(runtime.visibility_handoff_source().is_err());
    runtime.status.owner = Some(captured.owner.clone());
    runtime.status.applied_snapshot = None;
    assert!(runtime.visibility_handoff_source().is_err());
    runtime.status.applied_snapshot = Some(captured.snapshot.clone());
    runtime.manifest.pending = Some(journal::Transition {
        incoming: None, outgoing: runtime.manifest.active.clone(),
    });
    assert!(runtime.visibility_handoff_source().is_err());
    runtime.manifest.pending = None;
    runtime.status.state = State::RecoveryNeeded;
    assert!(runtime.visibility_handoff_source().is_err());
    runtime.status.state = State::Running;
    runtime.stop().unwrap();
    assert!(runtime.visibility_handoff_source().unwrap().is_none());
}

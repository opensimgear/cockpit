use super::*;
use osg_mapping::{engine::Counter, profile::Layout};
use osg_virtual_controller::{
    feeder::Message,
    session::{Channel, Factory, Session},
    vjoy::Binding,
};
#[derive(Default)]
struct FakeState {
    qualified: bool,
    provision_qualified: bool,
    provisioning: Vec<ProvisionRef>,
    provisioning_error: Option<String>,
    job: Option<ProvisionJob>,
    recoveries: Vec<String>,
    cancellations: Vec<String>,
    messages: Vec<Message>,
    spawned: usize,
    terminated: usize,
    fail_output: bool,
    replace_before_output: bool,
}
#[derive(Default)]
struct Fake(Mutex<FakeState>);
struct Feeder(Arc<Fake>);
impl Channel for Feeder {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        assert!(
            matches!(message, Message::Recover { .. }),
            "cleanup must never start or replay output"
        );
        let mut state = self.0 .0.lock().unwrap();
        state.messages.push(message.clone());
        if state.fail_output {
            Err("Cleanup reply was lost.".into())
        } else {
            Ok(())
        }
    }
    fn alive(&mut self) -> bool {
        true
    }
    fn terminate(&mut self) {
        self.0 .0.lock().unwrap().terminated += 1;
    }
}
impl Operations for Arc<Fake> {
    fn output_qualified(&self) -> bool {
        self.0.lock().unwrap().qualified
    }
    fn provision_qualified(&self) -> bool {
        self.0.lock().unwrap().provision_qualified
    }
    fn recover_output(&self, path: PathBuf, expected: &Record) -> Result<(), String> {
        if self.0.lock().unwrap().replace_before_output {
            let mut replacement = expected.clone();
            replacement.token.generation.0 += 1;
            sql_write(&path, serde_json::to_vec(&replacement).unwrap()).unwrap();
        }
        let fake = self.clone();
        let factory: Factory = Arc::new(move || {
            fake.0.lock().unwrap().spawned += 1;
            Ok(Box::new(Feeder(fake.clone())))
        });
        Session::recover_stored_exact(factory, path, expected)
    }
    fn provisioning(&self) -> Result<Vec<ProvisionRef>, String> {
        let state = self.0.lock().unwrap();
        if let Some(error) = &state.provisioning_error {
            Err(error.clone())
        } else {
            Ok(state.provisioning.clone())
        }
    }
    fn job(&self) -> Option<ProvisionJob> {
        self.0.lock().unwrap().job.clone()
    }
    fn recover_provision(&self, operation: &str) -> Result<ProvisionJob, String> {
        let mut state = self.0.lock().unwrap();
        state.recoveries.push(operation.into());
        let job = job(operation);
        state.job = Some(job.clone());
        Ok(job)
    }
    fn cancel_provision(&self, id: &str) -> Result<ProvisionJob, String> {
        let mut state = self.0.lock().unwrap();
        let job = state
            .job
            .as_mut()
            .filter(|job| job.active && job.job_id == id)
            .ok_or("No exact active cleanup job.")?;
        job.state = "cancelling".into();
        let job = job.clone();
        state.cancellations.push(id.into());
        Ok(job)
    }
}
fn setup() -> (tempfile::TempDir, Recovery, Arc<Fake>) {
    let root = crate::test_directory();
    let fake = Arc::new(Fake::default());
    fake.0.lock().unwrap().qualified = true;
    fake.0.lock().unwrap().provision_qualified = true;
    let recovery = Recovery {
        root: Some(root.path().canonicalize().unwrap()),
        ops: Arc::new(fake.clone()),
        operation: Mutex::new(()),
    };
    (root, recovery, fake)
}
fn token() -> RunToken {
    RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    }
}
fn binding() -> Binding {
    Binding {
        target: "retained-target".into(),
        slot: 1,
        axes: vec![],
        layout: Layout {
            buttons: 1,
            axes: vec![],
            hats: 0,
        },
    }
}
fn record(root: &Path, id: JournalId, phase: Phase) -> (PathBuf, Record) {
    let path = match id {
        JournalId::Manual => root.join("output-recovery.json"),
        JournalId::Companion { file } => {
            root.join("companion-recovery").join(format!("{file}.json"))
        }
    };
    let mut store = Store::open(path.clone()).unwrap();
    store.begin(token(), vec![binding()]).unwrap();
    if phase != Phase::Intent {
        store.transition(Phase::Confirmed).unwrap();
    }
    if matches!(phase, Phase::Releasing | Phase::Clean) {
        store.transition(Phase::Releasing).unwrap();
    }
    if phase == Phase::Clean {
        store.transition(Phase::Clean).unwrap();
    }
    (path, store.record().unwrap().clone())
}
fn journal_ref(id: JournalId, record: &Record) -> JournalRef {
    JournalRef {
        id,
        owner: record.token.clone(),
        record_hash: digest(record).unwrap(),
    }
}
fn companion(n: usize) -> JournalId {
    JournalId::Companion {
        file: format!("instance-{n:032x}"),
    }
}
fn provision() -> ProvisionRef {
    ProvisionRef {
        operation: uuid::Uuid::new_v4().to_string(),
        record_hash: format!("sha256:{}", "a".repeat(64)),
        target: 1,
        refresh_uncertain: true,
    }
}
fn job(operation: &str) -> ProvisionJob {
    ProvisionJob {
        job_id: uuid::Uuid::new_v4().to_string(),
        operation: operation.into(),
        active: true,
        state: "authorizing".into(),
        message: "Cleanup only".into(),
    }
}
#[test]
fn construction_and_status_ignore_obsolete_configs_and_do_not_create_storage() {
    let root = crate::test_directory();
    for name in [
        "local-settings.json",
        "companion-routes.json",
        "feedback-routes.json",
        "profiles.json",
    ] {
        std::fs::write(root.path().join(name), b"malformed retired configuration").unwrap();
    }
    let recovery = Recovery::new(Some(root.path().canonicalize().unwrap()), false);
    assert!(!recovery.status().required);
    assert!(recovery.guard_clear().is_ok());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 4);
    let missing = root.path().canonicalize().unwrap().join("does-not-exist");
    assert!(
        !Recovery::new(Some(missing.clone()), false)
            .status()
            .required
    );
    assert!(!missing.exists());
    assert!(!Recovery::new(None, false).status().required);
}
#[test]
fn retained_inventory_finds_manual_and_companion_owners_without_route_configuration() {
    let (root, recovery, _) = setup();
    record(root.path(), JournalId::Manual, Phase::Confirmed);
    record(root.path(), companion(1), Phase::Intent);
    record(root.path(), companion(2), Phase::Releasing);
    record(root.path(), companion(3), Phase::Clean);
    let status = recovery.status();
    assert!(status.required && status.diagnostics.is_empty());
    assert_eq!(status.outputs.len(), 3);
    assert!(status
        .outputs
        .iter()
        .all(|output| output.slots == [1] && hash(&output.reference.record_hash)));
    assert!(recovery
        .guard_clear()
        .unwrap_err()
        .contains("before acquiring"));
}
#[test]
fn malformed_or_unrecognized_retained_ownership_blocks_without_mutation() {
    let (root, recovery, _) = setup();
    let path = root.path().join("output-recovery.json");
    sql_write(&path, b"invalid journal").unwrap();
    assert!(recovery.status().required);
    assert_eq!(sql_read(&path).unwrap(), b"invalid journal");
    osg_persistence::test_support::write_evidence(&path,"recovery-output",None).unwrap();
    let child = root.path().join("companion-recovery");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(child.join("operator-notes.txt"), b"ignore").unwrap();
    assert!(!recovery.status().required);
    sql_write(&child.join("unknown.json"), b"preserve").unwrap();
    assert!(recovery.status().diagnostics[0].contains("unknown companion"));
    assert_eq!(
        sql_read(&child.join("unknown.json")).unwrap(),
        b"preserve"
    );
}
#[test]
fn retained_paths_and_inventory_bounds_fail_closed() {
    let (root, recovery, _) = setup();
    assert!(recovery
        .path(&JournalId::Companion {
            file: "../output-recovery".into()
        })
        .is_err());
    assert!(recovery
        .path(&JournalId::Companion {
            file: format!("instance-{}", "A".repeat(32))
        })
        .is_err());
    assert!(recovery.path(&companion(1)).is_err());
    for n in 0..=MAX_COMPANION_JOURNALS {
        record(root.path(), companion(n), Phase::Clean);
    }
    assert!(recovery.status().diagnostics[0].contains("exceeds its bound"));
    let relative = Recovery {
        root: Some("relative".into()),
        ops: recovery.ops.clone(),
        operation: Mutex::new(()),
    };
    assert!(relative.status().required);
    let regular = root.path().join("ordinary-file");
    std::fs::write(&regular, b"x").unwrap();
    let invalid = Recovery {
        root: Some(regular),
        ops: recovery.ops.clone(),
        operation: Mutex::new(()),
    };
    assert!(invalid.status().required);
}
#[test]
fn exact_confirmed_cleanup_sends_only_recover_and_cleans_the_same_owner() {
    let (root, recovery, fake) = setup();
    let (path, saved) = record(root.path(), companion(1), Phase::Confirmed);
    let reference = journal_ref(companion(1), &saved);
    recovery.recover(&reference).unwrap();
    let state = fake.0.lock().unwrap();
    assert_eq!(state.spawned, 1);
    assert_eq!(state.terminated, 1);
    assert!(matches!(
        state.messages.as_slice(),
        [Message::Recover { .. }]
    ));
    let after = output_journal::inspect(&path).unwrap().unwrap();
    assert_eq!(after.token, saved.token);
    assert_eq!(after.phase, Phase::Clean);
    drop(state);
    assert!(!recovery.status().required);
}
#[test]
fn stale_owner_hash_or_raced_record_never_opens_a_feeder() {
    let (root, recovery, fake) = setup();
    let (path, saved) = record(root.path(), JournalId::Manual, Phase::Confirmed);
    let mut reference = journal_ref(JournalId::Manual, &saved);
    reference.owner.generation.0 += 1;
    assert!(recovery.recover(&reference).is_err());
    reference.owner = saved.token.clone();
    reference.record_hash = "sha256:stale".into();
    assert!(recovery.recover(&reference).is_err());
    reference.record_hash = digest(&saved).unwrap();
    fake.0.lock().unwrap().replace_before_output = true;
    assert!(recovery
        .recover(&reference)
        .unwrap_err()
        .contains("complete retained output journal changed"));
    assert_eq!(fake.0.lock().unwrap().spawned, 0);
    assert_eq!(
        output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .token
            .generation
            .0,
        2
    );
}
#[test]
fn ambiguous_acknowledgement_requires_exact_owner_and_locked_full_record_without_output() {
    let (root, recovery, fake) = setup();
    let (path, saved) = record(root.path(), JournalId::Manual, Phase::Intent);
    let reference = journal_ref(JournalId::Manual, &saved);
    assert!(recovery
        .recover(&reference)
        .unwrap_err()
        .contains("ambiguous gaps"));
    let mut stale = reference.clone();
    stale.owner.session = uuid::Uuid::new_v4().to_string();
    assert!(recovery.acknowledge(&stale).is_err());
    let locked = Store::open(path.clone()).unwrap();
    assert!(recovery
        .acknowledge(&reference)
        .unwrap_err()
        .contains("Another Cockpit"));
    drop(locked);
    recovery.acknowledge(&reference).unwrap();
    assert_eq!(
        output_journal::inspect(&path).unwrap().unwrap().phase,
        Phase::Clean
    );
    assert_eq!(fake.0.lock().unwrap().spawned, 0);
    let (_, confirmed) = record(root.path(), JournalId::Manual, Phase::Confirmed);
    assert!(recovery
        .acknowledge(&journal_ref(JournalId::Manual, &confirmed))
        .is_err());
}
#[test]
fn failed_cleanup_stays_ambiguous_and_never_automatically_retries() {
    let (root, recovery, fake) = setup();
    let (path, saved) = record(root.path(), JournalId::Manual, Phase::Confirmed);
    let reference = journal_ref(JournalId::Manual, &saved);
    fake.0.lock().unwrap().fail_output = true;
    assert!(recovery.recover(&reference).is_err());
    assert_eq!(
        output_journal::inspect(&path).unwrap().unwrap().phase,
        Phase::Releasing
    );
    assert!(recovery.recover(&reference).is_err());
    assert_eq!(fake.0.lock().unwrap().spawned, 1);
    assert!(recovery.guard_clear().is_err());
}
#[test]
fn qualification_blocks_recovery_acknowledgement_and_provision_cleanup() {
    let (root, recovery, fake) = setup();
    let (path, saved) = record(root.path(), JournalId::Manual, Phase::Intent);
    fake.0.lock().unwrap().qualified = false;
    fake.0.lock().unwrap().provision_qualified = false;
    let reference = journal_ref(JournalId::Manual, &saved);
    assert_eq!(recovery.recover(&reference).unwrap_err(), QUALIFICATION);
    assert_eq!(recovery.acknowledge(&reference).unwrap_err(), QUALIFICATION);
    assert!(recovery.recover_provision(&provision()).is_err());
    assert_eq!(
        sql_read(&path).unwrap(),
        serde_json::to_vec(&saved).unwrap()
    );
    assert_eq!(fake.0.lock().unwrap().spawned, 0);
}
#[test]
fn provisioning_cleanup_pins_complete_native_operation_and_only_recovers_or_cancels() {
    let (_, recovery, fake) = setup();
    let recorded = provision();
    fake.0.lock().unwrap().provisioning = vec![recorded.clone()];
    let mut stale = recorded.clone();
    stale.refresh_uncertain = false;
    assert!(recovery.recover_provision(&stale).is_err());
    stale = recorded.clone();
    stale.operation = uuid::Uuid::new_v4().to_string();
    assert!(recovery.recover_provision(&stale).is_err());
    stale.operation = "not-an-operation".into();
    assert!(recovery.recover_provision(&stale).is_err());
    assert!(fake.0.lock().unwrap().recoveries.is_empty());
    let job = recovery.recover_provision(&recorded).unwrap();
    assert_eq!(
        fake.0.lock().unwrap().recoveries,
        [recorded.operation.clone()]
    );
    assert!(recovery.status().required);
    assert!(recovery.cancel_provision("bad-job").is_err());
    assert!(recovery
        .cancel_provision(&uuid::Uuid::new_v4().to_string())
        .is_err());
    assert_eq!(
        recovery.cancel_provision(&job.job_id).unwrap().state,
        "cancelling"
    );
    assert_eq!(fake.0.lock().unwrap().cancellations, [job.job_id]);
}
#[test]
fn invalid_provisioning_inventory_and_native_failures_block_new_acquisition() {
    let (_, recovery, fake) = setup();
    fake.0.lock().unwrap().provisioning_error = Some("protected registry unreadable".into());
    assert!(recovery.status().required);
    fake.0.lock().unwrap().provisioning_error = None;
    let valid = provision();
    for records in [
        vec![ProvisionRef {
            record_hash: "bad".into(),
            ..valid.clone()
        }],
        vec![valid.clone(); 2],
        vec![ProvisionRef {
            target: 0,
            ..valid.clone()
        }],
        vec![valid.clone(); 17],
    ] {
        fake.0.lock().unwrap().provisioning = records;
        let status = recovery.status();
        assert!(status.required && status.provisioning.is_empty());
    }
    fake.0.lock().unwrap().provisioning.clear();
    fake.0.lock().unwrap().job = Some(job(&valid.operation));
    assert!(recovery.guard_clear().is_err());
}
#[test]
fn retained_game_owner_cleanup_is_exact_and_never_reads_or_resumes_baseline_configuration() {
    let (root, recovery, fake) = setup();
    let owner = uuid::Uuid::new_v4().to_string();
    assert!(recovery.capture_mapping_owner("invalid").is_err());
    assert!(recovery.recover_mapping_owner("invalid", &None).is_err());
    let missing = recovery.capture_mapping_owner(&owner).unwrap();
    assert!(missing.is_none());
    recovery.recover_mapping_owner(&owner, &missing).unwrap();
    let (_, saved) = record(root.path(), JournalId::Manual, Phase::Confirmed);
    assert!(recovery
        .capture_mapping_owner(&owner)
        .unwrap_err()
        .contains("Another output owner"));
    assert!(recovery
        .recover_mapping_owner(&saved.token.session, &missing)
        .unwrap_err()
        .contains("appeared after"));
    let reference = recovery
        .capture_mapping_owner(&saved.token.session)
        .unwrap();
    assert!(recovery.recover_mapping_owner(&owner, &reference).is_err());
    let mut wrong_journal = reference.clone().unwrap();
    wrong_journal.id = companion(1);
    assert!(recovery
        .recover_mapping_owner(&saved.token.session, &Some(wrong_journal))
        .is_err());
    recovery
        .recover_mapping_owner(&saved.token.session, &reference)
        .unwrap();
    let clean = recovery
        .capture_mapping_owner(&saved.token.session)
        .unwrap();
    assert!(clean.is_none());
    recovery
        .recover_mapping_owner(&saved.token.session, &clean)
        .unwrap();
    assert_eq!(fake.0.lock().unwrap().spawned, 1);
    assert!(!root.path().join("local-settings.json").exists());
}
#[test]
fn disabled_native_operators_preserve_qualification_and_have_no_background_job() {
    let native = Native {
        enabled: false,
        #[cfg(windows)]
        provisioning: Mutex::new(Default::default()),
    };
    assert!(!native.output_qualified());
    assert!(!native.provision_qualified());
    assert!(native.provisioning().unwrap().is_empty());
    assert!(native.job().is_none());
    let temp = crate::test_directory();
    let (path, saved) = record(temp.path(), JournalId::Manual, Phase::Confirmed);
    assert!(native.recover_output(path, &saved).is_err());
    assert!(native
        .recover_provision(&uuid::Uuid::new_v4().to_string())
        .is_err());
    assert!(native
        .cancel_provision(&uuid::Uuid::new_v4().to_string())
        .is_err());
    assert!(!canonical("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"));
}

#[test]
fn strict_cleanup_requests_cannot_accept_config_paths_resume_or_provision_creation() {
    use serde_json::json;
    let (root, recovery, fake) = setup();
    let (_, saved) = record(root.path(), JournalId::Manual, Phase::Intent);
    let reference = journal_ref(JournalId::Manual, &saved);
    for request in [
        json!({"command":"mapping_preview_start"}),
        json!({"command":"profile_retired_recovery_status","profileId":"old"}),
        json!({"command":"profile_retired_recovery_recover"}),
        json!({"command":"profile_retired_recovery_recover","reference":{"id":{"kind":"manual","path":"elsewhere"},"owner":saved.token,"recordHash":digest(&saved).unwrap()}}),
        json!({"command":"profile_retired_recovery_provision_recover","reference":{"operation":uuid::Uuid::new_v4().to_string(),"create":1}}),
    ] {
        assert!(recovery.request(&request).is_err());
    }
    let status = recovery
        .request(&json!({"command":"profile_retired_recovery_status"}))
        .unwrap();
    assert_eq!(status["required"], true);
    assert_eq!(
        status["outputs"][0]["reference"]["recordHash"],
        reference.record_hash
    );
    assert_eq!(
        recovery
            .request(
                &json!({"command":"profile_retired_recovery_acknowledge","reference":reference})
            )
            .unwrap()["required"],
        false
    );
    assert!(fake.0.lock().unwrap().messages.is_empty());
    let (_, saved) = record(root.path(), JournalId::Manual, Phase::Confirmed);
    assert_eq!(recovery.request(&json!({"command":"profile_retired_recovery_recover","reference":journal_ref(JournalId::Manual,&saved)})).unwrap()["required"],false);
    let provision = provision();
    fake.0.lock().unwrap().provisioning = vec![provision.clone()];
    let status = recovery
        .request(
            &json!({"command":"profile_retired_recovery_provision_recover","reference":provision}),
        )
        .unwrap();
    assert_eq!(status["job"]["active"], true);
    assert!(recovery
        .request(&json!({"command":"profile_retired_recovery_provision_cancel","jobId":3}))
        .is_err());
    assert_eq!(recovery.request(&json!({"command":"profile_retired_recovery_provision_cancel","jobId":status["job"]["jobId"]})).unwrap()["job"]["state"],"cancelling");
    recovery.shutdown();
    assert_eq!(fake.0.lock().unwrap().cancellations.len(), 2);
}

#[test]
fn nonregular_journal_lock_or_parent_traversal_never_opens_a_feeder() {
    let (root, recovery, fake) = setup();
    let path = root.path().join("output-recovery.json");
    std::fs::create_dir(&path).unwrap();
    assert!(recovery.status().required);
    assert!(recovery.capture_mapping_owner(&token().session).is_err());
    std::fs::remove_dir(&path).unwrap();
    let (path, saved) = record(root.path(), JournalId::Manual, Phase::Confirmed);
    let original = sql_read(&path).unwrap();
    let lock = path.with_extension("lock");
    std::fs::remove_file(&lock).unwrap();
    std::fs::create_dir(&lock).unwrap();
    assert!(recovery.status().required);
    assert!(recovery
        .recover(&journal_ref(JournalId::Manual, &saved))
        .is_err());
    assert_eq!(sql_read(&path).unwrap(), original);
    assert_eq!(fake.0.lock().unwrap().spawned, 0);
    assert!(ordinary(&root.path().join("new/../output-recovery.json"), false).is_err());
    let file = root.path().join("file");
    std::fs::write(&file, b"regular").unwrap();
    assert!(ordinary(&file.join("journal.json"), false).is_err());
}
#[test]
fn redirected_existing_ancestor_never_reads_or_mutates_the_retained_target() {
    let (root, recovery, fake) = setup();
    let real = root.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let nested = real.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let (path, saved) = record(&nested, JournalId::Manual, Phase::Confirmed);
    let original = sql_read(&path).unwrap();
    let alias = root.path().join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    #[cfg(windows)]
    {
        // Junctions need no symlink privilege/developer mode on Windows CI.
        for path in [&alias, &real] {
            assert!(!path
                .to_string_lossy()
                .chars()
                .any(|c| "\"&|<>%!\r\n".contains(c)));
        }
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("cmd.exe")
            .raw_arg(format!(
                "/D /C mklink /J \"{}\" \"{}\"",
                alias.display(),
                real.display()
            ))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let redirected = Recovery {
        root: Some(alias.join("nested")),
        ops: recovery.ops.clone(),
        operation: Mutex::new(()),
    };
    let status = redirected.status();
    assert!(status.required && status.outputs.is_empty());
    assert!(status.diagnostics[0].contains("reparse"));
    assert!(redirected
        .recover(&journal_ref(JournalId::Manual, &saved))
        .is_err());
    assert!(redirected
        .acknowledge(&journal_ref(JournalId::Manual, &saved))
        .is_err());
    assert!(Recovery {
        root: Some(alias.join("not-created")),
        ops: recovery.ops.clone(),
        operation: Mutex::new(())
    }
    .guard_clear()
    .is_err());
    assert_eq!(sql_read(&path).unwrap(), original);
    assert_eq!(fake.0.lock().unwrap().spawned, 0);
}

fn sql_read(path:&std::path::Path)->Result<Vec<u8>,String>{osg_persistence::test_support::read_evidence(path,"recovery-output")}
fn sql_write(path:&std::path::Path,data:impl AsRef<[u8]>)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-output",Some(data.as_ref()))}

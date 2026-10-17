use super::*;
use osg_controller_input::visibility::{
    journal::{self, Backend, Store},
    policy::{Policy, Selection, State},
};
use osg_profiles::{compiler_models::*, resources::*, store::LocalAssignment};
use std::sync::Mutex;
const PHYSICAL: &str = "11111111-1111-4111-8111-111111111111";
pub(crate) fn owner() -> RunOwner {
    RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
        profile_revision: 1,
        game_id: None,
    }
}
pub(crate) fn snapshot() -> CompileSnapshot {
    CompileSnapshot {
        inventory_session: uuid::Uuid::new_v4().to_string(),
        inventory_revision: "1".into(),
        assignments: vec![LocalAssignment {
            resource_id: "physical".into(),
            source_id: "retained".into(),
            ports: vec![],
        }],
        resources: vec![CapturedResource {
            summary: ResourceSummary {
                id: "retained".into(),
                name: "Physical".into(),
                resource_type: ResourceType::HostController,
                availability: Availability::Online,
                generation: "1".into(),
                capability_revision: format!("sha256:{}", "0".repeat(64)),
                port_count: 0,
                cached_port_count: 0,
                connections: vec![Connection {
                    scope: "host-controllers".into(),
                    key: PHYSICAL.into(),
                    route: Route {
                        id: format!("hid:{PHYSICAL}"),
                        transport: Transport::Hid,
                        provider_generation: "1:1".into(),
                        attributes: BTreeMap::new(),
                        baseline_ready: true,
                        writable: false,
                        priority: 100,
                    },
                }],
                attributes: BTreeMap::new(),
                diagnostics: vec![],
            },
            ports: vec![],
            tokens: vec![],
            provider_setup_revision: None,
            game_id: None,
        }],
        game_context: None,
        backend: BackendCapabilities {
            visibility_available: true,
            visibility_qualified: true,
            ..Default::default()
        },
        inventory_diagnostics: vec![],
        read_only: false,
    }
}
pub(crate) fn plan(snapshot: &CompileSnapshot) -> ExecutionPlan {
    let mut empty = snapshot.clone();
    empty.assignments.clear();
    empty.resources.clear();
    let mut plan = osg_profiles::compiler::compile(&osg_profiles::default_document(), &empty)
        .plan
        .unwrap();
    plan.visibility = vec![VisibilitySummary {
        resource_id: "physical".into(),
        inventory_resource_id: Some("retained".into()),
        mode: VisibilityMode::HiddenWhileActive,
        readiness: PlanReadiness::Ready,
    }];
    plan
}
pub(crate) fn owned_run(owner: RunOwner, receipts: Vec<Receipt>) -> OwnedRun {
    OwnedRun {
        owner,
        snapshot:
            osg_profiles::session::AppliedSnapshot::capture(&osg_profiles::default_document())
                .unwrap()
                .summary()
                .clone(),
        program_hash: "hash".into(),
        capture_fingerprint: "capture".into(),
        receipts,
        attempted: vec![crate::profile_session::ApplyPhase::Visibility],
    }
}
fn state() -> State {
    State {
        active: false,
        inverse: false,
        applications: vec![r"\Device\Volume1\Existing.exe".into()],
        hidden: vec![],
    }
}
struct Driver(Arc<Mutex<State>>);
impl Backend for Driver {
    fn read(&mut self) -> Result<State, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String> {
        let mut actual = self.0.lock().unwrap();
        if &*actual != expected {
            return Err("External edit preserved".into());
        }
        *actual = desired.clone();
        Ok(())
    }
}
pub(crate) struct Fake {
    root: tempfile::TempDir,
    pub(crate) driver: Arc<Mutex<State>>,
    pub(crate) calls: Mutex<Vec<(String, Value)>>,
    pub(crate) owned: Mutex<Option<String>>,
    job: String,
    session: String,
    intent: Mutex<Intent>,
    ambiguous: Mutex<bool>,
    pub(crate) asynchronous: Mutex<bool>,
    pub(crate) start_pending: Mutex<bool>,
    ambiguous_stop: Mutex<bool>,
    restoring: Mutex<bool>,
    malformed: Mutex<Option<Value>>,
}
impl Fake {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            root: tempfile::tempdir().unwrap(),
            driver: Arc::new(Mutex::new(state())),
            calls: Mutex::new(vec![]),
            owned: Mutex::new(None),
            job: uuid::Uuid::new_v4().to_string(),
            session: uuid::Uuid::new_v4().to_string(),
            intent: Mutex::new(Intent::HideAll),
            ambiguous: Mutex::new(false),
            asynchronous: Mutex::new(false),
            start_pending: Mutex::new(false),
            ambiguous_stop: Mutex::new(false),
            restoring: Mutex::new(false),
            malformed: Mutex::new(None),
        })
    }
    fn path(&self, owner: &str) -> std::path::PathBuf {
        self.root
            .path()
            .join(format!("game-visibility-{owner}.json"))
    }
    fn status(&self, phase: &str) -> Value {
        json!({"jobId":self.job,"active":true,"phase":phase,"message":"fixture","sessionId":self.session})
    }
    fn restore(&self, owner: &str) -> Result<Value, String> {
        let path = self.path(owner);
        let mut store = Store::open(path)?;
        if let Some(record) = store.read()? {
            journal::restore(&mut Driver(self.driver.clone()), &mut store, &record.id)?;
        }
        *self.owned.lock().unwrap() = None;
        Ok(json!({"active":false,"phase":"restored"}))
    }
}
impl VisibilityIo for Fake {
    fn request(&self, operation: &str, value: &Value) -> Result<Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push((operation.into(), value.clone()));
        if let Some(value) = self.malformed.lock().unwrap().take() {
            return Ok(value);
        }
        match operation {
            "capture" => {
                let ids = value["sourceIds"].as_array().unwrap();
                let policies = ids
                    .iter()
                    .map(|id| {
                        (
                            id.as_str().unwrap().to_owned(),
                            self.intent.lock().unwrap().clone(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                Ok(json!({"mode":value["mode"],"sourceIds":ids,"policies":policies}))
            }
            "preflight" => {
                let intent: CapturedIntent = serde_json::from_value(value.clone()).unwrap();
                let selections = intent
                    .source_ids
                    .iter()
                    .enumerate()
                    .map(|(i, _)| Selection {
                        instance: format!(r"HID\FIXTURE\{i}"),
                        policy: match &*self.intent.lock().unwrap() {
                            Intent::HideFromProcess { .. } => Policy::HideFromProcess {
                                executable: r"\Device\Volume1\Game.exe".into(),
                            },
                            _ => Policy::HideAll,
                        },
                    })
                    .collect();
                let policy = Plan::build(
                    self.driver.lock().unwrap().clone(),
                    selections,
                    r"\Device\Volume1\Cockpit.exe".into(),
                )?;
                Ok(json!({"intent":intent,"policy":policy}))
            }
            "start" => {
                let owner = value["owner"].as_str().unwrap();
                if self.owned.lock().unwrap().is_some() {
                    return Err("Another native visibility owner".into());
                }
                let prepared: Preparation =
                    serde_json::from_value(value["prepared"].clone()).unwrap();
                let policy = prepared.policy.unwrap();
                let mut store = Store::open(self.path(owner))?;
                journal::apply(&mut Driver(self.driver.clone()), &mut store, policy, || {
                    Ok(())
                })?;
                *self.owned.lock().unwrap() = Some(owner.into());
                if *self.ambiguous.lock().unwrap() {
                    return Err("Ambiguous native Start reply".into());
                }
                Ok(self.status(if *self.start_pending.lock().unwrap() {
                    "applying"
                } else {
                    "active"
                }))
            }
            "stop" | "poll" => {
                let owner = value["owner"].as_str().unwrap();
                if self
                    .owned
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|current| current != owner)
                {
                    return Err("Another native visibility owner; preserve it".into());
                }
                if operation == "poll" && !*self.restoring.lock().unwrap() {
                    return Ok(self.status("active"));
                }
                if operation == "stop" && *self.asynchronous.lock().unwrap() {
                    *self.restoring.lock().unwrap() = true;
                    if std::mem::take(&mut *self.ambiguous_stop.lock().unwrap()) {
                        return Err("Lost restoration reply".into());
                    }
                    return Ok(self.status("restoring"));
                }
                self.restore(owner)
            }
            _ => Err("Unexpected native operation".into()),
        }
    }
}
#[test]
fn preparation_preserves_saved_scope_receipt_has_no_replay_policy_and_start_is_one_shot() {
    let io = Fake::new();
    *io.intent.lock().unwrap() = Intent::HideFromProcess {
        executable: r"C:\Games\Sim.exe".into(),
    };
    let snapshot = snapshot();
    let owner = owner();
    let before = io.driver.lock().unwrap().clone();
    let mut adapter = Visibility::prepare(&plan(&snapshot), &snapshot, &owner, io.clone()).unwrap();
    assert_eq!(*io.driver.lock().unwrap(), before);
    let receipts = adapter.receipts().unwrap();
    assert_eq!(receipts.len(), 1);
    let exported = serde_json::to_string(&receipts).unwrap();
    assert!(
        !exported.contains("Sim.exe")
            && !exported.contains("selections")
            && !exported.contains("prepared")
    );
    assert_eq!(adapter.start().unwrap(), Progress::Active);
    assert!(io.driver.lock().unwrap().inverse);
    assert!(adapter.start().is_err());
    assert_eq!(adapter.poll().unwrap(), Progress::Active);
    assert_eq!(adapter.stop().unwrap(), Progress::Restored);
    assert_eq!(adapter.stop().unwrap(), Progress::Restored);
    assert_eq!(*io.driver.lock().unwrap(), before);
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
fn only_exact_supported_physical_assignments_can_prepare_hiding() {
    let base = snapshot();
    for case in 0..10 {
        let io = Fake::new();
        let mut snapshot = base.clone();
        let mut plan = plan(&snapshot);
        match case {
            0 => snapshot.backend.visibility_qualified = false,
            1 => plan.visibility[0].readiness = PlanReadiness::Blocked,
            2 => snapshot.resources.clear(),
            3 => snapshot.assignments.clear(),
            4 => snapshot.resources[0].summary.resource_type = ResourceType::Companion,
            5 => snapshot.resources[0].summary.availability = Availability::Offline,
            6 => snapshot.resources[0].summary.connections.clear(),
            7 => {
                snapshot.resources[0].summary.connections[0]
                    .route
                    .baseline_ready = false
            }
            8 => snapshot.resources[0].summary.connections[0].route.id = "hid:other".into(),
            _ => plan.visibility.push(plan.visibility[0].clone()),
        }
        assert!(
            Visibility::prepare(&plan, &snapshot, &owner(), io.clone()).is_err(),
            "case{case}"
        );
        assert!(io.calls.lock().unwrap().is_empty());
    }
    for intent in [
        Intent::Visible,
        Intent::BlockOriginalInput,
        Intent::HideFromProcess {
            executable: "relative.exe".into(),
        },
    ] {
        let io = Fake::new();
        *io.intent.lock().unwrap() = intent;
        assert!(Visibility::prepare(&plan(&base), &base, &owner(), io.clone()).is_err());
        assert_eq!(
            io.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(op, _)| op.as_str())
                .collect::<Vec<_>>(),
            vec!["capture"]
        );
    }
}
#[test]
fn visible_intent_is_io_free_and_does_not_unhide_external_policy() {
    let io = Fake::new();
    io.driver
        .lock()
        .unwrap()
        .hidden
        .push(r"HID\EXTERNAL\1".into());
    let before = io.driver.lock().unwrap().clone();
    let snapshot = snapshot();
    let mut plan = plan(&snapshot);
    plan.visibility[0].mode = VisibilityMode::Visible;
    let mut adapter = Visibility::prepare(&plan, &snapshot, &owner(), io.clone()).unwrap();
    assert!(adapter.is_empty());
    assert!(adapter.receipts().unwrap().is_empty());
    assert_eq!(adapter.start().unwrap(), Progress::Restored);
    assert_eq!(adapter.stop().unwrap(), Progress::Restored);
    assert_eq!(*io.driver.lock().unwrap(), before);
    assert!(io.calls.lock().unwrap().is_empty());
}
#[test]
fn actual_native_qualification_cannot_be_bypassed_by_ready_captured_flags() {
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    assert!(controllers.game_visibility("preflight",&json!({"mode":"hidhide-session","sourceIds":[PHYSICAL],"policies":{PHYSICAL:{"mode":"hide-all"}}})).unwrap_err().contains("not qualified"));
    let snapshot = snapshot();
    let error = Visibility::prepare(&plan(&snapshot), &snapshot, &owner(), controllers)
        .err()
        .unwrap();
    assert!(
        error.contains("explicit policy")
            || error.contains("qualified")
            || error.contains("policy"),
        "{error}"
    );
}
#[test]
fn recovery_only_restores_exact_retained_owner_and_preserves_external_changes() {
    let io = Fake::new();
    let snapshot = snapshot();
    let owner = owner();
    let mut adapter = Visibility::prepare(&plan(&snapshot), &snapshot, &owner, io.clone()).unwrap();
    let run = owned_run(owner.clone(), adapter.receipts().unwrap());
    adapter.start().unwrap();
    *io.owned.lock().unwrap() = None;
    let mut recovery = Visibility::from_receipts(&run, io.clone())
        .unwrap()
        .unwrap();
    assert!(recovery.start().is_err());
    assert_eq!(recovery.stop().unwrap(), Progress::Restored);
    assert_eq!(*io.driver.lock().unwrap(), state());
    assert_eq!(
        io.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(op, _)| op == "start")
            .count(),
        1
    );
    let mut adapter = Visibility::prepare(&plan(&snapshot), &snapshot, &owner, io.clone()).unwrap();
    let run = owned_run(owner.clone(), adapter.receipts().unwrap());
    adapter.start().unwrap();
    io.driver
        .lock()
        .unwrap()
        .applications
        .push(r"\Device\Volume1\External.exe".into());
    let external = io.driver.lock().unwrap().clone();
    let mut recovery = Visibility::from_receipts(&run, io.clone())
        .unwrap()
        .unwrap();
    assert!(recovery.stop().is_err());
    assert_eq!(*io.driver.lock().unwrap(), external);
    let receipt: RecoveryReceipt = serde_json::from_value(run.receipts[0].state.clone()).unwrap();
    assert!(
        !journal::inspect(&io.path(&receipt.visibility_owner))
            .unwrap()
            .unwrap()
            .clean
    );
}
#[test]
fn ambiguous_start_is_not_replayed_and_restore_polls_without_recancelling() {
    let io = Fake::new();
    *io.ambiguous.lock().unwrap() = true;
    let snapshot = snapshot();
    let mut adapter =
        Visibility::prepare(&plan(&snapshot), &snapshot, &owner(), io.clone()).unwrap();
    assert!(adapter.poll().is_err());
    assert!(adapter.start().is_err());
    assert!(adapter.start().is_err());
    *io.asynchronous.lock().unwrap() = true;
    assert_eq!(adapter.stop().unwrap(), Progress::Pending);
    *io.asynchronous.lock().unwrap() = false;
    assert_eq!(adapter.stop().unwrap(), Progress::Restored);
    assert_eq!(*io.driver.lock().unwrap(), state());
    assert_eq!(
        io.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(op, _)| op == "start")
            .count(),
        1
    );
    let calls = io.calls.lock().unwrap();
    let stop = calls.iter().position(|(op, _)| op == "stop").unwrap();
    assert_eq!(calls[stop + 1].0, "poll");
}
#[test]
fn malformed_stale_and_forged_receipts_cannot_claim_another_owner() {
    let io = Fake::new();
    let snapshot = snapshot();
    let owner = owner();
    let adapter = Visibility::prepare(&plan(&snapshot), &snapshot, &owner, io.clone()).unwrap();
    let base = owned_run(owner.clone(), adapter.receipts().unwrap());
    for case in 0..7 {
        let mut run = base.clone();
        match case {
            0 => run.owner.generation += 1,
            1 => run.receipts.push(run.receipts[0].clone()),
            2 => run.receipts[0].resource_id = "other".into(),
            3 => run.receipts[0].state["visibilityOwner"] = json!("../outside"),
            4 => run.receipts[0].state["sourceIds"] = json!([]),
            5 => run.receipts[0].state["prepared"] = json!({}),
            _ => run.receipts[0].state["preparationHash"] = json!("bad"),
        }
        assert!(
            Visibility::from_receipts(&run, io.clone()).is_err(),
            "case{case}"
        );
    }
    let mut empty = base;
    empty.receipts.clear();
    assert!(Visibility::from_receipts(&empty, io.clone())
        .unwrap()
        .is_none());
    let mut invalid = owner;
    invalid.generation = 0;
    assert!(Visibility::prepare(&plan(&snapshot), &snapshot, &invalid, io.clone()).is_err());
}
#[test]
fn worker_replacement_or_failed_state_never_satisfies_visibility_admission() {
    let io = Fake::new();
    let snapshot = snapshot();
    let mut adapter =
        Visibility::prepare(&plan(&snapshot), &snapshot, &owner(), io.clone()).unwrap();
    adapter.start().unwrap();
    *io.malformed.lock().unwrap() =
        Some(json!({"active":true,"phase":"active","jobId":uuid::Uuid::new_v4().to_string()}));
    assert!(adapter.poll().is_err());
    *io.malformed.lock().unwrap() =
        Some(json!({"active":false,"phase":"attention","message":"external conflict"}));
    assert!(adapter.poll().unwrap_err().contains("external conflict"));
    *io.malformed.lock().unwrap() = Some(json!({"active":false,"phase":"restored"}));
    assert!(adapter.poll().is_err());
}

#[test]
fn stopping_an_unstarted_preparation_permanently_prevents_late_hiding() {
    let io = Fake::new();
    let snapshot = snapshot();
    let mut adapter =
        Visibility::prepare(&plan(&snapshot), &snapshot, &owner(), io.clone()).unwrap();
    assert_eq!(adapter.stop().unwrap(), Progress::Restored);
    assert!(adapter.start().is_err());
    assert_eq!(*io.driver.lock().unwrap(), state());
    assert!(io
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(operation, _)| operation != "start"));
}

#[test]
fn staging_is_unowned_and_binding_does_not_repeat_native_preflight() {
    let io = Fake::new();
    let snapshot = snapshot();
    let before = io.driver.lock().unwrap().clone();
    let staged = Visibility::stage(&plan(&snapshot), &snapshot, io.clone()).unwrap();
    assert_eq!(*io.driver.lock().unwrap(), before);
    assert!(io.owned.lock().unwrap().is_none());
    let calls = io.calls.lock().unwrap().len();
    let owner = owner();
    let mut visibility = staged.bind(&owner).unwrap();
    assert_eq!(io.calls.lock().unwrap().len(), calls);
    assert_eq!(
        visibility.receipts().unwrap()[0].state["owner"],
        json!(owner)
    );
    assert!(!visibility.needs_cleanup());
    visibility.start().unwrap();
    assert!(visibility.needs_cleanup());
    visibility.stop().unwrap();
    assert!(!visibility.needs_cleanup());

    let staged = Visibility::stage(&plan(&snapshot), &snapshot, io.clone()).unwrap();
    let calls = io.calls.lock().unwrap().len();
    let mut invalid = owner;
    invalid.generation = 0;
    assert!(staged.bind(&invalid).is_err());
    assert_eq!(io.calls.lock().unwrap().len(), calls);
}

#[test]
fn lost_first_stop_reply_retains_restoration_instead_of_recancelling() {
    let io = Fake::new();
    let snapshot = snapshot();
    let mut visibility = Visibility::stage(&plan(&snapshot), &snapshot, io.clone())
        .unwrap()
        .bind(&owner())
        .unwrap();
    visibility.start().unwrap();
    *io.asynchronous.lock().unwrap() = true;
    *io.ambiguous_stop.lock().unwrap() = true;
    assert!(visibility
        .stop()
        .unwrap_err()
        .contains("Lost restoration reply"));
    assert!(visibility.needs_cleanup());
    *io.asynchronous.lock().unwrap() = false;
    assert_eq!(visibility.stop().unwrap(), Progress::Restored);
    assert_eq!(*io.driver.lock().unwrap(), state());
    let calls = io.calls.lock().unwrap();
    let first_stop = calls
        .iter()
        .position(|(operation, _)| operation == "stop")
        .unwrap();
    assert_eq!(calls[first_stop + 1].0, "poll");
    assert!(!visibility.needs_cleanup());
}

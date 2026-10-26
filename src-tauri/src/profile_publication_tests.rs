use super::*;
use crate::stm32::{PublicationOperation, PublicationReply, PublicationRequest};

static TESTS: Mutex<()> = Mutex::new(());
const GATEWAY: &str = "stm32:0123456789abcdef01234567";

#[derive(Clone, Copy, PartialEq)]
enum ClaimMode {
    Confirmed,
    LostReply,
    ForeignLostReply,
}
struct FakeState {
    state: Value,
    owner: Option<String>,
    calls: Vec<(&'static str, bool)>,
    masks: Vec<(u32, u32)>,
    mode: ClaimMode,
    pending: bool,
}
struct FakeIo(Mutex<FakeState>);

fn target(index: u8, enabled: bool) -> Target {
    Target {
        resource: format!("controller-{index}"),
        inventory: format!("native-{index}"),
        gateway: GATEWAY.into(),
        module: format!("{:032x}", index as u32 + 1),
        epoch: 65536,
        boot: 10,
        configuration: 2,
        node: index as u32 + 1,
        source: index as u32 + 50,
        profile: json!({"buttons":8,"axes":0,"hats":0,"revision":1,"axisUsage":[],"neutral":[]}),
        slot: index,
        configured_enabled: true,
        report: index + 1,
        order: index as i32,
        enabled,
    }
}
fn owner() -> RunOwner {
    RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: "profile".into(),
        profile_revision: 1,
        game_id: None,
    }
}
impl FakeIo {
    fn new(targets: &[Target]) -> Arc<Self> {
        let modules = targets.iter().map(|t| json!({"id":t.module,"online":true,"directUsb":false,
            "boot":t.boot,"configurationGeneration":t.configuration,"node":t.node,"sourceNode":t.source,
            "profile":t.profile,"publicationSlot":t.slot,"configuredEnabled":t.configured_enabled,
            "publicationReportId":t.report,"configuredDescriptorIndex":t.order,"published":t.configured_enabled,
            "lease":66,"hasInput":false,"eventBaselineReady":false})).collect::<Vec<_>>();
        Arc::new(Self(Mutex::new(FakeState {
            state: json!({"gatewayType":"stm32-can-gateway",
            "gatewaySerial":"0123456789abcdef01234567","protocolVersion":3,"canProtocol":3,"controllerPublicationOverride":json!({"schema":1,"scope":"gateway-can","nonpersistent":true,"captureAtClaim":true,"restore":"compare-if-unchanged","minLeaseMs":250,"maxLeaseMs":5000,"maxTargets":32,"reconnectRequired":true}),
            "connected":true,"epoch":65536,"publicationRevision":17,"layoutPending":false,"modules":modules,
            "publication":{"phase":"idle","outcome":"no_change","leaseId":0,"expiresAtMs":0,
                "transition":"0","claimRevision":0,"targetMask":"00000000","enabledMask":"00000000","expectedReconnect":false}}),
            owner: None,
            calls: vec![],
            masks: vec![],
            mode: ClaimMode::Confirmed,
            pending: false,
        })))
    }
    fn change(&self, update: impl FnOnce(&mut FakeState)) {
        update(&mut self.0.lock().unwrap());
    }
    fn count(&self, operation: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(kind, _)| *kind == operation)
            .count()
    }
    fn settle(&self) {
        self.change(|s| {
            s.pending = false;
            s.state["layoutPending"] = json!(false);
            let enabled =
                u32::from_str_radix(s.state["publication"]["enabledMask"].as_str().unwrap(), 16)
                    .unwrap();
            let targets =
                u32::from_str_radix(s.state["publication"]["targetMask"].as_str().unwrap(), 16)
                    .unwrap();
            for m in s.state["modules"].as_array_mut().unwrap() {
                let bit = 1u32 << m["publicationSlot"].as_u64().unwrap();
                if targets & bit != 0 {
                    let visible = enabled & bit != 0;
                    if m["published"] != visible {
                        m["lease"] = json!(m["lease"].as_u64().unwrap() + 1);
                    }
                    m["published"] = json!(visible);
                    m["hasInput"] = json!(false);
                }
            }
        });
    }
}
impl Io for FakeIo {
    fn cached(&self, gateway: &str) -> Result<Value, String> {
        assert_eq!(gateway, GATEWAY);
        Ok(self.0.lock().unwrap().state.clone())
    }
    fn request(
        &self,
        gateway: &str,
        request: PublicationRequest,
    ) -> Result<PublicationReply, String> {
        assert_eq!(gateway, GATEWAY);
        assert_eq!(request.epoch, 65536);
        let mut s = self.0.lock().unwrap();
        let admitted = request
            .authority
            .as_ref()
            .is_some_and(|a| a.load(Ordering::Acquire));
        let mut owned = None;
        match request.operation {
            PublicationOperation::State => s.calls.push(("state", true)),
            PublicationOperation::Inspect { owner, lease } => {
                s.calls.push(("inspect", true));
                owned = Some(
                    s.owner.as_deref() == Some(&owner)
                        && s.state["publication"]["leaseId"] == lease
                        && s.state["publication"]["phase"] != "idle",
                );
            }
            PublicationOperation::Claim {
                owner,
                revision,
                target_mask,
                enabled_mask,
                ..
            } => {
                s.calls.push(("claim", admitted));
                s.masks.push((target_mask, enabled_mask));
                if !admitted {
                    return Err("Native run was revoked before admission".into());
                }
                if s.state["publication"]["phase"] != "idle" {
                    return Err("publication_busy".into());
                }
                s.owner = Some(if s.mode == ClaimMode::ForeignLostReply {
                    "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0".into()
                } else {
                    owner
                });
                s.state["publicationRevision"] =
                    json!(s.state["publicationRevision"].as_u64().unwrap() + 1);
                s.state["publication"] = json!({"phase":"owned","outcome":"ok","leaseId":73,"expiresAtMs":5000,
                    "transition":"1","claimRevision":revision,"targetMask":format!("{target_mask:08x}"),
                    "enabledMask":format!("{enabled_mask:08x}"),"expectedReconnect":false});
                let pending = s.pending;
                s.state["layoutPending"] = json!(pending);
                if !pending {
                    for m in s.state["modules"].as_array_mut().unwrap() {
                        let bit = 1u32 << m["publicationSlot"].as_u64().unwrap();
                        if target_mask & bit != 0 {
                            let visible = enabled_mask & bit != 0;
                            if m["published"] != visible {
                                m["lease"] = json!(m["lease"].as_u64().unwrap() + 1);
                            }
                            m["published"] = json!(visible);
                            m["hasInput"] = json!(true);
                        }
                    }
                }
                if s.mode != ClaimMode::Confirmed {
                    return Err("The one-shot claim reply was lost".into());
                }
            }
            PublicationOperation::Renew { owner, lease, .. } => {
                s.calls.push(("renew", admitted));
                if !admitted {
                    return Err("Native run was revoked before renewal admission".into());
                }
                if s.owner.as_deref() != Some(&owner) || s.state["publication"]["leaseId"] != lease
                {
                    return Err("publication_not_owned".into());
                }
                let expires = s.state["publication"]["expiresAtMs"].as_u64().unwrap() + 1000;
                s.state["publication"]["expiresAtMs"] = json!(expires);
            }
            PublicationOperation::Release { owner, lease } => {
                s.calls.push(("release", true));
                if s.owner.as_deref() != Some(&owner) || s.state["publication"]["leaseId"] != lease
                {
                    return Err("publication_not_owned".into());
                }
                s.state["publication"]["phase"] = json!("idle");
                s.state["publication"]["outcome"] = json!("no_change");
                s.state["publication"]["transition"] = json!("2");
                s.state["layoutPending"] = json!(false);
                s.state["publicationRevision"] =
                    json!(s.state["publicationRevision"].as_u64().unwrap() + 1);
                for m in s.state["modules"].as_array_mut().unwrap() {
                    if m["published"] != m["configuredEnabled"] {
                        m["lease"] = json!(m["lease"].as_u64().unwrap() + 1);
                    }
                    m["published"] = m["configuredEnabled"].clone();
                    m["hasInput"] = json!(false);
                }
            }
        }
        Ok(PublicationReply {
            state: s.state.clone(),
            owned,
        })
    }
}
fn prepare(targets: Vec<Target>, io: Arc<FakeIo>) -> Overrides {
    Overrides::prepare_io(&Staged { targets }, &owner(), io).unwrap()
}
fn wait(actor: &Overrides, predicate: impl Fn(&Status) -> bool) -> Status {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let status = actor.poll();
        if predicate(&status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "native actor did not settle: active={}, stopped={}, cleanup={}, error={:?}",
            status.active,
            status.stopped,
            status.cleanup_complete,
            status.error
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn stop(actor: &mut Overrides) {
    actor.stop().unwrap();
    wait(actor, |s| s.stopped && s.cleanup_complete);
}

#[test]
fn batches_exact_targets_and_seals_receipts_before_any_mutation() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false), target(1, true)];
    let io = FakeIo::new(&targets);
    let mut actor = prepare(targets.clone(), io.clone());
    assert_eq!(io.count("claim"), 0);
    assert_eq!(actor.receipts().len(), 1);
    let receipt = &actor.slots[0].receipt;
    let decoded = open_receipt(receipt, &actor.slots[0].value.owner).unwrap();
    assert_eq!(decoded.targets, targets);
    assert_eq!(decoded.target_mask, 3);
    assert_eq!(decoded.enabled_mask, 2);
    assert!(!serde_json::to_string(receipt)
        .unwrap()
        .contains(&decoded.secret));
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    assert_eq!(io.count("claim"), 1);
    assert_eq!(io.0.lock().unwrap().masks, vec![(3, 2)]);
    let verified = actor.verified(false).unwrap();
    verified.validate().unwrap();
    assert!(
        verified.states[GATEWAY]["modules"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["hasInput"] == true && m["eventBaselineReady"] == false),
        "fresh CAN input does not fabricate the independent event baseline"
    );
    stop(&mut actor);
    assert_eq!(io.count("release"), 1);
    assert_eq!(io.count("claim"), 1);
    actor.verified(true).unwrap().validate().unwrap();
}
#[test]
fn unsettled_descriptor_never_admits_execution_or_a_fabricated_baseline() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    io.change(|s| s.pending = true);
    let mut actor = prepare(targets, io.clone());
    actor.acquire().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while io.count("inspect") == 0 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!actor.poll().active);
    assert!(actor.verified(false).is_err());
    io.settle();
    thread::sleep(Duration::from_millis(60));
    assert!(!actor.poll().active);
    assert!(actor.verified(false).is_err());
    io.change(|s| s.state["modules"][0]["hasInput"] = json!(true));
    wait(&actor, |s| s.active);
    actor.verified(false).unwrap();
    stop(&mut actor);
}
#[test]
fn lost_claim_reply_never_replays_or_starts_and_cleanup_only_releases_our_proven_lease() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    io.change(|s| s.mode = ClaimMode::LostReply);
    let mut actor = prepare(targets, io.clone());
    actor.acquire().unwrap();
    let status = wait(&actor, |s| s.error.is_some());
    assert!(!status.active && status.error.is_some());
    stop(&mut actor);
    assert!(actor.verified(false).is_err());
    assert!(
        actor.poll().error.is_none(),
        "Exact confirmed restoration reconciles the ambiguous acquisition error"
    );
    actor.verified(true).unwrap().validate().unwrap();
    assert_eq!(io.count("claim"), 1);
    assert_eq!(io.count("renew"), 0);
    assert_eq!(io.count("release"), 1);
}
#[test]
fn revoked_authority_cannot_renew_and_stop_before_acquire_never_claims() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    let mut actor = prepare(targets.clone(), io.clone());
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    actor.slots[0]
        .shared
        .authority
        .store(false, Ordering::Release);
    assert!(actor.verified(false).is_err());
    wait(&actor, |s| s.error.is_some());
    stop(&mut actor);
    assert_eq!(io.count("claim"), 1);
    assert_eq!(io.count("release"), 1);
    assert!(io
        .0
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(kind, _)| *kind == "renew")
        .all(|(_, admitted)| !admitted));
    let io = FakeIo::new(&targets);
    let mut never_started = prepare(targets, io.clone());
    stop(&mut never_started);
    assert_eq!(io.count("claim"), 0);
    assert_eq!(io.count("renew"), 0);
    assert_eq!(io.count("release"), 0);
}

#[test]
fn completed_restoration_survives_repeated_withdrawal_without_rearming_the_actor() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    let mut actor = prepare(targets.clone(), io.clone());
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    let live = actor.verified(false).unwrap();

    actor.revoke_renewal();
    assert!(live.validate().is_err());
    assert!(actor.verified(false).is_err());
    assert!(actor.acquire().is_err());
    stop(&mut actor);
    let restored = actor.verified(true).unwrap();
    restored.validate().unwrap();
    assert!(actor.poll().error.is_none());
    let calls_after_restoration = io.0.lock().unwrap().calls.len();

    for _ in 0..3 {
        actor.revoke_renewal();
        actor.stop().unwrap();
        let status = actor.poll();
        assert!(status.stopped && status.cleanup_complete && status.error.is_none());
        actor.verified(true).unwrap().validate().unwrap();
        restored.validate().unwrap();
        assert!(actor.verified(false).is_err());
        assert!(live.validate().is_err());
        assert!(actor.acquire().is_err());
    }
    assert_eq!(io.0.lock().unwrap().calls.len(), calls_after_restoration);
    assert_eq!(io.count("claim"), 1);
    assert_eq!(io.count("release"), 1);

    // Immutable restoration proof still pins the exact observed CAN assignment.
    io.change(|s| s.state["modules"][0]["lease"] = json!(999));
    assert!(restored.validate().is_err());
    assert!(actor.verified(true).is_err());

    // Withdrawal before dispatch is permanent too; it cannot create a first Claim.
    let untouched_io = FakeIo::new(&targets);
    let mut untouched = prepare(targets, untouched_io.clone());
    untouched.revoke_renewal();
    assert!(untouched.acquire().is_err());
    stop(&mut untouched);
    assert_eq!(untouched_io.count("claim"), 0);
    assert_eq!(untouched_io.count("renew"), 0);
    assert_eq!(untouched_io.count("release"), 0);
}

#[test]
fn recapture_changes_only_revision_and_all_fixed_target_changes_fail_closed() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    let mut actor = prepare(targets.clone(), io.clone());
    let secret = actor.slots[0].value.secret.clone();
    io.change(|s| s.state["publicationRevision"] = json!(23));
    actor.recapture().unwrap();
    assert_eq!(actor.slots[0].value.revision, 23);
    assert_eq!(actor.targets(), targets);
    assert_eq!(actor.slots[0].value.secret, secret);
    assert_eq!(io.count("claim"), 0);
    for (field, value) in [
        ("boot", json!(11)),
        ("configurationGeneration", json!(3)),
        ("node", json!(2)),
        ("sourceNode", json!(51)),
        ("publicationSlot", json!(1)),
        ("configuredEnabled", json!(false)),
        ("publicationReportId", json!(2)),
        ("configuredDescriptorIndex", json!(1)),
        ("directUsb", json!(true)),
        ("online", json!(false)),
        ("profile", json!({"buttons":9})),
    ] {
        let mut changed = io.cached(GATEWAY).unwrap();
        changed["modules"][0][field] = value;
        assert!(
            !matches_target(&targets[0], &changed),
            "changed {field} was accepted"
        );
    }
    io.change(|s| s.state["modules"][0]["boot"] = json!(11));
    assert!(actor.recapture().is_err());
    stop(&mut actor);
}
#[test]
fn foreign_winner_with_same_revision_and_masks_never_becomes_our_owned_run() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    io.change(|s| s.mode = ClaimMode::ForeignLostReply);
    let mut actor = prepare(targets, io.clone());
    actor.acquire().unwrap();
    let status = wait(&actor, |s| s.stopped && s.cleanup_complete);
    assert!(
        !status.active
            && status
                .error
                .as_deref()
                .is_some_and(|s| s.contains("preserved"))
    );
    assert!(actor.verified(false).is_err());
    assert_eq!(io.count("claim"), 1);
    assert_eq!(io.count("renew"), 0);
    assert_eq!(io.count("release"), 0);
    assert_eq!(io.0.lock().unwrap().state["publication"]["phase"], "owned");
}
#[test]
fn newer_foreign_owner_and_later_unacknowledged_can_lease_are_never_adopted() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    let mut actor = prepare(targets.clone(), io.clone());
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    io.change(|s| {
        s.owner = Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into());
        s.state["publication"]["leaseId"] = json!(74);
        s.state["publication"]["claimRevision"] = json!(18);
    });
    let status = wait(&actor, |s| s.stopped && s.cleanup_complete);
    assert!(status.error.is_some());
    assert!(!status.active);
    assert_eq!(io.count("release"), 0);
    assert_eq!(io.count("claim"), 1);
    let io = FakeIo::new(&targets);
    let mut actor = prepare(targets, io.clone());
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    let pin = actor.verified(false).unwrap();
    io.change(|s| s.state["modules"][0]["lease"] = json!(999));
    assert!(pin.validate().is_err());
    let status = wait(&actor, |s| s.error.is_some());
    assert!(!status.active && status.error.is_some());
    stop(&mut actor);
    assert_eq!(io.count("claim"), 1);
    assert_eq!(io.count("release"), 1);
}
#[test]
fn crash_recovery_inspects_and_releases_only_the_exact_existing_private_receipt() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    let staged = prepare(targets, io.clone());
    let original = &staged.slots[0];
    let secret = original.value.secret.clone();
    io.request(
        GATEWAY,
        PublicationRequest {
            epoch: 65536,
            operation: PublicationOperation::Claim {
                owner: secret,
                revision: 17,
                target_mask: 1,
                enabled_mask: 0,
                ttl_ms: 5000,
            },
            authority: Some(Arc::new(AtomicBool::new(true))),
            control_epoch: None,
        },
    )
    .unwrap();
    io.change(|s| s.calls.clear());
    let value = open_receipt(&original.receipt, &original.value.owner).unwrap();
    let mut recovered = Overrides {
        slots: vec![Slot {
            value,
            receipt: original.receipt.clone(),
            shared: shared(true),
            spawned: false,
        }],
        io: io.clone(),
        recovery: true,
    };
    stop(&mut recovered);
    assert_eq!(io.count("claim"), 0);
    assert_eq!(io.count("renew"), 0);
    assert_eq!(io.count("release"), 1);
    assert!(io.count("inspect") > 0);
    let mut wrong_owner = original.value.owner.clone();
    wrong_owner.generation += 1;
    assert!(open_receipt(&original.receipt, &wrong_owner).is_err());
    let mut damaged = original.receipt.clone();
    damaged.state["sealedHex"] = json!("00");
    assert!(open_receipt(&damaged, &original.value.owner).is_err());
}

#[test]
fn can_lease_drift_at_due_renewal_revokes_before_any_blocking_renew_or_new_proof() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    struct DriftIo {
        inner: Arc<FakeIo>,
        arm: AtomicBool,
        drifted: AtomicBool,
        renew_entered: AtomicBool,
        release_renew: (Mutex<bool>, std::sync::Condvar),
    }
    impl Io for DriftIo {
        fn cached(&self, gateway: &str) -> Result<Value, String> {
            self.inner.cached(gateway)
        }
        fn request(
            &self,
            gateway: &str,
            request: PublicationRequest,
        ) -> Result<PublicationReply, String> {
            if matches!(&request.operation, PublicationOperation::State)
                && self.arm.swap(false, Ordering::AcqRel)
            {
                // Hold the next State until renewal is due, then expose an
                // independently advanced CAN assignment under the same owner.
                thread::sleep(Duration::from_millis(1100));
                self.inner
                    .change(|s| s.state["modules"][0]["lease"] = json!(999));
                self.drifted.store(true, Ordering::Release);
            }
            if matches!(&request.operation, PublicationOperation::Renew { .. })
                && self.drifted.load(Ordering::Acquire)
            {
                self.renew_entered.store(true, Ordering::Release);
                let (lock, wake) = &self.release_renew;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = wake.wait(released).unwrap();
                }
            }
            self.inner.request(gateway, request)
        }
    }
    let targets = vec![target(0, false)];
    let inner = FakeIo::new(&targets);
    let io = Arc::new(DriftIo {
        inner: inner.clone(),
        arm: AtomicBool::new(false),
        drifted: AtomicBool::new(false),
        renew_entered: AtomicBool::new(false),
        release_renew: (Mutex::new(false), std::sync::Condvar::new()),
    });
    let mut actor = Overrides::prepare_io(&Staged { targets }, &owner(), io.clone()).unwrap();
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    io.arm.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(3);
    while actor.poll().error.is_none()
        && !io.renew_entered.load(Ordering::Acquire)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    let status = actor.poll();
    let entered = io.renew_entered.load(Ordering::Acquire);
    let refused = actor.verified(false).is_err();
    let revoked = !actor.slots[0].shared.authority.load(Ordering::Acquire);
    // Always unblock a regressed dispatcher before asserting, so a failure
    // cannot strand a native worker or its bounded capacity in another test.
    *io.release_renew.0.lock().unwrap() = true;
    io.release_renew.1.notify_all();
    stop(&mut actor);
    assert!(io.drifted.load(Ordering::Acquire));
    assert!(
        !entered,
        "An independently changed CAN lease reached Renew before revocation"
    );
    assert!(
        !status.active && status.error.is_some() && revoked && refused,
        "The new route gained an active private proof before revocation"
    );
    assert_eq!(inner.count("renew"), 0);
    assert_eq!(inner.count("claim"), 1);
}

#[test]
fn expiry_between_state_and_private_inspect_confirms_exact_restoration_without_release() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    struct ExpireIo {
        inner: Arc<FakeIo>,
        expire: AtomicBool,
    }
    impl Io for ExpireIo {
        fn cached(&self, gateway: &str) -> Result<Value, String> {
            self.inner.cached(gateway)
        }
        fn request(
            &self,
            gateway: &str,
            request: PublicationRequest,
        ) -> Result<PublicationReply, String> {
            if matches!(&request.operation, PublicationOperation::Inspect { .. })
                && self.expire.swap(false, Ordering::AcqRel)
            {
                // The independent firmware clock expires after State reported
                // the old candidate, before its correlated owner inspection.
                self.inner.change(|s| {
                    s.owner = None;
                    s.state["publication"]["phase"] = json!("idle");
                    s.state["publication"]["outcome"] = json!("no_change");
                    s.state["publication"]["transition"] = json!("2");
                    s.state["publicationRevision"] = json!(19);
                    for m in s.state["modules"].as_array_mut().unwrap() {
                        m["published"] = m["configuredEnabled"].clone();
                        m["lease"] = json!(68);
                        m["hasInput"] = json!(false);
                    }
                });
            }
            self.inner.request(gateway, request)
        }
    }
    let targets = vec![target(0, false)];
    let inner = FakeIo::new(&targets);
    let staged = prepare(targets, inner.clone());
    let original = &staged.slots[0];
    inner
        .request(
            GATEWAY,
            PublicationRequest {
                epoch: 65536,
                operation: PublicationOperation::Claim {
                    owner: original.value.secret.clone(),
                    revision: 17,
                    target_mask: 1,
                    enabled_mask: 0,
                    ttl_ms: 5000,
                },
                authority: Some(Arc::new(AtomicBool::new(true))),
                control_epoch: None,
            },
        )
        .unwrap();
    inner.change(|s| s.calls.clear());
    let io = Arc::new(ExpireIo {
        inner: inner.clone(),
        expire: AtomicBool::new(true),
    });
    let mut recovered = Overrides {
        slots: vec![Slot {
            value: original.value.clone(),
            receipt: original.receipt.clone(),
            shared: shared(true),
            spawned: false,
        }],
        io,
        recovery: true,
    };
    stop(&mut recovered);
    let status = recovered.poll();
    assert!(
        status.error.is_none(),
        "Exact expiry restoration was mistaken for a foreign owner: {:?}",
        status.error
    );
    recovered.verified(true).unwrap().validate().unwrap();
    assert!(recovered.verified(false).is_err());
    assert!(inner.count("inspect") > 0);
    assert_eq!(inner.count("claim"), 0);
    assert_eq!(inner.count("renew"), 0);
    assert_eq!(inner.count("release"), 0);
}

#[test]
fn timed_out_cleanup_retries_after_transport_returns_without_claim_or_renewal() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    struct OfflineIo {
        inner: Arc<FakeIo>,
        offline: AtomicBool,
    }
    impl Io for OfflineIo {
        fn cached(&self, gateway: &str) -> Result<Value, String> {
            self.inner.cached(gateway)
        }
        fn request(
            &self,
            gateway: &str,
            request: PublicationRequest,
        ) -> Result<PublicationReply, String> {
            if self.offline.load(Ordering::Acquire) {
                return Err("The isolated transport is offline".into());
            }
            self.inner.request(gateway, request)
        }
    }
    let targets = vec![target(0, false)];
    let inner = FakeIo::new(&targets);
    let io = Arc::new(OfflineIo {
        inner: inner.clone(),
        offline: AtomicBool::new(false),
    });
    let mut actor = Overrides::prepare_io(&Staged { targets }, &owner(), io.clone()).unwrap();
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    let receipt = actor.receipts();
    let secret = actor.slots[0].value.secret.clone();
    io.offline.store(true, Ordering::Release);
    actor.stop().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let terminal = loop {
        let status = actor.poll();
        if status.stopped {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "Offline cleanup never reached its bounded terminal state"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(!terminal.active && !terminal.cleanup_complete && terminal.error.is_some());
    assert_eq!(inner.count("claim"), 1);
    assert_eq!(inner.count("renew"), 0);
    assert_eq!(inner.count("release"), 0);
    io.offline.store(false, Ordering::Release);
    stop(&mut actor);
    let final_status = actor.poll();
    assert!(final_status.cleanup_complete && final_status.error.is_none());
    assert_eq!(
        serde_json::to_value(actor.receipts()).unwrap(),
        serde_json::to_value(receipt).unwrap()
    );
    assert_eq!(actor.slots[0].value.secret, secret);
    actor.verified(true).unwrap().validate().unwrap();
    assert_eq!(inner.count("claim"), 1);
    assert_eq!(inner.count("renew"), 0);
    assert_eq!(inner.count("release"), 1);
}

#[test]
fn completed_exact_restoration_proof_does_not_expire_like_active_ownership() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let targets = vec![target(0, false)];
    let io = FakeIo::new(&targets);
    let mut actor = prepare(targets, io.clone());
    actor.acquire().unwrap();
    wait(&actor, |s| s.active);
    stop(&mut actor);
    let shared = &actor.slots[0].shared;
    shared.verified.lock().unwrap().as_mut().unwrap().0 = Instant::now() - Duration::from_secs(30);
    actor.verified(true).unwrap().validate().unwrap();
    assert!(actor.verified(false).is_err());
    io.change(|s| s.state["modules"][0]["lease"] = json!(999));
    assert!(
        actor.verified(true).is_err(),
        "Age exemption must retain the exact restored CAN identity fence"
    );
    assert_eq!(io.count("claim"), 1);
    assert_eq!(io.count("renew"), 0);
    assert_eq!(io.count("release"), 1);
}

mod native_host {
    use super::*;
    use crate::profile_session::{Candidate, Coordinator, State};
    use crate::profile_session_host::NativeProfileHost;
    use osg_profiles::{
        resources::{Observation, ProviderBatch, ResourceType, Route},
        resources_ports::PortUpdate,
    };

    struct NoGames;
    impl crate::game_discovery::Platform for NoGames {
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
            vec![]
        }
        fn validate(&self, _: &str, _: &str) -> Result<(), String> {
            Err("No game belongs to the publication fixture".into())
        }
        fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
            panic!("No game launch")
        }
        fn find(
            &self,
            _: &Value,
        ) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
            panic!("No game discovery")
        }
        fn alive(&self, _: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
            panic!("No game process")
        }
    }
    struct Fixture {
        coordinator: Coordinator<NativeProfileHost>,
        resources: Arc<crate::profile_resources::Runtime>,
        controllers: Arc<osg_controller_input::ControllerService>,
        games: Arc<crate::games::GamesRuntime>,
        companions: Arc<crate::companion_host::Runtime>,
        epoch: Arc<AtomicU64>,
        io: Arc<FakeIo>,
        source: String,
        claims: Arc<Mutex<Vec<Value>>>,
        cancel_claim: Arc<AtomicBool>,
        actions: Option<Arc<action_tests::ActionDevice>>,
        peer_stop: Arc<AtomicBool>,
        peer: Option<thread::JoinHandle<()>>,
        _directory: tempfile::TempDir,
    }
    impl Fixture {
        fn new() -> Self {
            Self::with_actions(false)
        }
        fn with_actions(with_actions: bool) -> Self {
            let directory = crate::test_directory();
            let journal = directory.path().join("profile-session.json");
            let io = FakeIo::new(&[target(0, false)]);
            io.change(|s| s.pending = true);
            let actions = with_actions.then(|| Arc::new(action_tests::ActionDevice::new()));
            let observed = io.clone();
            let observed_actions = actions.clone();
            let collect: crate::profile_resources::Collect = Arc::new(move |revision| {
                let state = action_tests::cached(&observed, observed_actions.as_deref());
                let module = &state["modules"][0];
                let metadata = advertised(&state, module)
                    .expect("The exact native fixture capability remains advertised");
                let item = Observation {
                    key: module["id"].as_str().unwrap().into(),
                    name: "Native CAN controller".into(),
                    resource_type: ResourceType::OsgModule,
                    evidence: vec![],
                    diagnostics: vec![],
                    attributes: BTreeMap::from([
                        ("gatewayId".into(), GATEWAY.into()),
                        ("moduleId".into(), module["id"].as_str().unwrap().into()),
                        ("controllerPublicationOverride".into(), metadata.to_string()),
                    ]),
                    route: Some(Route {
                        id: format!("can:{GATEWAY}:{}", module["id"].as_str().unwrap()),
                        transport: Transport::Can,
                        provider_generation: json!([
                            state["epoch"],
                            module["boot"],
                            module["lease"],
                            module["configurationGeneration"],
                            module["lease"]
                        ])
                        .to_string(),
                        attributes: BTreeMap::from([
                            ("gatewayId".into(), GATEWAY.into()),
                            ("moduleId".into(), module["id"].as_str().unwrap().into()),
                            ("boot".into(), module["boot"].to_string()),
                            ("node".into(), module["node"].to_string()),
                        ]),
                        baseline_ready: true,
                        writable: true,
                        priority: 50,
                    }),
                    ports: Some(
                        PortUpdate::complete(
                            observed_actions
                                .as_ref()
                                .map(|a| a.ports())
                                .unwrap_or_default(),
                        )
                        .unwrap(),
                    ),
                };
                vec![(
                    "publication-can".into(),
                    Ok(ProviderBatch {
                        scope: "publication-can".into(),
                        session: "isolated-native-publication".into(),
                        revision,
                        complete: true,
                        observations: vec![item],
                    }),
                )]
            });
            let resources = Arc::new(crate::profile_resources::Runtime::new(None, collect, true));
            resources.refresh_native().unwrap();
            let source = resources
                .request(&json!({"command":"profile_resources_list"}))
                .unwrap()["resources"][0]["id"]
                .as_str()
                .unwrap()
                .to_owned();
            let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
            let games = Arc::new(crate::games::GamesRuntime::test_runtime(
                Arc::new(|_| panic!("No provider or device command")),
                crate::games::Paths::in_directory(directory.path()),
                Arc::new(NoGames),
            ));
            let companions = Arc::new(crate::companion_host::Runtime::new(
                None,
                Arc::new(osg_mapping_host::companions::CompanionProvider::new(
                    Arc::new(|| 0),
                )),
            ));
            let epoch = Arc::new(AtomicU64::new(1));
            let claims = Arc::new(Mutex::new(vec![]));
            let cancel_claim = Arc::new(AtomicBool::new(false));
            let cached = io.clone();
            let requests = io.clone();
            let observed_claims = claims.clone();
            let cancel = cancel_claim.clone();
            let control = epoch.clone();
            let cached_actions = actions.clone();
            let publication_actions = actions.clone();
            let ordinary_actions = actions.clone();
            let ordinary_io = io.clone();
            let panels = Arc::new(crate::gateways::ProfilePanelIo::publication_action_fixture(
                GATEWAY.into(),
                Arc::new(move || action_tests::cached(&cached, cached_actions.as_deref())),
                Arc::new(move |request| {
                    if matches!(request.operation, PublicationOperation::Release { .. }) {
                        if let Some(actions) = publication_actions.as_ref() {
                            actions.publication_release(&requests);
                        }
                    }
                    if let PublicationOperation::Claim {
                        owner,
                        revision,
                        target_mask,
                        enabled_mask,
                        ..
                    } = &request.operation
                    {
                        // Read the real durable Coordinator journal at the actual
                        // serial admission boundary, before this fixture mutates.
                        let manifest: Value =
                            serde_json::from_slice(&osg_persistence::test_support::read_evidence(&journal,"recovery-profile").unwrap()).unwrap();
                        let incoming = &manifest["pending"]["incoming"];
                        assert_eq!(incoming["attempted"], json!(["overrides"]));
                        let run_owner: RunOwner =
                            serde_json::from_value(incoming["owner"].clone()).unwrap();
                        let receipt: Receipt = serde_json::from_value(
                            incoming["receipts"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .find(|r| r["kind"] == KIND)
                                .unwrap()
                                .clone(),
                        )
                        .unwrap();
                        let sealed = open_receipt(&receipt, &run_owner).unwrap();
                        assert_eq!(&sealed.secret, owner);
                        assert_eq!(sealed.revision, *revision);
                        assert_eq!(sealed.target_mask, *target_mask);
                        assert_eq!(sealed.enabled_mask, *enabled_mask);
                        observed_claims.lock().unwrap().push(json!({"generation":run_owner.generation,"revision":revision,"targets":target_mask,"enabled":enabled_mask}));
                        if cancel.load(Ordering::Acquire) {
                            control.fetch_add(1, Ordering::AcqRel);
                        }
                    }
                    if matches!(
                        request.operation,
                        PublicationOperation::Claim { .. } | PublicationOperation::Renew { .. }
                    ) {
                        let (native, expected) = request.control_epoch.as_ref().expect(
                            "The Host must carry its native cancellation fence to serial admission",
                        );
                        if native.load(Ordering::Acquire) != *expected {
                            return Err("Stop revoked the queued native publication before serial admission".into());
                        }
                    }
                    requests.request(GATEWAY, request)
                }),
                Arc::new(move |request| {
                    ordinary_actions
                        .as_ref()
                        .map(|a| a.request(&ordinary_io, request))
                        .unwrap_or_else(|| json!({"ok":false,"error":"No ordinary action"}))
                }),
            ));
            let host = NativeProfileHost::new(
                resources.clone(),
                controllers.clone(),
                games.clone(),
                companions.clone(),
                panels,
                false,
                epoch.clone(),
            );
            let coordinator =
                Coordinator::new(host, Some(directory.path().join("profile-session.json")));
            let peer_stop = Arc::new(AtomicBool::new(false));
            let stopped = peer_stop.clone();
            let peer_io = io.clone();
            let peer = thread::spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let pending = {
                        let state = peer_io.0.lock().unwrap();
                        state.state["publication"]["phase"] == "owned"
                            && state.state["layoutPending"] == true
                    };
                    if pending {
                        peer_io.settle();
                        peer_io.change(|s| s.state["modules"][0]["hasInput"] = json!(true));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            });
            Self {
                coordinator,
                resources,
                controllers,
                games,
                companions,
                epoch,
                io,
                source,
                claims,
                cancel_claim,
                actions,
                peer_stop,
                peer: Some(peer),
                _directory: directory,
            }
        }
        fn candidate(&self, enabled: Option<bool>) -> Candidate {
            let mut document =
                osg_profiles::new_document("Native publication ownership fixture").unwrap();
            let assignments = if let Some(enabled) = enabled {
                document.resources.push(osg_profiles::Resource {
                    id: "controller-0".into(),
                    name: "Exact CAN controller".into(),
                    kind: osg_profiles::ResourceKind::Device,
                    required: true,
                    ports: vec![],
                });
                document.overrides.push(osg_profiles::RuntimeOverride {
                    resource_id: "controller-0".into(),
                    enabled,
                });
                vec![osg_profiles::store::LocalAssignment {
                    resource_id: "controller-0".into(),
                    source_id: self.source.clone(),
                    ports: vec![],
                }]
            } else {
                vec![]
            };
            let mut candidate = Candidate { launch_preparation: false,
                document,
                assignments,
                read_only: false,
                game_id: None,
                admission_epoch: Some(self.epoch.load(Ordering::Acquire)),
                stage: None,
            };
            candidate.stage = Some(
                NativeProfileHost::stage_with_handoff(
                    &self.resources,
                    &self.games,
                    &self.companions,
                    self.controllers.clone(),
                    false,
                    &candidate,
                    self.coordinator.visibility_handoff_source().unwrap(),
                )
                .unwrap(),
            );
            candidate
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.coordinator.stop();
            self.peer_stop.store(true, Ordering::Release);
            if let Some(peer) = self.peer.take() {
                let _ = peer.join();
            }
            self.resources.shutdown();
            self.games.shutdown();
            self.companions.shutdown();
            self.controllers.shutdown();
        }
    }
    #[test]
    fn actual_host_coordinator_journals_before_claim_and_recaptures_after_override_default_stop() {
        let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
        let mut f = Fixture::new();
        let first = f.candidate(Some(false));
        f.coordinator.apply(first).unwrap();
        assert_eq!(f.coordinator.status().state, State::Running);
        assert_eq!(f.io.count("claim"), 1);
        assert_eq!(
            f.io.cached(GATEWAY).unwrap()["modules"][0]["published"],
            false
        );
        let revision_before_restore = f.io.cached(GATEWAY).unwrap()["publicationRevision"]
            .as_u64()
            .unwrap();
        let second = f.candidate(Some(true));
        f.coordinator.apply(second).unwrap();
        assert_eq!(f.coordinator.status().state, State::Running);
        let claims = f.claims.lock().unwrap();
        assert_eq!(claims.len(), 2);
        assert!(
            claims[1]["revision"].as_u64().unwrap() > revision_before_restore,
            "Incoming revision must be resealed after outgoing restoration"
        );
        assert_ne!(claims[0]["generation"], claims[1]["generation"]);
        drop(claims);
        assert_eq!(f.io.count("claim"), 2);
        assert_eq!(f.io.count("release"), 1);
        let default = f.candidate(None);
        f.coordinator.apply(default).unwrap();
        assert_eq!(f.coordinator.status().state, State::Running);
        assert_eq!(f.io.count("claim"), 2);
        assert_eq!(f.io.count("release"), 2);
        assert_eq!(
            f.io.cached(GATEWAY).unwrap()["modules"][0]["published"],
            true
        );
        f.coordinator.stop().unwrap();
        assert_eq!(f.coordinator.status().state, State::Stopped);
        assert_eq!(f.io.count("claim"), 2);
        assert_eq!(f.io.count("release"), 2);
    }
    #[test]
    fn actual_host_stop_epoch_fences_a_claim_queued_after_durable_intent() {
        let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
        let mut f = Fixture::new();
        f.cancel_claim.store(true, Ordering::Release);
        let candidate = f.candidate(Some(false));
        assert!(f.coordinator.apply(candidate).is_err());
        assert_eq!(
            f.claims.lock().unwrap().len(),
            1,
            "The queued request observed its durable sealed attempted phase"
        );
        assert_eq!(f.io.count("claim"), 0);
        assert_eq!(f.io.count("renew"), 0);
        assert_eq!(f.io.count("release"), 0);
        assert_eq!(
            f.io.cached(GATEWAY).unwrap()["publication"]["phase"],
            "idle"
        );
        assert_eq!(
            f.io.cached(GATEWAY).unwrap()["modules"][0]["published"],
            true
        );
        assert_ne!(f.coordinator.status().state, State::Running);
    }
    #[test]
    fn actual_host_rebinds_the_same_physical_ports_across_different_profile_resource_ids() {
        let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
        for keep_override in [true, false] {
            let mut f = Fixture::with_actions(true);
            let first = f.action_candidate();
            f.coordinator.apply(first).unwrap();
            assert_eq!(f.coordinator.status().state, State::Running);
            let physical = f.source.clone();
            let mut incoming = f.action_candidate();
            for resource in &mut incoming.document.resources {
                if resource.id == "controller-0" {
                    resource.id = "incoming-controller".into();
                }
            }
            for intent in &mut incoming.document.overrides {
                if intent.resource_id == "controller-0" {
                    intent.resource_id = "incoming-controller".into();
                }
            }
            for assignment in &mut incoming.assignments {
                if assignment.resource_id == "controller-0" {
                    assignment.resource_id = "incoming-controller".into();
                }
            }
            for binding in &mut incoming.document.bindings {
                if binding.from.node_id == "controller-0" {
                    binding.from.node_id = "incoming-controller".into();
                }
                if binding.to.node_id == "controller-0" {
                    binding.to.node_id = "incoming-controller".into();
                }
            }
            if !keep_override {
                incoming.document.overrides.clear();
            }
            assert_eq!(incoming.assignments[0].source_id, physical);
            incoming.stage = None;
            incoming.stage = Some(
                NativeProfileHost::stage_with_handoff(
                    &f.resources,
                    &f.games,
                    &f.companions,
                    f.controllers.clone(),
                    false,
                    &incoming,
                    f.coordinator.visibility_handoff_source().unwrap(),
                )
                .unwrap(),
            );
            f.coordinator.apply(incoming).unwrap();
            assert_eq!(
                f.coordinator.status().state,
                State::Running,
                "keep_override={keep_override}"
            );
            assert_eq!(f.io.count("claim"), if keep_override { 2 } else { 1 });
            assert_eq!(
                f.io.cached(GATEWAY).unwrap()["modules"][0]["published"],
                !keep_override
            );
            f.coordinator.stop().unwrap();
            assert_eq!(f.coordinator.status().state, State::Stopped);
        }
    }
    mod action_tests {
        include!("profile_publication_action_tests.rs");
    }
}

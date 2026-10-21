//! Bounded native physical output actors. Host calls only cached/nonblocking
//! methods; existing panel workers own every serial operation and firmware lease.
use crate::{
    gateways::ProfilePanelIo,
    profile_resources::Runtime,
    profile_session::{OwnedRun, Receipt, RunOwner},
    profile_session_action_lease::{Lease, Released, MAX_ENDPOINTS},
};
use osg_mapping::{
    engine::{typed::Effect, ReportReason},
    program::{NamedArgument, SignalSample, SignalValue},
    signal::{ActionSemantics, PortContract},
};
use osg_profiles::compiler_models::{ExecutionPlan, PlanReadiness, SinkAdapter, SinkPlan};
use osg_profiles::resources::ResourceToken;
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
const QUEUE: usize = 64;
const RENEW: Duration = Duration::from_secs(1);
const WAIT_FOR_BIND: Duration = Duration::from_secs(3);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Permit {
    fn reserve(count: usize) -> Result<Vec<Self>, String> {
        WORKERS.fetch_update(Ordering::AcqRel,Ordering::Acquire,|n|n.checked_add(count).filter(|next|*next<=MAX_ENDPOINTS)).map_err(|_|"Physical action worker capacity is still owned by an outgoing run; finish its cleanup first.")?;
        Ok((0..count).map(|_| Self).collect())
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}
trait Driver: Send {
    fn receipt(&self) -> Receipt;
    fn key(&self) -> String;
    fn claim(&mut self) -> Result<(), String>;
    fn bind(&mut self, sink: &SinkPlan) -> Result<(), String>;
    fn invoke(&mut self, owner: &RunOwner, args: &[NamedArgument]) -> Result<(), String>;
    fn renew(&mut self, owner: &RunOwner) -> Result<(), String>;
    fn release(&mut self) -> Result<Released, String>;
}
impl Driver for Lease {
    fn receipt(&self) -> Receipt {
        self.receipt()
    }
    fn key(&self) -> String {
        self.endpoint_key()
    }
    fn claim(&mut self) -> Result<(), String> {
        self.claim()
    }
    fn bind(&mut self, sink: &SinkPlan) -> Result<(), String> {
        self.bind_delivery(sink)
    }
    fn invoke(&mut self, o: &RunOwner, a: &[NamedArgument]) -> Result<(), String> {
        self.invoke(o, a)
    }
    fn renew(&mut self, o: &RunOwner) -> Result<(), String> {
        self.renew(o)
    }
    fn release(&mut self) -> Result<Released, String> {
        self.release()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Dormant,
    Prepared,
    Acquiring,
    Claimed,
    Bound,
    Stopped,
}
struct State {
    stage: Stage,
    queue: VecDeque<Effect>,
    bind: Option<SinkPlan>,
    error: Option<String>,
    cleanup_done: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            stage: Stage::Prepared,
            queue: VecDeque::new(),
            bind: None,
            error: None,
            cleanup_done: false,
        }
    }
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    revoked: AtomicBool,
}
impl Shared {
    fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        s.queue.clear();
        s.bind = None;
        self.wake.notify_all();
    }
    fn fail(&self, error: String) {
        self.revoke();
        self.state.lock().unwrap_or_else(|p| p.into_inner()).error = Some(error);
    }
}
struct Slot {
    endpoint: Option<(String, String)>,
    sink_id: String,
    required: bool,
    dormant: bool,
    one_shot: bool,
    contract: Option<PortContract>,
    receipt: Receipt,
    driver: Option<Box<dyn Driver>>,
    shared: Arc<Shared>,
    bind_queued: bool,
    requested_token: Option<ResourceToken>,
    sequence: u64,
    at: u64,
}
#[derive(Default)]
pub(crate) struct ActionPoll {
    pub acquired: bool,
    pub bound: bool,
    pub stopped: bool,
    pub cleanup_complete: bool,
    pub required_failed: bool,
    pub diagnostics: Vec<String>,
}
pub(crate) struct PanelActions {
    slots: Vec<Slot>,
    owner: RunOwner,
    unavailable: Vec<String>,
    program: Vec<u8>,
    cancelled: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    acquired: bool,
    recovery: bool,
}
fn endpoint(sink: &SinkPlan) -> (String, String) {
    (sink.endpoint.node_id.clone(), sink.endpoint.port_id.clone())
}
impl PanelActions {
    pub(crate) fn prepare(
        plan: &ExecutionPlan,
        owner: &RunOwner,
        resources: Arc<Runtime>,
        io: Arc<ProfilePanelIo>,
    ) -> Result<Self, String> {
        Self::prepare_with(plan, owner, |sink| {
            Lease::prepare(sink, owner, resources.clone(), io.clone())
                .map(|d| Box::new(d) as Box<dyn Driver>)
        })
    }
    fn prepare_with(
        plan: &ExecutionPlan,
        owner: &RunOwner,
        mut factory: impl FnMut(&SinkPlan) -> Result<Box<dyn Driver>, String>,
    ) -> Result<Self, String> {
        let mut slots = vec![];
        let mut keys = BTreeSet::new();
        let mut endpoints = BTreeSet::new();
        let mut unavailable = vec![];
        for sink in &plan.sinks {
            if !matches!(
                sink.adapter,
                SinkAdapter::PhysicalAction | SinkAdapter::PhysicalState
            ) {
                continue;
            }
            let branch = plan
                .branches
                .iter()
                .find(|b| b.sink == sink.endpoint)
                .ok_or("Physical action sink has no exact compiled branch.")?;
            if sink.readiness == PlanReadiness::Unavailable || branch.readiness == PlanReadiness::Unavailable {
                unavailable.push(format!("{} is offline; its physical output is inactive.", sink.endpoint.node_id));
                continue;
            }
            let mut make = || {
                if sink.adapter != SinkAdapter::PhysicalAction
                    || sink.readiness != PlanReadiness::Ready
                {
                    return Err(
                        "This physical output has no ready advertised action adapter.".into(),
                    );
                }
                factory(sink)
            };
            let driver = match make() {
                Ok(d) => d,
                Err(e) if !branch.required => {
                    unavailable.push(format!("{}: {e}", sink.endpoint.node_id));
                    continue;
                }
                Err(e) => return Err(e),
            };
            let logical = endpoint(sink);
            if !keys.insert(driver.key()) || !endpoints.insert(logical.clone()) {
                return Err(
                    "Several physical sinks alias the same native endpoint; select one owner.",
                )?;
            }
            let selected = plan
                .program
                .sinks
                .iter()
                .filter(|s| s.endpoint.node_id == logical.0 && s.endpoint.port_id == logical.1)
                .collect::<Vec<_>>();
            if selected.len() != 1 {
                return Err("Physical sink must match one exact immutable program output.".into());
            }
            let one_shot = matches!(
                &sink.contract,
                PortContract::Action {
                    semantics: ActionSemantics::OneShot,
                    ..
                }
            );
            slots.push(Slot {
                endpoint: Some(logical),
                sink_id: selected[0].id.clone(),
                required: branch.required,
                dormant: branch.readiness == PlanReadiness::Dormant,
                one_shot,
                contract: Some(sink.contract.clone()),
                receipt: driver.receipt(),
                driver: Some(driver),
                shared: Arc::new(Shared {
                    state: Mutex::new(State {
                        stage: if branch.readiness == PlanReadiness::Dormant {
                            Stage::Dormant
                        } else {
                            Stage::Prepared
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                bind_queued: false,
                requested_token: None,
                sequence: 0,
                at: 0,
            });
        }
        if slots.len() > MAX_ENDPOINTS {
            return Err("Physical action endpoint budget exceeded.".into());
        }
        Ok(Self {
            slots,
            owner: owner.clone(),
            unavailable,
            program: serde_json::to_vec(&plan.program).map_err(|e| e.to_string())?,
            cancelled: Arc::default(),
            active: Arc::default(),
            acquired: false,
            recovery: false,
        })
    }
    pub(crate) fn recover(
        run: &OwnedRun,
        resources: Arc<Runtime>,
        io: Arc<ProfilePanelIo>,
    ) -> Result<Self, String> {
        let mut slots = vec![];
        let mut keys = BTreeSet::new();
        for r in run
            .receipts
            .iter()
            .filter(|r| r.kind == "physical-action-lease")
        {
            let driver = Lease::recover(r, &run.owner, resources.clone(), io.clone())?;
            if !keys.insert(driver.endpoint_key()) {
                return Err("Duplicate physical cleanup receipt.".into());
            }
            slots.push(Slot {
                endpoint: None,
                sink_id: String::new(),
                required: true,
                dormant: false,
                one_shot: false,
                contract: None,
                receipt: r.clone(),
                driver: Some(Box::new(driver)),
                shared: Arc::default(),
                bind_queued: false,
                requested_token: None,
                sequence: 0,
                at: 0,
            });
        }
        if slots.len() > MAX_ENDPOINTS {
            return Err("Physical cleanup receipt budget exceeded.".into());
        }
        Ok(Self {
            slots,
            owner: run.owner.clone(),
            unavailable: vec![],
            program: vec![],
            cancelled: Arc::default(),
            active: Arc::default(),
            acquired: false,
            recovery: true,
        })
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    pub(crate) fn optional_unavailable(&self) -> &[String] {
        &self.unavailable
    }
    pub(crate) fn owns_endpoint(&self, endpoint: &osg_profiles::Endpoint) -> bool {
        self.slots.iter().any(|s| {
            s.endpoint.as_ref() == Some(&(endpoint.node_id.clone(), endpoint.port_id.clone()))
        })
    }
    pub(crate) fn capture_receipts(&self) -> Vec<Receipt> {
        self.slots.iter().map(|s| s.receipt.clone()).collect()
    }
    /// One worker per ready endpoint; anticipated dormant endpoints already
    /// have sealed receipts but cannot acquire output until explicit admission.
    pub(crate) fn acquire(&mut self) -> Result<(), String> {
        if self.acquired || self.cancelled.load(Ordering::Acquire) {
            return Err("Prepare a fresh physical action owner before acquisition.".into());
        }
        let indices = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| (!s.dormant).then_some(i))
            .collect();
        self.spawn_indices(indices)
    }
    /// Launch-context admission only, before Start. Never creates a driver,
    /// receipt or endpoint, and never reclaims a failed/retired lease.
    pub(crate) fn acquire_ready(&mut self, plan: &ExecutionPlan) -> Result<(), String> {
        if !self.acquired
            || self.recovery
            || self.active.load(Ordering::Acquire)
            || self.cancelled.load(Ordering::Acquire)
            || serde_json::to_vec(&plan.program).map_err(|e| e.to_string())? != self.program
        {
            return Err("Late physical output admission requires the unchanged journaled program before Start; prepare a fresh Apply.".into());
        }
        let mut indices = vec![];
        for (i, slot) in self.slots.iter().enumerate() {
            if !slot.dormant {
                continue;
            }
            if slot.shared.revoked.load(Ordering::Acquire) {
                if !slot.required {
                    continue;
                }
                return Err(
                    "A retired physical endpoint cannot be reclaimed by late launch admission."
                        .into(),
                );
            }
            let branch = plan
                .branches
                .iter()
                .find(|b| Some((b.sink.node_id.clone(), b.sink.port_id.clone())) == slot.endpoint)
                .ok_or("The late physical output branch changed; prepare a fresh Apply.")?;
            if branch.readiness == PlanReadiness::Ready {
                indices.push(i);
            } else if !matches!(branch.readiness, PlanReadiness::Dormant | PlanReadiness::Unavailable) && branch.required {
                return Err("The anticipated required physical output is unavailable; prepare a fresh Apply.".into());
            }
        }
        self.spawn_indices(indices)
    }
    /// Permanently withdraw an optional endpoint whose native delivery proof
    /// was lost before Start. Existing workers clean up; inert slots never claim.
    pub(crate) fn revoke_optional(
        &mut self,
        endpoint: &osg_profiles::Endpoint,
        diagnostic: String,
    ) -> Result<(), String> {
        if self.recovery
            || self.active.load(Ordering::Acquire)
            || self.cancelled.load(Ordering::Acquire)
        {
            return Err(
                "Optional acquisition can be withdrawn only before physical output Start.".into(),
            );
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| {
                slot.endpoint.as_ref()
                    == Some(&(endpoint.node_id.clone(), endpoint.port_id.clone()))
            })
            .ok_or("The optional physical endpoint was not sealed for this run.")?;
        if slot.required {
            return Err("A required physical endpoint must block admission when its native delivery proof is lost.".into());
        }
        slot.shared.fail(diagnostic);
        if slot.driver.is_some() {
            let mut state = slot.shared.state.lock().unwrap_or_else(|p| p.into_inner());
            state.stage = Stage::Stopped;
            state.cleanup_done = true;
        }
        Ok(())
    }
    fn spawn_indices(&mut self, indices: Vec<usize>) -> Result<(), String> {
        let permits = Permit::reserve(indices.len())?;
        self.acquired = true;
        let mut spawn_failed = false;
        for (index, permit) in indices.into_iter().zip(permits) {
            let slot = &mut self.slots[index];
            let driver = slot
                .driver
                .take()
                .ok_or("Physical action worker was already consumed.")?;
            slot.dormant = false;
            slot.shared
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .stage = Stage::Acquiring;
            let shared = slot.shared.clone();
            let cancelled = self.cancelled.clone();
            let active = self.active.clone();
            let owner = self.owner.clone();
            let recovery = self.recovery;
            if thread::Builder::new()
                .name("profile-owned-action".into())
                .spawn(move || worker(driver, shared, cancelled, active, owner, recovery, permit))
                .is_err()
            {
                self.cancelled.store(true, Ordering::Release);
                slot.shared
                    .fail("Cannot start the bounded native physical output worker.".into());
                let mut state = slot.shared.state.lock().unwrap_or_else(|p| p.into_inner());
                state.stage = Stage::Stopped;
                state.cleanup_done = true;
                spawn_failed = true;
                break;
            }
        }
        if spawn_failed {
            for slot in &self.slots {
                if slot.driver.is_some() {
                    let mut state = slot.shared.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.stage = Stage::Stopped;
                    state.cleanup_done = true;
                }
            }
            return Err("Cannot start the bounded native physical output worker.".into());
        }
        Ok(())
    }
    pub(crate) fn bind_delivery(&mut self, plan: &ExecutionPlan) -> Result<bool, String> {
        if !self.acquired
            || self.recovery
            || self.cancelled.load(Ordering::Acquire)
            || self.active.load(Ordering::Acquire)
        {
            return Err(
                "Physical output delivery can bind only its acquired owner before Start.".into(),
            );
        }
        for slot in &mut self.slots {
            if slot.dormant {
                continue;
            }
            let mut state = slot.shared.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.error.is_some() || slot.shared.revoked.load(Ordering::Acquire) {
                if slot.required {
                    return Err("A required physical output failed during acquisition.".into());
                }
                continue;
            }
            let matches = plan
                .sinks
                .iter()
                .filter(|sink| Some(endpoint(sink)) == slot.endpoint)
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err("Physical delivery plan changed its selected endpoint.".into());
            }
            let sink = matches[0];
            let first = !slot.bind_queued && state.stage == Stage::Claimed;
            let refresh = slot.bind_queued
                && state.stage == Stage::Bound
                && slot.requested_token != sink.token;
            if first || refresh {
                // Renew the same exact lease after a real native token refresh.
                // This cannot claim/reclaim an endpoint or run after activation.
                state.stage = Stage::Claimed;
                state.bind = Some(sink.clone());
                slot.bind_queued = true;
                slot.requested_token = sink.token.clone();
                slot.shared.wake.notify_all();
            }
        }
        Ok(self.poll().bound)
    }
    pub(crate) fn start(&mut self) -> Result<(), String> {
        if !self.acquired
            || self.recovery
            || self.cancelled.load(Ordering::Acquire)
            || self.active.load(Ordering::Acquire)
            || !self.poll().bound
        {
            return Err(
                "Physical output requires acquired and freshly bound native delivery before Start."
                    .into(),
            );
        }
        self.active.store(true, Ordering::Release);
        for slot in &self.slots {
            slot.shared.wake.notify_all();
        }
        Ok(())
    }
    /// Pre-revoke every known loss before admitting any tuple from this batch.
    pub(crate) fn dispatch(&mut self, effects: &[Effect]) -> Result<(), String> {
        if !self.active.load(Ordering::Acquire) || self.cancelled.load(Ordering::Acquire) {
            return Err("Physical output delivery is not active.".into());
        }
        for effect in effects {
            if effect.run.session != self.owner.session
                || effect.run.generation.0 != self.owner.generation
            {
                return Err("Discarded physical output from a retired native run.".into());
            }
            if let Some(slot) = self.slots.iter().find(|s| {
                s.endpoint.as_ref()
                    == Some(&(
                        effect.endpoint.node_id.clone(),
                        effect.endpoint.port_id.clone(),
                    ))
            }) {
                if slot.dormant {
                    continue;
                }
                if matches!(
                    effect.reason,
                    ReportReason::SourceLost | ReportReason::Stop | ReportReason::Overflow
                ) || !matches!(
                    &effect.sample,
                    SignalSample::Known(SignalValue::ActionPack(_))
                ) && !matches!(effect.reason, ReportReason::Start | ReportReason::Baseline)
                {
                    slot.shared.revoke();
                }
            }
        }
        for effect in effects {
            let Some(slot) = self.slots.iter_mut().find(|s| {
                s.endpoint.as_ref()
                    == Some(&(
                        effect.endpoint.node_id.clone(),
                        effect.endpoint.port_id.clone(),
                    ))
            }) else {
                continue;
            };
            if slot.dormant {
                if matches!(
                    &effect.sample,
                    SignalSample::Known(SignalValue::ActionPack(_))
                ) {
                    return Err("The physical endpoint is dormant; fresh native launch admission is required before Start.".into());
                }
                continue;
            }
            if slot.shared.revoked.load(Ordering::Acquire) {
                continue;
            }
            let SignalSample::Known(SignalValue::ActionPack(pack)) = &effect.sample else {
                continue;
            };
            let bounded = matches!(&slot.contract,Some(PortContract::Action{parameters,..})
                if parameters.len()<=4 && parameters.len()==pack.arguments.len()
                && parameters.iter().zip(&pack.arguments).all(|(p,a)|p.id==a.id&&p.schema.accepts(&a.value)));
            if !bounded
                || effect.sink_id != slot.sink_id
                || effect.sequence.0 <= slot.sequence
                || effect.at_us.0 < slot.at
                || slot.one_shot
                    && matches!(
                        effect.reason,
                        ReportReason::Start
                            | ReportReason::Baseline
                            | ReportReason::SourceLost
                            | ReportReason::Stop
                            | ReportReason::Overflow
                    )
            {
                slot.shared.fail(
                    "Discarded an invalid, retired or baseline one-shot physical output.".into(),
                );
                return Err("Invalid physical output effect; the endpoint was revoked.".into());
            }
            let mut state = slot.shared.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.queue.len() >= QUEUE {
                drop(state);
                slot.shared
                    .fail("Physical action queue overflow; pending tuples were discarded.".into());
                return Err("Physical action queue overflow; the endpoint was revoked.".into());
            }
            slot.sequence = effect.sequence.0;
            slot.at = effect.at_us.0;
            state.queue.push_back(effect.clone());
            slot.shared.wake.notify_all();
        }
        Ok(())
    }
    pub(crate) fn poll(&self) -> ActionPoll {
        let mut result = ActionPoll {
            acquired: self.acquired,
            bound: self.acquired,
            stopped: self.acquired,
            cleanup_complete: self.acquired,
            ..Default::default()
        };
        for slot in &self.slots {
            let s = slot.shared.state.lock().unwrap_or_else(|p| p.into_inner());
            let failed = s.error.is_some() || slot.shared.revoked.load(Ordering::Acquire);
            result.acquired &= matches!(s.stage, Stage::Dormant | Stage::Claimed | Stage::Bound)
                || !slot.required && failed;
            result.bound &=
                matches!(s.stage, Stage::Dormant | Stage::Bound) || !slot.required && failed;
            result.stopped &= s.stage == Stage::Stopped;
            result.cleanup_complete &= s.stage == Stage::Stopped && s.cleanup_done;
            result.required_failed |= slot.required && failed;
            if let Some(e) = &s.error {
                result
                    .diagnostics
                    .push(format!("{}: {e}", slot.receipt.resource_id));
            }
        }
        result
    }
    /// Every dispatched driver is moved into its worker. A retained driver
    /// proves that this preparation never reached an action I/O worker, even
    /// after stop() has marked an inert preparation acquired for shutdown.
    pub(crate) fn is_unstarted_preparation(&self) -> bool {
        !self.recovery && self.slots.iter().all(|slot| slot.driver.is_some())
    }
    pub(crate) fn stop(&mut self) -> Result<ActionPoll, String> {
        self.active.store(false, Ordering::Release);
        // Recovery workers never enter the acquisition/claim branch.
        if self.recovery && !self.acquired {
            self.acquire()?;
        }
        self.cancelled.store(true, Ordering::Release);
        for s in &self.slots {
            s.shared.revoke();
        }
        for slot in &self.slots {
            if slot.driver.is_some() {
                let mut state = slot.shared.state.lock().unwrap_or_else(|p| p.into_inner());
                state.stage = Stage::Stopped;
                state.cleanup_done = true;
            }
        }
        self.acquired = true;
        Ok(self.poll())
    }
}
impl Drop for PanelActions {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        self.cancelled.store(true, Ordering::Release);
        for s in &self.slots {
            s.shared.revoke();
        }
    }
}
fn worker(
    mut driver: Box<dyn Driver>,
    shared: Arc<Shared>,
    cancelled: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    owner: RunOwner,
    recovery: bool,
    _permit: Permit,
) {
    let stopped = || cancelled.load(Ordering::Acquire) || shared.revoked.load(Ordering::Acquire);
    let mut bound = false;
    let claim_at = Instant::now();
    let mut renew_at = claim_at + RENEW;
    let run = (|| {
        if recovery || stopped() {
            return Ok(());
        }
        driver.claim()?;
        if stopped() {
            return Ok(());
        }
        shared.state.lock().unwrap_or_else(|p| p.into_inner()).stage = Stage::Claimed;
        loop {
            if stopped() {
                break;
            }
            let bind = shared
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .bind
                .take();
            if let Some(sink) = bind {
                driver.bind(&sink)?;
                if stopped() {
                    break;
                }
                driver.renew(&owner)?;
                if stopped() {
                    break;
                }
                bound = true;
                renew_at = Instant::now() + RENEW;
                shared.state.lock().unwrap_or_else(|p| p.into_inner()).stage = Stage::Bound;
            }
            if !bound && claim_at.elapsed() >= WAIT_FOR_BIND {
                return Err("Physical output delivery did not acquire its fresh native binding before the bounded lease expired.".into());
            }
            if bound && Instant::now() >= renew_at {
                driver.renew(&owner)?;
                renew_at = Instant::now() + RENEW;
                if stopped() {
                    break;
                }
            }
            let next = if bound && active.load(Ordering::Acquire) {
                shared
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .queue
                    .pop_front()
            } else {
                None
            };
            if let Some(effect) = next {
                if stopped() {
                    break;
                }
                let SignalSample::Known(SignalValue::ActionPack(pack)) = effect.sample else {
                    return Err("Invalid queued physical action tuple.".into());
                };
                driver.invoke(&owner, &pack.arguments)?;
            } else {
                let state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
                let _ = shared
                    .wake
                    .wait_timeout(state, Duration::from_millis(20))
                    .unwrap_or_else(|p| p.into_inner());
            }
        }
        Ok(())
    })();
    if let Err(e) = run {
        shared.fail(e);
    }
    shared.revoke();
    let cleanup = driver.release();
    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
    state.stage = Stage::Stopped;
    state.cleanup_done = cleanup.is_ok();
    if let Err(e) = cleanup {
        state.error = Some(format!("Physical output cleanup remains unresolved: {e}"));
    }
}
#[cfg(test)]
#[path = "profile_session_actions_tests.rs"]
pub(crate) mod tests;

//! Profile-owned use of the existing visibility worker and compare-fenced journal.
//! Receipts reference a native owner; recovery never carries an Apply policy.
use crate::profile_session::{OwnedRun, Receipt, RunOwner};
use osg_controller_input::visibility::{handoff::Proof, policy::Plan, settings::Intent};
use osg_profiles::{
    compiler_models::{CompileSnapshot, ExecutionPlan, PlanReadiness},
    resources::{Availability, ResourceType, Transport},
    VisibilityMode,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const KIND: &str = "controller-visibility";
const RESOURCE: &str = "controller-visibility";
pub(crate) trait VisibilityIo: Send + Sync {
    fn request(&self, operation: &str, value: &Value) -> Result<Value, String>;
}
impl VisibilityIo for osg_controller_input::ControllerService {
    fn request(&self, operation: &str, value: &Value) -> Result<Value, String> {
        self.game_visibility(operation, value)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CapturedIntent {
    mode: String,
    source_ids: Vec<String>,
    policies: BTreeMap<String, Intent>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preparation {
    intent: CapturedIntent,
    policy: Option<Plan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    handoff: Option<Proof>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryReceipt {
    schema_version: u32,
    owner: RunOwner,
    visibility_owner: String,
    source_ids: Vec<String>,
    preparation_hash: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    Pending,
    Active,
    Restored,
}
/// The native service owns asynchronous application, monitoring and cleanup.
/// This adapter only pins the profile owner and checks the exact worker result.
pub(crate) struct Visibility {
    io: Arc<dyn VisibilityIo>,
    receipt: Option<RecoveryReceipt>,
    prepared: Option<Value>,
    attempted: bool,
    stopping: bool,
    restored: bool,
    job: Option<String>,
    session: Option<String>,
    handoff: Option<Outgoing>,
}
/// An unowned native capture. Binding a coordinator owner performs no I/O.
pub(crate) struct StagedVisibility {
    io: Arc<dyn VisibilityIo>,
    captured: Option<(Vec<String>, String, Value)>,
    handoff: Option<Outgoing>,
}
struct Outgoing {
    receipt: RecoveryReceipt,
    proof: Proof,
}
impl StagedVisibility {
    pub(crate) fn bind(self, owner: &RunOwner) -> Result<Visibility, String> {
        if !owner_valid(owner) {
            return Err("Invalid profile visibility owner.".into());
        }
        let (receipt, prepared) = match self.captured {
            Some((source_ids, preparation_hash, prepared)) => (
                Some(RecoveryReceipt {
                    schema_version: 1,
                    owner: owner.clone(),
                    visibility_owner: uuid::Uuid::new_v4().to_string(),
                    source_ids,
                    preparation_hash,
                }),
                Some(prepared),
            ),
            None => (None, None),
        };
        Ok(Visibility {
            io: self.io,
            receipt,
            prepared,
            attempted: false,
            stopping: false,
            restored: false,
            job: None,
            session: None,
            handoff: self.handoff,
        })
    }
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .ok()
        .map(|id| id.to_string())
        .as_deref()
        == Some(value)
}
fn owner_valid(owner: &RunOwner) -> bool {
    uuid(&owner.session)
        && uuid(&owner.profile_id)
        && owner.generation > 0
        && owner.profile_revision > 0
        && owner.profile_revision <= 9_007_199_254_740_991
}
fn hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    })
}
impl RecoveryReceipt {
    fn validate(&self, owner: &RunOwner) -> Result<(), String> {
        if self.schema_version != 1
            || &self.owner != owner
            || !owner_valid(owner)
            || !uuid(&self.visibility_owner)
            || !hash(&self.preparation_hash)
            || self.source_ids.is_empty()
            || self.source_ids.len() > 16
            || self.source_ids.iter().any(|id| !uuid(id))
            || self.source_ids.iter().collect::<BTreeSet<_>>().len() != self.source_ids.len()
        {
            return Err(
                "The native visibility receipt does not match this exact profile owner.".into(),
            );
        }
        Ok(())
    }
}
fn reference(run: &OwnedRun) -> Result<Option<RecoveryReceipt>, String> {
    let receipts = run
        .receipts
        .iter()
        .filter(|r| r.kind == KIND)
        .collect::<Vec<_>>();
    if receipts.is_empty() {
        return Ok(None);
    }
    if receipts.len() != 1
        || receipts[0].resource_id != RESOURCE
        || serde_json::to_vec(&receipts[0].state)
            .map_err(|e| e.to_string())?
            .len()
            > 16_384
    {
        return Err("Malformed or duplicated profile visibility receipt.".into());
    }
    let receipt: RecoveryReceipt = serde_json::from_value(receipts[0].state.clone())
        .map_err(|_| "Malformed profile visibility recovery reference.")?;
    receipt.validate(&run.owner)?;
    Ok(Some(receipt))
}
impl Visibility {
    pub(crate) fn stage(
        plan: &ExecutionPlan,
        snapshot: &CompileSnapshot,
        io: Arc<dyn VisibilityIo>,
    ) -> Result<StagedVisibility, String> {
        Self::stage_with_handoff(plan, snapshot, io, None)
    }
    pub(crate) fn stage_with_handoff(
        plan: &ExecutionPlan,
        snapshot: &CompileSnapshot,
        io: Arc<dyn VisibilityIo>,
        outgoing: Option<&OwnedRun>,
    ) -> Result<StagedVisibility, String> {
        let mut selected = BTreeMap::new();
        let mut logical = BTreeSet::new();
        for intent in &plan.visibility {
            if !logical.insert(&intent.resource_id) {
                return Err("Duplicate profile visibility selection.".into());
            }
            if intent.mode == VisibilityMode::Visible {
                continue;
            }
            if intent.readiness == PlanReadiness::Unavailable { continue; }
            if intent.readiness != PlanReadiness::Ready
                || !snapshot.backend.visibility_available
                || !snapshot.backend.visibility_qualified
            {
                return Err("Profile visibility is unavailable or unqualified; no hiding policy was changed.".into());
            }
            let resource = snapshot
                .resources
                .iter()
                .find(|r| Some(&r.summary.id) == intent.inventory_resource_id.as_ref())
                .ok_or("The hidden device has no exact native inventory assignment.")?;
            if !snapshot
                .assignments
                .iter()
                .any(|a| a.resource_id == intent.resource_id && a.source_id == resource.summary.id)
                || !matches!(
                    resource.summary.resource_type,
                    ResourceType::HostController | ResourceType::OsgModule
                )
                || resource.summary.availability != Availability::Online
            {
                return Err("Visibility requires a current assigned physical controller.".into());
            }
            let routes = resource
                .summary
                .connections
                .iter()
                .filter(|c| c.route.transport == Transport::Hid)
                .collect::<Vec<_>>();
            if routes.len() != 1 {
                return Err(
                    "Select one exact physical HID instance for each visibility intent.".into(),
                );
            }
            let route = routes[0];
            if route.scope != "host-controllers"
                || !uuid(&route.key)
                || route.route.id != format!("hid:{}", route.key)
                || !route.route.baseline_ready
                || selected
                    .insert(route.key.clone(), intent.resource_id.clone())
                    .is_some()
            {
                return Err(
                    "The physical visibility identity is ambiguous or lacks a current baseline."
                        .into(),
                );
            }
        }
        let mut result = StagedVisibility {
            io,
            captured: None,
            handoff: None,
        };
        if selected.is_empty() {
            return Ok(result);
        }
        if selected.len() > 16 {
            return Err(
                "One visibility session can select at most sixteen physical controllers.".into(),
            );
        }
        let ids = selected.into_keys().collect::<Vec<_>>();
        // Capture saved explicit local scope; generic portable intent never
        // invents HideAll or substitutes Unix suppression for HidHide.
        let captured = result.io.request(
            "capture",
            &json!({"mode":"hidhide-session","sourceIds":ids}),
        )?;
        if serde_json::to_vec(&captured)
            .map_err(|e| e.to_string())?
            .len()
            > 16_384
        {
            return Err("Captured visibility scope exceeds its native budget.".into());
        }
        let intent: CapturedIntent = serde_json::from_value(captured.clone())
            .map_err(|_| "Malformed native visibility scope.")?;
        if intent.mode != "hidhide-session"
            || intent.source_ids != ids
            || intent.policies.len() != ids.len()
            || ids.iter().any(|id| {
                !matches!(
                    intent.policies.get(id),
                    Some(Intent::HideAll | Intent::HideFromProcess { .. })
                )
            })
        {
            return Err("Save an explicit supported hide policy for every physical selection; no broader scope was substituted.".into());
        }
        for policy in intent.policies.values() {
            policy.validate()?;
        }
        let outgoing = outgoing
            .map(|run| {
                let receipt = reference(run)?;
                if receipt.is_some()
                    && (!run
                        .attempted
                        .contains(&crate::profile_session::ApplyPhase::Visibility)
                        || !run
                            .attempted
                            .contains(&crate::profile_session::ApplyPhase::Start))
                {
                    return Err(
                        "Only an exact applied profile can provide outgoing visibility proof."
                            .to_owned(),
                    );
                }
                Ok(receipt)
            })
            .transpose()?
            .flatten();
        let prepared = if let Some(receipt) = &outgoing {
            result.io.request(
                "handoff_preflight",
                &json!({"owner":receipt.visibility_owner,"intent":captured}),
            )?
        } else {
            result.io.request("preflight", &captured)?
        };
        if serde_json::to_vec(&prepared)
            .map_err(|e| e.to_string())?
            .len()
            > 131_072
        {
            return Err("Native visibility preparation exceeds its budget.".into());
        }
        let proof: Preparation = serde_json::from_value(prepared.clone())
            .map_err(|_| "Malformed native visibility preparation.")?;
        if proof.intent != intent {
            return Err("Native visibility scope changed during preflight.".into());
        }
        match (outgoing, proof.handoff.as_ref()) {
            (Some(receipt), Some(handoff)) if handoff.owner == receipt.visibility_owner => {
                handoff.validate()?;
                result.handoff = Some(Outgoing {
                    receipt,
                    proof: handoff.clone(),
                });
            }
            (None, None) => {}
            _ => return Err(
                "The native outgoing visibility proof does not match its applied profile receipt."
                    .into(),
            ),
        }
        let policy = proof
            .policy
            .as_ref()
            .ok_or("HidHide preflight did not retain an exact policy.")?;
        policy.validate()?;
        if policy.selections.len() != ids.len() {
            return Err("Native visibility preparation lost a physical selection.".into());
        }
        let digest = format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(&prepared).map_err(|e| e.to_string())?)
        );
        result.captured = Some((ids, digest, prepared));
        Ok(result)
    }
    /// Runs under final coordinator admission, before outgoing authority is
    /// revoked or transition intent can mutate external visibility.
    pub(crate) fn validate_handoff(&self, previous: Option<&OwnedRun>) -> Result<(), String> {
        let Some(handoff) = &self.handoff else {
            return Ok(());
        };
        let previous =
            previous.ok_or("The outgoing hidden profile changed during native preflight.")?;
        if !previous
            .attempted
            .contains(&crate::profile_session::ApplyPhase::Visibility)
            || !previous
                .attempted
                .contains(&crate::profile_session::ApplyPhase::Start)
            || reference(previous)?.as_ref() != Some(&handoff.receipt)
        {
            return Err(
                "The outgoing visibility owner/receipt changed before profile admission.".into(),
            );
        }
        if self
            .io
            .request("handoff_validate", &json!({"proof":handoff.proof}))?
            != json!({"valid":true})
        {
            return Err("The outgoing native hide session could not be revalidated.".into());
        }
        Ok(())
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.receipt.is_none()
    }
    pub(crate) fn needs_cleanup(&self) -> bool {
        self.receipt.is_some() && self.attempted && !self.restored
    }

    pub(crate) fn receipts(&self) -> Result<Vec<Receipt>, String> {
        self.receipt
            .as_ref()
            .map(|receipt| {
                Ok(vec![Receipt {
                    kind: KIND.into(),
                    resource_id: RESOURCE.into(),
                    state: serde_json::to_value(receipt).map_err(|e| e.to_string())?,
                }])
            })
            .unwrap_or_else(|| Ok(vec![]))
    }
    fn request(&self, operation: &str, prepared: bool) -> Result<Value, String> {
        let receipt = self
            .receipt
            .as_ref()
            .ok_or("No owned visibility selection.")?;
        let mut value = json!({"owner":receipt.visibility_owner});
        if prepared {
            value["prepared"] = self
                .prepared
                .clone()
                .ok_or("Recovery cannot reapply visibility.")?;
        }
        self.io.request(operation, &value)
    }
    fn status(&mut self, value: &Value) -> Result<Progress, String> {
        if !value.is_object()
            || serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > 16_384
        {
            return Err("Malformed native visibility worker status.".into());
        }
        let active = value["active"]
            .as_bool()
            .ok_or("Native visibility status lacks ownership state.")?;
        let phase = value["phase"]
            .as_str()
            .ok_or("Native visibility status lacks a phase.")?;
        if let Some(job) = value.get("jobId") {
            let job = job
                .as_str()
                .filter(|id| uuid(id))
                .ok_or("Native visibility worker identity is malformed.")?;
            if self.job.as_ref().is_some_and(|old| old != job) {
                return Err(
                    "Native visibility worker ownership changed; preserve its state.".into(),
                );
            }
            self.job = Some(job.into());
        } else if active {
            return Err("Native visibility worker has no exact job identity.".into());
        }
        if active && phase == "active" {
            let session = value["sessionId"]
                .as_str()
                .filter(|id| uuid(id))
                .ok_or("Native visibility is active without an exact journal session.")?;
            if self.session.as_ref().is_some_and(|old| old != session) {
                return Err(
                    "Native visibility journal ownership changed; preserve its state.".into(),
                );
            }
            self.session = Some(session.into());
        }
        match (active, phase) {
            (true, "active") => Ok(Progress::Active),
            (true, "preflight" | "applying" | "baseline" | "restoring" | "queued") => {
                Ok(Progress::Pending)
            }
            (false, "restored") => Ok(Progress::Restored),
            _ => Err(format!(
                "Owned visibility needs recovery attention: {}",
                value["message"]
                    .as_str()
                    .unwrap_or("policy conflict")
                    .chars()
                    .take(512)
                    .collect::<String>()
            )),
        }
    }
    /// Call only after the profile intent/receipt is durable and inputs/output
    /// fallback have been acquired. A failed or ambiguous Start is never retried.
    pub(crate) fn start(&mut self) -> Result<Progress, String> {
        if self.is_empty() {
            return Ok(Progress::Restored);
        }
        if self.attempted || self.stopping || self.restored || self.prepared.is_none() {
            return Err("Visibility acquisition is one-shot; restore the retained owner before another Apply.".into());
        }
        self.attempted = true;
        let value = self.request("start", true)?;
        let progress = self.status(&value)?;
        if progress == Progress::Restored {
            return Err(
                "Visibility ended before activation; no profile delivery was admitted.".into(),
            );
        }
        Ok(progress)
    }
    pub(crate) fn poll(&mut self) -> Result<Progress, String> {
        if self.is_empty() || self.restored {
            return Ok(Progress::Restored);
        }
        if !self.attempted {
            return Err("Visibility has not been acquired by this profile.".into());
        }
        let value = self.request("poll", false)?;
        let progress = self.status(&value)?;
        if progress == Progress::Restored && !self.stopping {
            return Err("Owned visibility ended; disarm profile delivery before cleanup.".into());
        }
        Ok(progress)
    }
    /// Poll asynchronous restore without repeatedly cancelling its recovery
    /// worker. A final guarded Stop releases the native owner after restoration.
    pub(crate) fn stop(&mut self) -> Result<Progress, String> {
        if self.is_empty() || self.restored {
            return Ok(Progress::Restored);
        }
        let operation = if self.stopping { "poll" } else { "stop" };
        // A lost reply may already have started restoration. Retain that
        // disposition so a retry polls instead of cancelling the worker.
        self.stopping = true;
        let value = self.request(operation, false)?;
        let mut progress = self.status(&value)?;
        if operation == "poll" && progress == Progress::Active {
            // A definite rejection may have preceded restoration (for example,
            // another owner was present). Only a fresh exact-owner active
            // observation permits initiating Stop again. Pending restoration
            // is always polled, never cancelled and restarted.
            let value = self.request("stop", false)?;
            progress = self.status(&value)?;
        }
        if progress == Progress::Restored {
            if operation == "poll" {
                let value = self.request("stop", false)?;
                if self.status(&value)? != Progress::Restored {
                    return Err("Visibility ownership was not released after restore.".into());
                }
            }
            self.restored = true;
            Ok(Progress::Restored)
        } else {
            Ok(Progress::Pending)
        }
    }
    /// Construct cleanup authority from the exact retained native receipt only.
    /// There is no prepared policy, and Start can never run on this object.
    pub(crate) fn from_receipts(
        run: &OwnedRun,
        io: Arc<dyn VisibilityIo>,
    ) -> Result<Option<Self>, String> {
        let Some(receipt) = reference(run)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            io,
            receipt: Some(receipt),
            prepared: None,
            attempted: true,
            stopping: false,
            restored: false,
            job: None,
            session: None,
            handoff: None,
        }))
    }
}
#[cfg(test)]
impl Visibility {
    fn prepare(
        plan: &ExecutionPlan,
        snapshot: &CompileSnapshot,
        owner: &RunOwner,
        io: Arc<dyn VisibilityIo>,
    ) -> Result<Self, String> {
        if !owner_valid(owner) {
            return Err("Invalid profile visibility owner.".into());
        }
        Self::stage(plan, snapshot, io)?.bind(owner)
    }
}
#[cfg(test)]
#[path = "profile_session_visibility_tests.rs"]
pub(crate) mod tests;

//! Per-instance protected assignment stamps and journal receipts. Recovery can
//! restore only an exact owned stamp, never replay a profile assignment.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentReceipt {
    schema_version: u8,
    expected: AssignmentRef,
    desired: AssignmentRef,
    baseline: AssignmentRef,
    capability_fingerprint: String,
    installation_fingerprint: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignmentRef {
    instance_id: String,
    companion_id: String,
    publication_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ownership: Option<AssignmentOwner>,
}
fn reference(assignment: &Assignment) -> AssignmentRef {
    AssignmentRef {
        instance_id: assignment.instance_id.clone(),
        companion_id: assignment.companion_id.clone(),
        publication_hash: assignment.publication_hash.clone(),
        ownership: assignment.ownership.clone(),
    }
}
fn reconstruct(inner: &Inner, reference: &AssignmentRef) -> Result<Assignment, String> {
    let publication = inner
        .publications
        .get(&reference.publication_hash)
        .ok_or("The immutable assignment panel is no longer retained.")?;
    Ok(Assignment {
        instance_id: reference.instance_id.clone(),
        companion_id: reference.companion_id.clone(),
        publication_hash: reference.publication_hash.clone(),
        definition: publication.definition(&reference.instance_id),
        ownership: reference.ownership.clone(),
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentRestore {
    Restored,
    AlreadyBaseline,
    OwnershipLost,
}
impl AssignmentReceipt {
    pub fn instance_id(&self) -> &str {
        &self.desired.instance_id
    }
    pub fn owner(&self) -> &str {
        self.desired
            .ownership
            .as_ref()
            .map(|stamp| stamp.owner.as_str())
            .unwrap_or("")
    }
    pub fn revision(&self) -> u64 {
        self.desired
            .ownership
            .as_ref()
            .map(|stamp| stamp.revision)
            .unwrap_or(0)
    }
    pub fn publication_hash(&self) -> &str {
        &self.desired.publication_hash
    }
    pub fn capability_fingerprint(&self) -> &str {
        &self.capability_fingerprint
    }
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "sha256:{}",
        hash(&serde_json::to_vec(value).map_err(|error| error.to_string())?)
    ))
}
fn installation(inner: &Inner, companion: &str) -> Result<String, String> {
    let credential = inner
        .record
        .credentials
        .iter()
        .find(|credential| credential.companion_id == companion)
        .ok_or("The profile companion installation is no longer authorized.")?;
    digest(&(companion, &credential.token_hash))
}
fn shape(receipt: &AssignmentReceipt) -> Result<(), String> {
    let stamp = receipt
        .desired
        .ownership
        .as_ref()
        .ok_or("The assignment receipt has no native owner.")?;
    ensure(
        receipt.schema_version == 1
            && uuid::Uuid::parse_str(&stamp.owner)
                .is_ok_and(|owner| owner.to_string() == stamp.owner)
            && stamp.revision > 0
            && receipt.expected.instance_id == receipt.desired.instance_id
            && receipt.baseline.instance_id == receipt.desired.instance_id
            && receipt.expected.companion_id == receipt.desired.companion_id
            && receipt.baseline.companion_id == receipt.desired.companion_id
            && receipt.baseline.ownership.is_none()
            && digest_id(&receipt.capability_fingerprint)
            && digest_id(&receipt.installation_fingerprint)
            && digest_id(&stamp.baseline_hash)
            && stamp.baseline_publication_hash == receipt.baseline.publication_hash,
        "Invalid or modified native assignment receipt.",
    )?;
    for assignment in [&receipt.expected, &receipt.desired, &receipt.baseline] {
        ensure(
            id(&assignment.instance_id)
                && id(&assignment.companion_id)
                && digest_id(&assignment.publication_hash),
            "Invalid assignment receipt identity or source descriptor.",
        )?;
    }
    Ok(())
}
fn publication(inner: &Inner, assignment: &Assignment) -> Result<(), String> {
    let publication = inner
        .publications
        .get(&assignment.publication_hash)
        .ok_or("The immutable assignment panel is no longer retained.")?;
    ensure(
        publication.definition(&assignment.instance_id) == assignment.definition,
        "The immutable assignment source descriptor changed.",
    )
}
pub(super) fn validate_stamp(inner: &Inner, receipt: &AssignmentReceipt) -> Result<(), String> {
    shape(receipt)?;
    ensure(
        inner
            .record
            .instances
            .iter()
            .any(|assignment| reference(assignment) == receipt.desired)
            && installation(inner, &receipt.desired.companion_id)?
                == receipt.installation_fingerprint,
        "This profile no longer owns the exact companion assignment revision.",
    )
}
pub(super) fn validate_owned(inner: &Inner, receipt: &AssignmentReceipt) -> Result<(), String> {
    validate_stamp(inner, receipt)?;
    let desired = reconstruct(inner, &receipt.desired)?;
    let baseline = reconstruct(inner, &receipt.baseline)?;
    ensure(
        inner
            .record
            .instances
            .iter()
            .any(|assignment| assignment == &desired)
            && installation(inner, &receipt.desired.companion_id)?
                == receipt.installation_fingerprint
            && receipt.desired.ownership.as_ref().unwrap().baseline_hash == digest(&baseline)?,
        "This profile no longer owns the exact companion assignment revision.",
    )?;
    publication(inner, &desired)?;
    ensure(
        inner.publications[&receipt.desired.publication_hash].capability_fingerprint
            == receipt.capability_fingerprint,
        "The owned companion capability changed.",
    )
}

impl Hub {
    pub fn capture_profile_assignment(
        &self,
        proof: &Preparation,
        owner: &str,
        previous: Option<&AssignmentReceipt>,
    ) -> Result<AssignmentReceipt, String> {
        ensure(
            uuid::Uuid::parse_str(owner).is_ok_and(|id| id.to_string() == owner),
            "Use a canonical native profile run identity.",
        )?;
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.validate_profile_preparation_inner(&inner, proof)?;
        let expected = inner
            .record
            .instances
            .iter()
            .find(|assignment| assignment.instance_id == proof.instance_id)
            .ok_or("The prepared companion assignment disappeared.")?
            .clone();
        publication(&inner, &expected)?;
        let mut baseline = expected.clone();
        if let Some(stamp) = &expected.ownership {
            ensure(
                stamp.owner != owner,
                "A new assignment requires a new native profile owner.",
            )?;
            let previous = previous
                .ok_or("The outgoing companion assignment receipt is required for takeover.")?;
            validate_owned(&inner, previous)?;
            ensure(
                previous.desired == reference(&expected),
                "The outgoing receipt owns a different companion assignment.",
            )?;
            baseline = reconstruct(&inner, &previous.baseline)?;
        } else {
            ensure(
                previous.is_none(),
                "An outgoing receipt cannot claim an unowned assignment.",
            )?;
        }
        baseline.ownership = None;
        let revision = expected
            .ownership
            .as_ref()
            .map_or(Some(1), |stamp| stamp.revision.checked_add(1))
            .ok_or("Companion ownership revision exhausted.")?;
        let desired = Assignment {
            instance_id: proof.instance_id.clone(),
            companion_id: proof.companion_id.clone(),
            publication_hash: proof.publication_hash.clone(),
            definition: proof.definition.clone(),
            ownership: Some(AssignmentOwner {
                owner: owner.into(),
                revision,
                baseline_hash: digest(&baseline)?,
                baseline_publication_hash: baseline.publication_hash.clone(),
            }),
        };
        let receipt = AssignmentReceipt {
            schema_version: 1,
            installation_fingerprint: installation(&inner, &desired.companion_id)?,
            expected: reference(&expected),
            desired: reference(&desired),
            baseline: reference(&baseline),
            capability_fingerprint: proof.capability_fingerprint.clone(),
        };
        shape(&receipt)?;
        Ok(receipt)
    }
    /// This method is an effect and must follow a durable acquisition intent.
    pub fn acquire_profile_assignment(&self, receipt: &AssignmentReceipt) -> Result<(), String> {
        shape(receipt)?;
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if validate_owned(&inner, receipt).is_ok() {
            return Ok(());
        }
        ensure(
            inner
                .record
                .instances
                .iter()
                .any(|assignment| reference(assignment) == receipt.expected)
                && installation(&inner, &receipt.desired.companion_id)?
                    == receipt.installation_fingerprint,
            "Companion assignment or installation changed after the acquisition was captured.",
        )?;
        let desired = reconstruct(&inner, &receipt.desired)?;
        let baseline = reconstruct(&inner, &receipt.baseline)?;
        ensure(
            receipt.desired.ownership.as_ref().unwrap().baseline_hash == digest(&baseline)?,
            "The captured original companion baseline changed.",
        )?;
        publication(&inner, &desired)?;
        ensure(
            inner.publications[&receipt.desired.publication_hash].capability_fingerprint
                == receipt.capability_fingerprint,
            "The desired companion capability changed.",
        )?;
        self.replace_profile_assignment(&mut inner, &desired)
    }
    pub fn validate_profile_assignment(&self, receipt: &AssignmentReceipt) -> Result<(), String> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        validate_owned(&inner, receipt)
    }
    /// Neutralization can verify its protected owner without a panel cache.
    /// This grants no publication, baseline, or feedback delivery capability.
    pub fn validate_profile_assignment_owner(
        &self,
        receipt: &AssignmentReceipt,
    ) -> Result<(), String> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        validate_stamp(&inner, receipt)
    }
    /// Recovery never acquires, rearms input, or reinstalls the profile panel.
    pub fn restore_profile_assignment(
        &self,
        receipt: &AssignmentReceipt,
    ) -> Result<AssignmentRestore, String> {
        shape(receipt)?;
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if inner
            .record
            .instances
            .iter()
            .any(|assignment| reference(assignment) == receipt.baseline)
        {
            return Ok(AssignmentRestore::AlreadyBaseline);
        }
        if validate_stamp(&inner, receipt).is_err() {
            return Ok(AssignmentRestore::OwnershipLost);
        }
        validate_owned(&inner, receipt)?;
        let baseline = reconstruct(&inner, &receipt.baseline)?;
        publication(&inner, &baseline)?;
        self.replace_profile_assignment(&mut inner, &baseline)?;
        Ok(AssignmentRestore::Restored)
    }
    fn replace_profile_assignment(
        &self,
        inner: &mut Inner,
        assignment: &Assignment,
    ) -> Result<(), String> {
        let prior = inner
            .record
            .instances
            .iter()
            .find(|row| row.instance_id == assignment.instance_id)
            .ok_or("The owned companion assignment disappeared.")?
            .clone();
        let mut record = inner.record.clone();
        *record
            .instances
            .iter_mut()
            .find(|row| row.instance_id == assignment.instance_id)
            .unwrap() = assignment.clone();
        let sessions = inner
            .sessions
            .iter()
            .filter(|(_, session)| session.token.source_id == assignment.instance_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for session in sessions {
            self.disarm(inner, &session, "profile-assignment-changed", true);
        }
        if let Err(error) = self
            .sink
            .register(assignment.definition.clone())
            .and_then(|_| self.persist(&record))
        {
            if let Err(restore) = self.sink.register(prior.definition) {
                inner.diagnostic = Some(format!("Companion assignment was preserved but descriptor restoration failed: {restore}"));
            }
            return Err(format!(
                "Companion acquisition was not committed; the source remains disarmed: {error}"
            ));
        }
        inner.record = record;
        inner.feedback.remove(&assignment.instance_id);
        Ok(())
    }
    pub fn configure_profile_feedback(
        &self,
        receipt: &AssignmentReceipt,
        bindings: &[FeedbackBinding],
    ) -> Result<(), String> {
        self.configure_feedback_authorized(
            receipt.instance_id(),
            receipt.publication_hash(),
            bindings,
            Some(receipt),
        )
    }
    pub fn profile_feedback_token(
        &self,
        receipt: &AssignmentReceipt,
    ) -> Result<FeedbackToken, String> {
        self.feedback_token_authorized(receipt.instance_id(), Some(receipt))
    }
    pub fn publish_profile_feedback(
        &self,
        receipt: &AssignmentReceipt,
        token: &FeedbackToken,
        values: &[FeedbackSample],
        mode: FeedbackMode,
        now_ms: u64,
    ) -> Result<u64, String> {
        self.publish_feedback_authorized(token, values, mode, now_ms, Some(receipt))
    }
    pub fn clear_profile_feedback(
        &self,
        receipt: &AssignmentReceipt,
        now_ms: u64,
    ) -> Result<(), String> {
        self.clear_feedback_authorized(
            receipt.instance_id(),
            receipt.publication_hash(),
            now_ms,
            Some(receipt),
        )
    }
}

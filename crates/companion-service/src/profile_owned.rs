//! Per-instance protected assignment stamps and journal receipts. Recovery can
//! restore only an exact owned stamp, never replay a profile assignment.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentReceipt {
    schema_version: u8,
    expected: Option<AssignmentRef>,
    desired: AssignmentRef,
    baseline: Option<AssignmentRef>,
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
    Hub::validate_assignment_target(inner,&reference.companion_id,publication)?;
    Ok(Assignment {
        instance_id: reference.instance_id.clone(),
        companion_id: reference.companion_id.clone(),
        publication_hash: reference.publication_hash.clone(),
        definition: publication.definition(&reference.instance_id),
        ownership: reference.ownership.clone(),
    })
}
fn baseline(inner: &Inner, receipt: &AssignmentReceipt) -> Result<Option<Assignment>, String> {
    receipt
        .baseline
        .as_ref()
        .map(|reference| reconstruct(inner, reference))
        .transpose()
}
fn current(inner: &Inner, instance: &str) -> Option<AssignmentRef> {
    inner
        .record
        .instances
        .iter()
        .find(|row| row.instance_id == instance)
        .map(reference)
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
fn baseline_publication_hash(publication: Option<&str>) -> Result<String, String> {
    match publication {
        Some(publication) => Ok(publication.into()),
        // Keep the protected stamp schema compatible. Absence has a stable
        // digest sentinel rather than pretending a panel was originally set.
        None => digest(&Option::<Assignment>::None),
    }
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
    let transition = match &receipt.expected {
        None => receipt.baseline.is_none() && stamp.revision == 1,
        Some(expected) => match &expected.ownership {
            None => receipt.baseline.as_ref() == Some(expected) && stamp.revision == 1,
            Some(prior) => {
                prior.owner != stamp.owner
                    && prior.revision.checked_add(1) == Some(stamp.revision)
                    && prior.baseline_hash == stamp.baseline_hash
                    && prior.baseline_publication_hash == stamp.baseline_publication_hash
            }
        },
    };
    ensure(
        receipt.schema_version == 1
            && uuid::Uuid::parse_str(&stamp.owner)
                .is_ok_and(|owner| owner.to_string() == stamp.owner)
            && stamp.revision > 0
            && receipt.expected.as_ref().is_none_or(|expected| {
                expected.instance_id == receipt.desired.instance_id
                    && expected.companion_id == receipt.desired.companion_id
            })
            && receipt.baseline.as_ref().is_none_or(|baseline| {
                baseline.instance_id == receipt.desired.instance_id
                    && baseline.companion_id == receipt.desired.companion_id
                    && baseline.ownership.is_none()
            })
            && (receipt.baseline.is_some()
                || receipt.desired.instance_id
                    == profile_instance_id(&receipt.desired.companion_id))
            && transition
            && digest_id(&receipt.capability_fingerprint)
            && digest_id(&receipt.installation_fingerprint)
            && digest_id(&stamp.baseline_hash)
            && stamp.baseline_publication_hash
                == baseline_publication_hash(
                    receipt
                        .baseline
                        .as_ref()
                        .map(|baseline| baseline.publication_hash.as_str()),
                )?,
        "Invalid or modified native assignment receipt.",
    )?;
    for assignment in receipt
        .expected
        .iter()
        .chain(std::iter::once(&receipt.desired))
        .chain(receipt.baseline.iter())
    {
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
    Hub::validate_assignment_target(inner,&assignment.companion_id,publication)?;
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
    let baseline = baseline(inner, receipt)?;
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
            .cloned();
        if let Some(expected) = &expected {
            publication(&inner, expected)?;
        } else {
            ensure(
                inner.record.instances.len() < MAX_INSTANCES,
                "The companion assignment limit was reached.",
            )?;
        }
        let mut baseline = expected.clone();
        if let Some(stamp) = expected
            .as_ref()
            .and_then(|expected| expected.ownership.as_ref())
        {
            ensure(
                stamp.owner != owner,
                "A new assignment requires a new native profile owner.",
            )?;
            let previous = previous
                .ok_or("The outgoing companion assignment receipt is required for takeover.")?;
            validate_owned(&inner, previous)?;
            ensure(
                expected
                    .as_ref()
                    .is_some_and(|expected| previous.desired == reference(expected)),
                "The outgoing receipt owns a different companion assignment.",
            )?;
            baseline = self::baseline(&inner, previous)?;
        } else {
            ensure(
                previous.is_none(),
                "An outgoing receipt cannot claim an unowned assignment.",
            )?;
        }
        if let Some(baseline) = &mut baseline {
            baseline.ownership = None;
        }
        let revision = expected
            .as_ref()
            .and_then(|expected| expected.ownership.as_ref())
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
                baseline_publication_hash: baseline_publication_hash(
                    baseline
                        .as_ref()
                        .map(|baseline| baseline.publication_hash.as_str()),
                )?,
            }),
        };
        let receipt = AssignmentReceipt {
            schema_version: 1,
            installation_fingerprint: installation(&inner, &desired.companion_id)?,
            expected: expected.as_ref().map(reference),
            desired: reference(&desired),
            baseline: baseline.as_ref().map(reference),
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
            current(&inner, receipt.instance_id()) == receipt.expected
                && (receipt.expected.is_some()
                    || (receipt.instance_id()
                        == profile_instance_id(&receipt.desired.companion_id)
                        && !inner.record.instances.iter().any(|assignment| {
                            assignment.companion_id == receipt.desired.companion_id
                        })))
                && installation(&inner, &receipt.desired.companion_id)?
                    == receipt.installation_fingerprint,
            "Companion assignment or installation changed after the acquisition was captured.",
        )?;
        let desired = reconstruct(&inner, &receipt.desired)?;
        let baseline = baseline(&inner, receipt)?;
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
        self.replace_profile_assignment(&mut inner, receipt.instance_id(), Some(&desired))
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
        self.restore_profile_assignment_latest(receipt, &std::collections::BTreeMap::new())
    }
    /// Release an exact protected profile owner, restoring the latest saved
    /// publication of its original panel identity when native saved heads exist.
    /// The immutable receipt still proves the original baseline; replacement
    /// metadata never weakens its owner, installation or source comparisons.
    pub fn restore_profile_assignment_latest(
        &self,
        receipt: &AssignmentReceipt,
        latest_by_panel: &std::collections::BTreeMap<String, String>,
    ) -> Result<AssignmentRestore, String> {
        shape(receipt)?;
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if current(&inner, receipt.instance_id()) == receipt.baseline {
            if receipt.baseline.is_none() {
                self.sink.unregister(receipt.instance_id())?;
            }
            return Ok(AssignmentRestore::AlreadyBaseline);
        }
        if validate_stamp(&inner, receipt).is_err() {
            return Ok(AssignmentRestore::OwnershipLost);
        }
        let mut baseline = baseline(&inner, receipt)?;
        if let Some(baseline) = &mut baseline {
            validate_owned(&inner, receipt)?;
            publication(&inner, baseline)?;
            let original = inner.publications.get(&baseline.publication_hash)
                .ok_or("The immutable original companion panel is no longer retained.")?;
            if let Some(latest_hash) = latest_by_panel.get(&original.ui_id) {
                ensure(digest_id(latest_hash), "Invalid latest saved companion publication hash.")?;
                let latest = inner.publications.get(latest_hash)
                    .ok_or("The latest saved companion panel is no longer retained; restore its publication before recovery.")?;
                ensure(latest.ui_id == original.ui_id, "The latest saved companion panel has a different stable identity.")?;
                baseline.publication_hash = latest.publication_hash.clone();
                baseline.definition = latest.definition(&baseline.instance_id);
            }
        } else {
            ensure(
                receipt.desired.ownership.as_ref().unwrap().baseline_hash == digest(&baseline)?,
                "The captured original companion baseline changed.",
            )?;
        }
        self.replace_profile_assignment(&mut inner, receipt.instance_id(), baseline.as_ref())?;
        Ok(AssignmentRestore::Restored)
    }
    fn replace_profile_assignment(
        &self,
        inner: &mut Inner,
        instance: &str,
        assignment: Option<&Assignment>,
    ) -> Result<(), String> {
        let prior = inner
            .record
            .instances
            .iter()
            .find(|row| row.instance_id == instance)
            .cloned();
        let mut record = inner.record.clone();
        match (
            record
                .instances
                .iter()
                .position(|row| row.instance_id == instance),
            assignment,
        ) {
            (Some(index), Some(assignment)) => record.instances[index] = assignment.clone(),
            (Some(index), None) => {
                record.instances.remove(index);
            }
            (None, Some(assignment)) => {
                ensure(
                    record.instances.len() < MAX_INSTANCES,
                    "The companion assignment limit was reached.",
                )?;
                record.instances.push(assignment.clone());
            }
            (None, None) => return Err("The owned companion assignment disappeared.".into()),
        }
        let sessions = inner
            .sessions
            .iter()
            .filter(|(_, session)| session.token.source_id == instance)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for session in sessions {
            self.disarm(inner, &session, "profile-assignment-changed", true);
        }
        if let Err(error) = assignment
            .map(|assignment| self.sink.register(assignment.definition.clone()))
            .unwrap_or_else(|| self.sink.unregister(instance))
            .and_then(|_| self.persist(&record))
        {
            let rollback = prior
                .map(|prior| self.sink.register(prior.definition))
                .unwrap_or_else(|| self.sink.unregister(instance));
            if let Err(restore) = rollback {
                inner.diagnostic = Some(format!("Companion assignment was preserved but descriptor restoration failed: {restore}"));
            }
            return Err(format!(
                "Companion acquisition was not committed; the source remains disarmed: {error}"
            ));
        }
        inner.record = record;
        inner.feedback.remove(instance);
        Ok(())
    }
}

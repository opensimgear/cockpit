//! Read-only provenance for replacing one fully applied native hide session.
//! Incoming hiding is planned against the original journal state; this proof
//! cannot apply, restore, or transfer ownership on its own.
use super::{
    journal::Record,
    policy::{Plan, State},
    runtime::Status,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Proof {
    pub schema_version: u32,
    pub owner: String,
    pub job_id: String,
    pub session_id: String,
    pub record_hash: String,
    pub plan: Plan,
}
fn canonical(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .ok()
        .map(|id| id.to_string())
        .as_deref()
        == Some(value)
}
fn fingerprint(record: &Record) -> Result<String, String> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(record).map_err(|e| e.to_string())?)
    ))
}
impl Proof {
    pub fn capture(
        owner: &str,
        record: &Record,
        status: &Status,
        actual: &State,
    ) -> Result<Self, String> {
        let proof = Self {
            schema_version: 1,
            owner: owner.into(),
            job_id: status.job_id.clone(),
            session_id: record.id.clone(),
            record_hash: fingerprint(record)?,
            plan: record.plan.clone(),
        };
        proof.validate_active(record, status, actual)?;
        Ok(proof)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || ![&self.owner, &self.job_id, &self.session_id]
                .iter()
                .all(|id| canonical(id))
            || !self
                .record_hash
                .strip_prefix("sha256:")
                .is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            || serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 131_072
        {
            return Err("Invalid native visibility handoff proof.".into());
        }
        self.plan.validate()
    }
    pub fn validate_active(
        &self,
        record: &Record,
        status: &Status,
        actual: &State,
    ) -> Result<(), String> {
        self.validate_metadata_active(record, status)?;
        if actual != &self.plan.after {
            return Err("The outgoing native driver snapshot changed from its applied hide state; preserve it.".into());
        }
        Ok(())
    }
    /// Exact cached owner/journal admission only. This makes no claim that a
    /// fresh driver read occurred; restoration/Start retain actual-state fences.
    pub fn validate_metadata_active(&self, record: &Record, status: &Status) -> Result<(), String> {
        self.validate()?;
        let steps = record.validate()?;
        if record.id != self.session_id
            || record.plan != self.plan
            || record.clean
            || record.restoring
            || record.pending.is_some()
            || record.position + 1 != steps.len()
            || fingerprint(record)? != self.record_hash
            || !status.active
            || status.phase != "active"
            || status.job_id != self.job_id
            || status.session_id.as_deref() != Some(self.session_id.as_str())
        {
            return Err("The outgoing hide owner no longer matches its fully applied native worker and journal metadata; preserve it.".into());
        }
        Ok(())
    }
    pub fn validate_restored(
        &self,
        record: &Record,
        status: &Status,
        actual: &State,
    ) -> Result<(), String> {
        self.validate()?;
        record.validate()?;
        if record.id != self.session_id
            || record.plan != self.plan
            || !record.clean
            || record.position != 0
            || record.pending.is_some()
            || status.active
            || status.phase != "restored"
            || status.job_id != self.job_id
            || status.session_id.as_deref() != Some(self.session_id.as_str())
            || actual != &self.plan.before
        {
            return Err("Restore the exact outgoing visibility journal to its captured original before hiding for the incoming owner.".into());
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;

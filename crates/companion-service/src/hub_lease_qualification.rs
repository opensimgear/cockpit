//! Read-only, development-only evidence for the running companion fixture.
use super::{Hub, Session, LEASE_MS};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone, Default)]
pub(super) struct Evidence {
    renewals: u64,
    max_gap_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    instance_id: String,
    session_id: String,
    generation: String,
    publication_hash: String,
}

impl Session {
    pub(super) fn note_input_renewal(&mut self, now: u64) {
        if self.armed {
            self.lease_evidence.renewals = self.lease_evidence.renewals.saturating_add(1);
            self.lease_evidence.max_gap_ms = self
                .lease_evidence
                .max_gap_ms
                .max(now.saturating_sub(self.last_seen));
        }
    }
}

impl Hub {
    pub fn qualify_input_leases(&self, identities: [Identity; 2]) -> Result<Value, String> {
        if identities[0].instance_id == identities[1].instance_id {
            return Err("Choose two distinct panel instances.".into());
        }
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let now = self.now_ms().max(inner.last_now);
        let mut panels = Vec::with_capacity(2);
        for identity in identities {
            let session = inner
                .sessions
                .get(&identity.session_id)
                .filter(|session| {
                    session.armed
                        && session.token.source_id == identity.instance_id
                        && session.token.generation.to_string() == identity.generation
                        && session.token.publication_hash == identity.publication_hash
                })
                .ok_or("The exact panel input session is no longer armed.")?;
            let gap_at_read_ms = now.saturating_sub(session.last_seen);
            let max_gap_ms = session.lease_evidence.max_gap_ms.max(gap_at_read_ms);
            if max_gap_ms >= LEASE_MS {
                return Err("The exact panel input lease expired.".into());
            }
            panels.push(json!({
                "instanceId": identity.instance_id, "sessionId": identity.session_id,
                "generation": identity.generation, "publicationHash": identity.publication_hash,
                "leaseMs": LEASE_MS, "acceptedRenewals": session.lease_evidence.renewals,
                "maxGapMs": max_gap_ms, "gapAtReadMs": gap_at_read_ms,
                "minimumMarginMs": LEASE_MS - max_gap_ms,
            }));
        }
        Ok(json!(panels))
    }
}

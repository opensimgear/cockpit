//! Remove source descriptors together with their durable assignment authority.
use super::*;

impl Hub {
    pub(super) fn commit_instance_removal(
        &self,
        inner: &mut Inner,
        record: &Record,
        reason: &str,
    ) -> Result<(), String> {
        let removed = inner.record.instances.iter()
            .filter(|old| !record.instances.iter().any(|next| next.instance_id == old.instance_id))
            .cloned().collect::<Vec<_>>();
        let sessions = inner.sessions.iter()
            .filter(|(_, session)| removed.iter().any(|item| item.instance_id == session.token.source_id))
            .map(|(id, _)| id.clone()).collect::<Vec<_>>();
        for session in sessions { self.disarm(inner, &session, reason, true); }
        // As with saved-panel selections, callbacks run before persistence and
        // are compensated on failure. No callback runs in a storage transaction.
        let commit = (|| {
            for item in &removed { self.sink.unregister(&item.instance_id)?; }
            self.persist(record)
        })();
        if let Err(error) = commit {
            let failures = removed.iter().filter_map(|item|
                self.sink.register(item.definition.clone()).err()).collect::<Vec<_>>();
            if !failures.is_empty() {
                inner.diagnostic = Some(format!(
                    "Companion authority was preserved; descriptor restoration needs attention: {}",
                    failures.join("; ")));
            }
            return Err(format!("Companion removal was not committed; changed inputs remain disarmed: {error}"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "hub_retirement_tests.rs"]
mod tests;

//! Standalone persistent visibility ownership; no vJoy dependency or macro replay.
use super::{
    journal::{self, Backend, Record, Store},
    policy::Plan,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub session: String,
    pub generation: u64,
    pub sequence: u64,
    pub interface: String,
}
impl Source {
    pub fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.id).is_err()
            || uuid::Uuid::parse_str(&self.session).is_err()
            || self.generation == 0
            || self.sequence == 0
            || self.interface.is_empty()
            || self.interface.len() > 2048
        {
            return Err("Visibility requires an exact current controller baseline.".into());
        }
        Ok(())
    }
}
pub fn validate_sources(
    expected: &[Source],
    current: &[Source],
    fresh: bool,
) -> Result<(), String> {
    if expected.is_empty()
        || expected.len() > 16
        || expected.len() != current.len()
        || expected
            .iter()
            .map(|s| &s.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != expected.len()
    {
        return Err("Invalid visibility source group.".into());
    }
    for old in expected {
        old.validate()?;
        let new = current
            .iter()
            .find(|s| s.id == old.id)
            .ok_or("A selected controller disappeared.")?;
        new.validate()?;
        if old.session != new.session
            || old.generation != new.generation
            || old.interface != new.interface
            || new.sequence < old.sequence
            || (fresh && new.sequence == old.sequence)
        {
            return Err("Controller capture lost its identity, baseline or fresh reports; restore visibility before retrying.".into());
        }
    }
    Ok(())
}
pub struct Session<B: Backend> {
    backend: B,
    store: Store,
    pub record: Record,
    pub sources: Vec<Source>,
    stopped: bool,
}
impl<B: Backend> Session<B> {
    pub fn start(
        mut backend: B,
        mut store: Store,
        plan: Plan,
        sources: Vec<Source>,
        mut observe: impl FnMut(bool) -> Result<Vec<Source>, String>,
    ) -> Result<Self, String> {
        validate_sources(&sources, &observe(false)?, false)?;
        let record = journal::apply(&mut backend, &mut store, plan, || {
            validate_sources(&sources, &observe(false)?, false)
        })?;
        // Establish the comparison point after hiding. A report obtained during
        // preflight cannot prove that capture still works under the new policy.
        let fresh = observe(false).and_then(|baseline| {
            validate_sources(&sources, &baseline, false)?;
            observe(true).and_then(|current| validate_sources(&baseline, &current, true))
        });
        if let Err(error) = fresh {
            return Err(
                match journal::restore(&mut backend, &mut store, &record.id) {
                    Ok(_) => format!("{error} Visibility restored."),
                    Err(restore) => format!("{error} Recovery requires attention: {restore}"),
                },
            );
        }
        Ok(Self {
            backend,
            store,
            record,
            sources,
            stopped: false,
        })
    }
    /// Called by the native worker without UI polling. Errors from source loss,
    /// executable relocation or an optional combined-session owner all stop hiding.
    pub fn check(&mut self, current: Result<Vec<Source>, String>) -> Result<(), String> {
        if self.stopped {
            return Err("Visibility session has stopped.".into());
        }
        let healthy = current.and_then(|value| validate_sources(&self.sources, &value, false));
        if let Err(error) = healthy {
            return Err(match self.stop() {
                Ok(()) => format!("{error} Visibility restored."),
                Err(restore) => format!("{error} Recovery requires attention: {restore}"),
            });
        }
        Ok(())
    }
    pub fn verify(
        &mut self,
        check: impl FnOnce(&mut B) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Err(error) = check(&mut self.backend) {
            return Err(match self.stop() {
                Ok(()) => format!("{error} Visibility restored."),
                Err(restore) => format!("{error} Recovery requires attention: {restore}"),
            });
        }
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if !self.stopped {
            self.record = journal::restore(&mut self.backend, &mut self.store, &self.record.id)?;
            self.stopped = true;
        }
        Ok(())
    }
}
impl<B: Backend> Drop for Session<B> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

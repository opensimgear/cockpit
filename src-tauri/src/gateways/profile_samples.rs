use super::*;

impl GatewayManager {
    pub(crate) fn with_profile_samples<T>(&self, topic: &str, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let pin = {
            let session = self.profile_session.lock().unwrap_or_else(|error| error.into_inner());
            let plan = session.prepare_sample_plan(topic)?;
            self.profile_samples.pin(plan)?
        };
        // The coordinator is available to routing, Stop and the capture worker.
        pin.wait_ready()?;
        action()
    }
}

#[cfg(test)]
#[path = "profile_samples_tests.rs"]
mod tests;

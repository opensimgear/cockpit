use super::*;
use crate::transport::notifications::recovery::RecoveryWatch;

impl Worker {
    pub(super) fn reconcile(
        &mut self,
        prior: &UsbRouteToken,
        expected: &Value,
        saved: bool,
        applied: bool,
    ) -> Result<(), String> {
        let expected_generation = prior.configuration_generation
            .checked_add(u32::from(applied))
            .ok_or("USB configuration generation is exhausted.")?;
        let explicit = self.explicit;
        let eligible = if explicit { candidate_for_explicit } else { candidate };
        let mut topology = self.recovery.as_ref().map(|source| source.watch(&self.wake));
        self.close();
        let deadline = Instant::now() + self.timing.reconnect;
        self.wait(self.timing.settle.min(self.timing.reconnect))?;

        while Instant::now() < deadline {
            // The shared owner publishes its cache before waking recovery. A
            // hardware retry can still need a fresh view of an unchanged port.
            let cached = topology.as_mut().and_then(RecoveryWatch::cached_ports);
            let ports = match cached { Some(ports) => ports, None => self.factory.ports()? };
            let ports: Vec<_> = ports.into_iter().filter(|port| {
                eligible(port) && port.serial_number.as_deref().and_then(module_console::serial)
                    .as_deref() == Some(&prior.serial)
            }).collect();

            if ports.len() > 1 {
                return Err("USB module reconnection is ambiguous; the commit was not repeated.".into());
            }
            if let Some(port) = ports.first() {
                if self.connect(port, explicit).is_ok() {
                    let actual_route = self.snapshot.route.as_ref().unwrap();
                    if actual_route.module_id != prior.module_id || actual_route.boot != prior.boot {
                        self.close();
                        return Err("The USB module restarted or was replaced during configuration; the commit was not repeated.".into());
                    }
                    let actual = self.snapshot.config.as_ref().unwrap();
                    if actual["config"] == *expected
                        && actual["generation"].as_u64() == Some(expected_generation as u64)
                        && (!saved || actual["generation"] == actual["savedGeneration"])
                    {
                        return Ok(());
                    }
                }
            } else if let Some(watch) = topology.as_ref().filter(|watch| watch.covered()) {
                if !watch.wait_for_change(&self.wake, &self.cancel, deadline) {
                    return Err(CLOSED.into());
                }
                continue;
            }
            self.wait(Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())))?;
        }
        Err(UNKNOWN_CONFIG.into())
    }
}

use super::*;
use crate::transport::notifications::recovery::RecoveryWatch;

impl Gateway {
    pub(super) fn reconnect_port(
        &mut self,
        watch: Option<&mut RecoveryWatch>,
        serial: &str,
        vendor: &Option<String>,
        product: &Option<String>,
    ) -> Result<Option<PortInfo>, String> {
        let matches = |port: &PortInfo| {
            port.serial_number.as_deref().is_some_and(|value| value.eq_ignore_ascii_case(serial))
                && &port.vendor_id == vendor
                && &port.product_id == product
        };
        if let Some(cached) = watch.and_then(RecoveryWatch::cached_ports) {
            if !self.record_ports(cached).iter().any(matches) {
                return Ok(None);
            }
        }
        // A committed arrival tells us when to try. Recheck the live port
        // metadata before open, then the caller verifies the UID on the wire.
        Ok(self.ports()?.into_iter().find(matches))
    }

    pub(super) fn wait_for_reconnect(
        &self,
        watch: Option<&RecoveryWatch>,
        missing: bool,
        deadline: Instant,
    ) -> Result<(), String> {
        if let Some(watch) = watch.filter(|watch| missing && watch.covered()) {
            return if watch.wait_for_change(&self.recovery_wake, &self.cancel, deadline) {
                Ok(())
            } else {
                Err(CLOSED.into())
            };
        }
        self.sleep(self.timing.poll.min(deadline.saturating_duration_since(Instant::now())))
    }
}

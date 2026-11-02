//! Passive raw waits and serialized, validated command dispatch.
use super::*;

impl Transport {
    pub(crate) fn capture_origin(&self) -> Instant { self.receiver.shared.origin }
    pub(crate) fn wait_captured(
        &self,
        timeout: Duration,
    ) -> Result<Option<CapturedDatagram>, String> {
        let until = Instant::now()
            .checked_add(timeout)
            .ok_or("Invalid DCS export wait timeout.")?;
        self.wait_captured_until(Some(until))
    }
    /// No deadline means an indefinite native wait without idle polling.
    pub(crate) fn wait_captured_until(
        &self,
        until: Option<Instant>,
    ) -> Result<Option<CapturedDatagram>, String> {
        let shared = &self.receiver.shared;
        let mut state = shared.lock();
        loop {
            if let Some(frame) = Shared::pop(&mut state, self.subscriber)? {
                return Ok(Some(frame));
            }
            state = if let Some(until) = until {
                let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                    return Ok(None);
                };
                shared
                    .changed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(|p| p.into_inner())
                    .0
            } else {
                shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(|p| p.into_inner())
            };
        }
    }
    /// Cancels only this raw queue, without interrupting acquisition siblings.
    pub(crate) fn interrupt_capture(&self) {
        let shared = &self.receiver.shared;
        if let Some(queue) = shared.lock().subscribers.get_mut(&self.subscriber) {
            *queue = Err("DCS export reader interrupted.".into());
        }
        shared.changed.notify_all();
    }
    pub fn send(&self, command: &str) -> Result<(), String> {
        if !matches!(command, "UFC_MASTER_CAUTION 0\n" | "UFC_MASTER_CAUTION 1\n") {
            return Err("Invalid DCS-BIOS master-caution command.".into());
        }
        self.send_bytes(command)
    }
    pub(crate) fn send_control(&self, command: &str, argument: &str) -> Result<(), String> {
        self.send_control_port(command, argument, self.command.port())
    }
    pub(crate) fn send_control_port(
        &self,
        command: &str,
        argument: &str,
        port: u16,
    ) -> Result<(), String> {
        self.send_control_checked(command, argument, port, None)
    }
    /// Submit only if every acquired report has reached the canonical decoder
    /// commit. This guard remains held through the nonblocking UDP dispatch.
    pub(crate) fn send_control_port_after(
        &self,
        command: &str,
        argument: &str,
        port: u16,
        processed_head: u64,
    ) -> Result<(), String> {
        self.send_control_checked(command, argument, port, Some(processed_head))
    }
    fn send_control_checked(
        &self,
        command: &str,
        argument: &str,
        port: u16,
        processed_head: Option<u64>,
    ) -> Result<(), String> {
        if port == 0
            || command.is_empty()
            || command.len() > 64
            || !command
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !(matches!(argument, "INC" | "DEC")
                || argument
                    .parse::<i32>()
                    .is_ok_and(|n| (-65535..=65535).contains(&n))
                    && argument.len() <= 6
                    && argument
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b == b'+' || b == b'-'))
        {
            return Err("Invalid catalogue control command.".into());
        }
        self.send_bytes_after(
            &format!("{command} {argument}\n"),
            SocketAddrV4::new(*self.command.ip(), port),
            processed_head,
        )
    }
    fn send_bytes(&self, command: &str) -> Result<(), String> {
        self.send_bytes_to(command, self.command)
    }
    fn send_bytes_to(&self, command: &str, destination: SocketAddrV4) -> Result<(), String> {
        self.send_bytes_after(command, destination, None)
    }
    fn send_bytes_after(
        &self,
        command: &str,
        destination: SocketAddrV4,
        processed_head: Option<u64>,
    ) -> Result<(), String> {
        // Queue loss revokes authority even before this reader polls. Serialize
        // validation and the nonblocking dispatch with native loss commits.
        let state = self.receiver.shared.lock();
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        state
            .subscribers
            .get(&self.subscriber)
            .ok_or("DCS export subscriber closed.")?
            .as_ref()
            .map_err(|error| error.clone())?;
        if processed_head.is_some_and(|head| head != state.sequence) {
            // A transient rejection is never queued or retried here. Routing
            // scheduler work (R4) owns any later decision to submit again.
            return Err("DCS decode has pending native data; command was not submitted.".into());
        }
        let written = self
            .sender
            .send_to(command.as_bytes(), destination)
            .map_err(|e| format!("DCS-BIOS command send failed: {e}"))?;
        drop(state);
        if written != command.len() {
            return Err("DCS-BIOS command send was incomplete and will not be retried.".into());
        }
        Ok(())
    }
}

use super::*;

impl Desktop {
    /// Desktop rules keep an isolated reducer after focus changes. Its timers
    /// and the action scheduler share the native admission clock, but can be
    /// earlier than the canonical reducer's next deadline.
    pub fn next_deadline_us(&self) -> Option<Counter> {
        if !self.started { return None; }
        [self.focused.then(|| self.scheduler.next_deadline_us()).flatten(),
            self.focused.then(|| self.shadow.as_ref().and_then(Phase::next_deadline_us)).flatten(),
            self.authorization_deadline]
            .into_iter().flatten().min_by_key(|deadline| deadline.0)
    }
}

#[cfg(test)]
#[path = "deadline_tests.rs"]
mod tests;

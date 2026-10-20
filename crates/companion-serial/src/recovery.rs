//! Reconnect policy for an existing assignment after a transient USB failure.
//! These diagnostics grant no authority; the normal authenticated open and fresh
//! neutral-baseline handshake still perform every recovery.
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const STABLE_HEALTH: Duration = Duration::from_secs(10);

/// Only an identity-bound transport failure permits automatic retry. Other
/// retryable firmware faults include touch, display, and setup failures.
pub(super) fn transport_failure(device: &Value) -> bool {
    super::safe_panel_operation(device).is_some_and(|operation| {
        operation.get("sessionId").is_some()
            && operation.get("generation").is_some()
            && operation["phase"] == "failed"
            && operation["retryable"] == true
            && matches!(operation["errorCode"].as_str(),
                Some("request-failed" | "preparation-heartbeat-failed" | "image-download-failed"))
    })
}

#[derive(Default)]
pub(super) struct Retry {
    target: Option<String>,
    attempts: u32,
    retry_at: Option<Instant>,
    healthy_since: Option<Instant>,
    connecting: bool,
}

impl Retry {
    pub(super) fn observe(
        &mut self,
        now: Instant,
        target: Option<&str>,
        recoverable: bool,
        healthy: bool,
    ) {
        if self.target.as_deref() != target {
            self.reset();
            self.target = target.map(str::to_owned);
        }
        if target.is_none() {
            return;
        }
        if healthy {
            self.connecting = false;
            self.retry_at = None;
            let since = *self.healthy_since.get_or_insert(now);
            if now.saturating_duration_since(since) >= STABLE_HEALTH {
                self.attempts = 0;
            }
        } else {
            self.healthy_since = None;
            if recoverable {
                self.connecting = false;
            }
            if recoverable && self.retry_at.is_none() {
                self.retry_at = Some(now + delay(self.attempts));
            }
        }
    }

    /// The caller must also require a currently recoverable, unchanged
    /// assignment. Panel preparation retains the deadline without authorizing a
    /// second connect command while the first one is still in progress.
    pub(super) fn due(&self, now: Instant) -> bool {
        self.target.is_some() && self.retry_at.is_some_and(|at| now >= at)
    }

    pub(super) fn attempted(&mut self, now: Instant) {
        if self.target.is_none() {
            return;
        }
        self.healthy_since = None;
        self.connecting = true;
        self.attempts = self.attempts.saturating_add(1);
        self.retry_at = Some(now + delay(self.attempts));
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn view(&self, now: Instant) -> Option<Value> {
        self.target.as_ref()?;
        let (phase, next, message) = if self.healthy_since.is_some() && self.attempts > 0 {
            ("recovered", 0, "USB connection restored")
        } else if self.connecting {
            ("connecting", 0, "Reconnecting USB and restoring the active panel")
        } else if let Some(at) = self.retry_at {
            if now >= at {
                ("connecting", 0, "Reconnecting USB and restoring the active panel")
            } else {
                (
                    "waiting",
                    at.saturating_duration_since(now).as_millis().min(u64::MAX as u128) as u64,
                    "USB connection interrupted; retrying automatically",
                )
            }
        } else {
            return None;
        };
        Some(json!({"phase":phase,"attempt":self.attempts,"nextRetryMs":next,"message":message}))
    }
}

fn delay(attempts: u32) -> Duration {
    Duration::from_secs((1u64 << attempts.min(5)).min(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure() -> Value {
        json!({
            "companionId":"screen", "sessionId":"session", "generation":"1",
            "operation":{
                "instanceId":"instance", "publicationHash":format!("sha256:{}", "a".repeat(64)),
                "sessionId":"session", "generation":"1", "phase":"failed",
                "retryable":true, "errorCode":"request-failed"
            }
        })
    }

    #[test]
    fn only_current_transport_request_failures_are_automatic() {
        for code in ["request-failed", "preparation-heartbeat-failed", "image-download-failed"] {
            let mut device = failure();
            device["operation"]["errorCode"] = json!(code);
            assert!(transport_failure(&device), "{code}");
            device["operation"]["retryable"] = json!(false);
            assert!(!transport_failure(&device), "nonretryable {code}");
            device["operation"]["retryable"] = json!(true);
            device["operation"]["sessionId"] = json!("old-session");
            assert!(!transport_failure(&device), "stale {code}");
        }
        for code in [
            "cache_full", "cache_unavailable", "cache_active_corrupt", "cache_invalid",
            "cache_corrupt", "cache_io", "invalid-panel", "invalid-layout", "render-failed",
            "touch-unavailable", "image-download-invalid", "image-dimensions", "image-identity", "image-budget", "cancelled",
            "input-cancelled-or-lease-expired-reconnect-required", "session-counter-exhausted",
            "setup-storage-unavailable", "trusted-clock-invalid",
        ] {
            let mut device = failure();
            device["operation"]["errorCode"] = json!(code);
            assert!(!transport_failure(&device), "{code}");
        }
        for (field, value) in [("retryable", json!(false)), ("phase", json!("ready")),
            ("sessionId", json!("old-session")), ("generation", json!("2"))] {
            let mut device = failure();
            device["operation"][field] = value;
            assert!(!transport_failure(&device), "{field}");
        }
        for field in ["sessionId", "generation"] {
            let mut device = failure();
            device["operation"].as_object_mut().unwrap().remove(field);
            assert!(!transport_failure(&device), "missing operation {field}");
            let mut device = failure();
            device.as_object_mut().unwrap().remove(field);
            assert!(!transport_failure(&device), "missing device {field}");
        }
        let mut device = failure();
        device["operation"]["token"] = json!("must not be copied");
        assert!(!transport_failure(&device));
        assert!(!transport_failure(&json!({})));
    }

    #[test]
    fn retry_uses_bounded_exponential_backoff() {
        let mut now = Instant::now();
        let mut retry = Retry::default();
        assert!(retry.view(now).is_none());
        retry.observe(now, Some("panel"), true, false);
        assert_eq!(retry.view(now).unwrap()["nextRetryMs"], 1000);
        assert!(!retry.due(now + Duration::from_millis(999)));
        now += Duration::from_secs(1);
        assert!(retry.due(now));
        assert_eq!(retry.view(now).unwrap()["phase"], "connecting");
        for (attempt, seconds) in [2, 4, 8, 16, 30, 30, 30].into_iter().enumerate() {
            retry.attempted(now);
            assert_eq!(retry.view(now).unwrap()["phase"], "connecting");
            retry.observe(now, Some("panel"), true, false);
            let view = retry.view(now).unwrap();
            assert_eq!(view["attempt"], (attempt + 1) as u64);
            assert_eq!(view["nextRetryMs"], seconds * 1000);
            assert_eq!(view["phase"], "waiting");
            assert!(!retry.due(now + Duration::from_secs(seconds - 1)));
            now += Duration::from_secs(seconds);
            assert!(retry.due(now));
        }
    }

    #[test]
    fn only_continuous_health_resets_the_attempts() {
        let now = Instant::now();
        let mut retry = Retry::default();
        retry.observe(now, Some("panel"), true, false);
        retry.attempted(now + Duration::from_secs(1));
        retry.observe(now + Duration::from_secs(2), Some("panel"), false, true);
        assert!(!retry.due(now + Duration::from_secs(100)));
        assert_eq!(retry.view(now).unwrap()["phase"], "recovered");
        retry.observe(now + Duration::from_secs(11), Some("panel"), false, true);
        assert_eq!(retry.view(now).unwrap()["attempt"], 1);
        retry.observe(now + Duration::from_secs(11), Some("panel"), false, false);
        retry.observe(now + Duration::from_secs(12), Some("panel"), true, false);
        assert_eq!(retry.view(now + Duration::from_secs(12)).unwrap()["nextRetryMs"], 2000);
        retry.observe(now + Duration::from_secs(13), Some("panel"), false, true);
        retry.observe(now + Duration::from_secs(23), Some("panel"), false, true);
        assert!(retry.view(now + Duration::from_secs(23)).is_none());
        retry.observe(now + Duration::from_secs(24), Some("panel"), true, false);
        assert_eq!(retry.view(now + Duration::from_secs(24)).unwrap()["nextRetryMs"], 1000);
    }

    #[test]
    fn preparation_preserves_backoff_and_target_changes_reset_it() {
        let now = Instant::now();
        let mut retry = Retry::default();
        retry.observe(now, Some("first"), true, false);
        retry.attempted(now + Duration::from_secs(1));
        retry.observe(now + Duration::from_secs(2), Some("first"), false, false);
        assert_eq!(retry.view(now + Duration::from_secs(2)).unwrap()["phase"], "connecting");
        assert!(!retry.due(now + Duration::from_secs(2)));
        assert!(retry.due(now + Duration::from_secs(3)));
        retry.observe(now + Duration::from_secs(2), Some("second"), false, false);
        assert!(retry.view(now).is_none());
        retry.observe(now + Duration::from_secs(2), Some("second"), true, false);
        assert_eq!(retry.view(now + Duration::from_secs(2)).unwrap()["attempt"], 0);
        retry.observe(now + Duration::from_secs(3), None, true, false);
        assert!(retry.view(now).is_none());
        assert!(!retry.due(now + Duration::from_secs(100)));
        retry.attempted(now);
        assert!(retry.view(now).is_none());
        retry.observe(now, Some("second"), true, false);
        retry.reset();
        assert!(retry.view(now).is_none());
        assert!(!retry.due(now + Duration::from_secs(100)));
    }

    #[test]
    fn active_reconnect_metadata_tracks_preparation_failure_and_recovery() {
        let now = Instant::now();
        let mut retry = Retry::default();
        retry.observe(now, Some("panel"), true, false);
        assert_eq!(retry.view(now).unwrap()["phase"], "waiting");
        retry.attempted(now + Duration::from_secs(1));
        assert_eq!(retry.view(now + Duration::from_secs(1)).unwrap()["phase"], "connecting");
        retry.observe(now + Duration::from_secs(2), Some("panel"), false, false);
        assert_eq!(retry.view(now + Duration::from_secs(2)).unwrap()["phase"], "connecting");
        retry.observe(now + Duration::from_secs(2), Some("panel"), true, false);
        let waiting = retry.view(now + Duration::from_secs(2)).unwrap();
        assert_eq!(waiting["phase"], "waiting");
        assert_eq!(waiting["nextRetryMs"], 1000);
        retry.attempted(now + Duration::from_secs(3));
        retry.observe(now + Duration::from_secs(4), Some("panel"), false, false);
        assert_eq!(retry.view(now + Duration::from_secs(4)).unwrap()["phase"], "connecting");
        retry.observe(now + Duration::from_secs(5), Some("panel"), false, true);
        assert_eq!(retry.view(now + Duration::from_secs(5)).unwrap()["phase"], "recovered");
        assert!(!retry.due(now + Duration::from_secs(100)));
    }
}

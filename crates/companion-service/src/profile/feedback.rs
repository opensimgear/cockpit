//! Feedback operations fenced by the exact private profile assignment receipt.
use super::*;

impl Hub {
    pub fn configure_profile_feedback(&self, receipt: &AssignmentReceipt, bindings: &[FeedbackBinding]) -> Result<(), String> {
        self.configure_feedback_authorized(receipt.instance_id(), receipt.publication_hash(), bindings, Some(receipt))
    }
    pub fn profile_feedback_token(&self, receipt: &AssignmentReceipt) -> Result<FeedbackToken, String> {
        self.feedback_token_authorized(receipt.instance_id(), Some(receipt))
    }
    pub fn publish_profile_feedback(&self, receipt: &AssignmentReceipt, token: &FeedbackToken,
        values: &[FeedbackSample], mode: FeedbackMode, now_ms: u64) -> Result<u64, String> {
        self.publish_feedback_authorized(token, values, mode, now_ms, Some(receipt))
    }
    /// Best-effort state presentation with exact owner/session fences. Busy,
    /// expired or revoked snapshots are discarded in favour of the next one.
    pub fn try_publish_profile_feedback(&self, receipt: &AssignmentReceipt, token: &FeedbackToken,
        values: &[FeedbackSample], mode: FeedbackMode, captured: std::time::Instant,
        max_age: std::time::Duration, is_current: impl FnOnce() -> bool) -> Result<Option<u64>, String> {
        self.try_publish_feedback_authorized(token, values, mode, captured, max_age, is_current, Some(receipt))
    }
    pub fn clear_profile_feedback(&self, receipt: &AssignmentReceipt, now_ms: u64) -> Result<(), String> {
        self.clear_feedback_authorized(receipt.instance_id(), receipt.publication_hash(), now_ms, Some(receipt))
    }
    pub fn try_clear_profile_feedback(&self, receipt: &AssignmentReceipt) -> Result<bool, String> {
        self.try_clear_feedback_authorized(receipt)
    }
}

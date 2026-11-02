//! Correlate native datagram capture into the decoder's existing source clock.
use super::ExportSession;
use crate::dcs_bios::CapturedDatagram;
use std::time::Instant;

impl ExportSession {
    pub(crate) fn feed_captured(
        &mut self,
        report: &CapturedDatagram,
        now_ms: u64,
        observed: Instant,
    ) {
        if report.sequence == 0 || report.producer_id.is_empty() {
            self.reset_stream(now_ms);
            return;
        }
        if let Some((producer, sequence)) = &self.capture_cursor {
            if producer == &report.producer_id && report.sequence <= *sequence {
                // Duplicate/late capture cannot renew or replace source evidence.
                self.tick(now_ms);
                return;
            }
            if producer != &report.producer_id || sequence.checked_add(1) != Some(report.sequence) {
                self.reset_stream(now_ms);
            }
        }
        self.capture_cursor = Some((report.producer_id.clone(), report.sequence));
        let Some(age) = observed.checked_duration_since(report.captured_at) else {
            self.reset_stream(now_ms);
            return;
        };
        let age_ms = u64::try_from(age.as_millis()).unwrap_or(u64::MAX);
        if age_ms > self.freshness_ms {
            // An old queued full export must not revive a dead simulator, even
            // when a consumer's synthetic clock cannot represent the older time.
            self.reset_stream(now_ms);
            return;
        }
        let Some(captured_ms) = now_ms.checked_sub(age_ms) else {
            self.reset_stream(now_ms);
            return;
        };
        self.feed(&report.bytes, captured_ms);
        self.tick(now_ms);
    }
}

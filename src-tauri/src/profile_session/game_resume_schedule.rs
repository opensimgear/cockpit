//! One recovery attempt per committed Windows readiness revision.

const RETRY_US: u64 = 500_000;

#[derive(Default)]
pub(super) struct Schedule {
    observed: Option<u64>,
    retry_at: Option<u64>,
}

impl Schedule {
    pub(super) fn begin(&mut self, revision: Option<u64>, now: u64) -> bool {
        if let Some(deadline) = self.retry_at {
            if now < deadline {
                return false;
            }
        } else if revision.is_some() && revision == self.observed {
            return false;
        }
        self.observed = revision;
        // Other platforms have no committed discovery notification.
        self.retry_at = revision.is_none().then(|| now.saturating_add(RETRY_US));
        true
    }

    pub(super) fn failed(&mut self, now: u64) {
        self.retry_at = Some(now.saturating_add(RETRY_US));
    }

    pub(super) fn deadline(&self) -> Option<u64> {
        self.retry_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_for_shared_readiness_has_no_deadline() {
        let mut schedule = Schedule::default();
        assert!(schedule.begin(Some(4), 0));
        assert!(!schedule.begin(Some(4), 2 * RETRY_US));
        assert_eq!(schedule.deadline(), None);
        assert!(schedule.begin(Some(5), 2 * RETRY_US));
        assert!(!schedule.begin(Some(5), 2 * RETRY_US));
    }

    #[test]
    fn an_admission_failure_retries_at_its_deadline() {
        let mut schedule = Schedule::default();
        assert!(schedule.begin(Some(3), 0));
        schedule.failed(10);
        assert_eq!(schedule.deadline(), Some(10 + RETRY_US));
        assert!(!schedule.begin(Some(4), 9 + RETRY_US));
        assert!(schedule.begin(Some(4), 10 + RETRY_US));
        assert_eq!(schedule.deadline(), None);
    }

    #[test]
    fn an_unobserved_platform_keeps_its_existing_retry() {
        let mut schedule = Schedule::default();
        assert!(schedule.begin(None, 0));
        assert!(!schedule.begin(None, RETRY_US - 1));
        assert!(schedule.begin(None, RETRY_US));
        assert_eq!(schedule.deadline(), Some(2 * RETRY_US));
    }
}

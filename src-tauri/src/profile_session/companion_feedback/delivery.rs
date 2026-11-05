//! Bounded latest-value delivery, with publication receipts independent of capture.
use osg_companion_service::{FeedbackMode, FeedbackSample, FeedbackStatus};
use std::{collections::{BTreeMap, BTreeSet}, time::{Duration, Instant}};

const CHANNEL_LIMIT: usize = 64;
const COOLDOWN: Duration = super::MIN_PUBLISH_INTERVAL;
const RENEWAL: Duration = Duration::from_millis(750);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stamp {
    pub sample: FeedbackSample,
    pub captured_us: Option<u64>,
    pub captured_sequence: u64,
}

pub(crate) struct Entry {
    pub stamp: Stamp,
    pub sample: FeedbackSample,
    pub age_due: Option<Instant>,
}

pub(crate) struct Projection {
    pub entries: Vec<Entry>,
    pub captured: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct Receipt {
    stamps: Vec<Stamp>,
    snapshot: bool,
}

pub(crate) struct Publication {
    pub samples: Vec<FeedbackSample>,
    pub captured: Instant,
    pub mode: FeedbackMode,
    pub receipt: Receipt,
}

#[derive(Default)]
pub(crate) struct Delivery {
    latest: Option<Projection>,
    confirmed: BTreeMap<String, (Stamp, Instant)>,
    initialized: bool,
}

impl Delivery {
    pub(crate) fn update(&mut self, projection: Projection) -> Result<(), String> {
        if projection.entries.len() > CHANNEL_LIMIT {
            return Err("The companion feedback projection exceeds 64 channels.".into());
        }
        let channels = projection.entries.iter().map(|entry| entry.stamp.sample.channel_id.as_str())
            .collect::<BTreeSet<_>>();
        if channels.len() != projection.entries.len() || projection.entries.iter()
            .any(|entry| entry.stamp.sample.channel_id != entry.sample.channel_id) {
            return Err("The companion feedback projection has invalid channel identities.".into());
        }
        self.confirmed.retain(|channel, _| channels.contains(channel.as_str()));
        self.latest = Some(projection);
        Ok(())
    }

    pub(crate) fn reset(&mut self) { *self = Self::default(); }

    fn changed(&self, entry: &Entry) -> bool {
        self.confirmed.get(&entry.stamp.sample.channel_id)
            .is_none_or(|(stamp, _)| stamp != &entry.stamp)
    }

    fn renewal(&self, entry: &Entry) -> Option<Instant> {
        (entry.sample.status == FeedbackStatus::Fresh).then(|| {
            self.confirmed.get(&entry.stamp.sample.channel_id).map(|(_, at)| *at + RENEWAL)
        }).flatten()
    }

    pub(crate) fn deadline(&self, last_attempt: Option<Instant>) -> Option<Instant> {
        let projection = self.latest.as_ref()?;
        let dirty = !self.initialized || projection.entries.iter().any(|entry| self.changed(entry));
        let due = dirty.then_some(projection.captured).into_iter()
            .chain(projection.entries.iter().filter_map(|entry| self.renewal(entry)))
            .chain(projection.entries.iter().filter_map(|entry| entry.age_due)).min()?;
        Some(last_attempt.map_or(due, |at| due.max(at + COOLDOWN)))
    }

    pub(crate) fn prepare(&self, now: Instant, last_attempt: Option<Instant>) -> Option<Publication> {
        if now < self.deadline(last_attempt)? { return None; }
        let projection = self.latest.as_ref()?;
        let snapshot = !self.initialized;
        let entries = projection.entries.iter().filter(|entry| snapshot || self.changed(entry)
            || self.renewal(entry).is_some_and(|due| now >= due)).collect::<Vec<_>>();
        if !snapshot && entries.is_empty() { return None; }
        Some(Publication {
            samples: entries.iter().map(|entry| entry.sample.clone()).collect(),
            captured: projection.captured,
            mode: if snapshot { FeedbackMode::Snapshot } else { FeedbackMode::Delta },
            receipt: Receipt { stamps: entries.iter().map(|entry| entry.stamp.clone()).collect(), snapshot },
        })
    }

    pub(crate) fn confirm(&mut self, receipt: Receipt, at: Instant) {
        let Some(projection) = &self.latest else { return; };
        for stamp in receipt.stamps {
            if projection.entries.iter().any(|entry| entry.stamp.sample.channel_id == stamp.sample.channel_id) {
                self.confirmed.insert(stamp.sample.channel_id.clone(), (stamp, at));
            }
        }
        self.initialized |= receipt.snapshot;
    }
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;

//! Shared native decode state; catalogue projections retain their own authority.
use super::{Catalogue, IntegerOutput, Memory};
use std::{sync::Arc, time::{Duration, Instant}};

#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub counter: IntegerOutput,
    pub aircraft_address: u16,
    pub aircraft_length: usize,
    pub freshness_ms: u64,
}
impl Spec {
    pub fn from_catalogue(catalogue: &Catalogue, freshness_ms: u64) -> Result<Self, String> {
        if !(100..=10_000).contains(&freshness_ms)
            || catalogue.aircraft_length == 0
            || catalogue.aircraft_address as usize + catalogue.aircraft_length > 65_536 {
            return Err("Invalid bounded DCS decode specification.".into());
        }
        Ok(Self { counter: catalogue.counter.clone(), aircraft_address: catalogue.aircraft_address,
            aircraft_length: catalogue.aircraft_length, freshness_ms })
    }
    pub fn key(&self) -> String { format!("{self:?}") }
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub producer_id: String,
    pub epoch: u64,
    pub capture_sequence: u64,
    pub aircraft: Option<String>,
    pub counter: Option<u16>,
    pub captured_at: Option<Instant>,
    pub freshness_ms: u64,
    pub status: &'static str,
    pub malformed_frames: u64,
    pub error: Option<String>,
    pub memory: Arc<Memory>,
}
impl Snapshot {
    pub fn age_ms(&self, now: Instant) -> Option<u64> {
        self.captured_at.and_then(|at| now.checked_duration_since(at))
            .map(|age| u64::try_from(age.as_millis()).unwrap_or(u64::MAX))
    }
    pub fn fresh_at(&self, now: Instant) -> bool {
        self.status == "live" && self.aircraft.is_some()
            && self.age_ms(now).is_some_and(|age| age <= self.freshness_ms)
    }
    pub fn deadline(&self) -> Option<Instant> {
        (self.status == "live" || self.status == "waiting")
            .then_some(self.captured_at).flatten()
            .and_then(|at| at.checked_add(Duration::from_millis(self.freshness_ms + 1)))
    }
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedSnapshot").field("producer_id", &self.producer_id)
            .field("epoch", &self.epoch).field("counter", &self.counter)
            .field("capture_sequence", &self.capture_sequence).field("status", &self.status)
            .field("aircraft", &self.aircraft).field("error", &self.error).finish()
    }
}
impl osg_streams::RetainedBytes for Snapshot {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + std::mem::size_of::<Memory>()
            + self.memory.bytes.capacity() + self.memory.valid.capacity()
            + self.producer_id.capacity()
            + self.aircraft.as_ref().map_or(0, String::capacity)
            + self.error.as_ref().map_or(0, String::capacity)
    }
}

mod state;
pub(crate) use state::State;
mod source;
pub(crate) use source::Source;

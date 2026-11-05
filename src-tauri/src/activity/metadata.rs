//! Scoped metadata captures. Producers only advance a retained wake epoch;
//! capture, diffing and storage belong to the Activity writer.
use super::{capture, observations, Observe, Snapshot};
use osg_streams::NativeWake;
use std::{sync::Arc, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group { Games, Profiles, Devices, Companions }
impl Group {
    pub(super) const ALL: [Self; 4] = [Self::Games, Self::Profiles, Self::Devices, Self::Companions];
    pub(super) fn kinds(self) -> &'static [&'static str] {
        match self {
            Self::Games => &["game", "game_process", "game_session", "game_vehicle", "integration", "discovery"],
            Self::Profiles => &["profile"],
            Self::Devices => &["gateway", "module", "controller"],
            Self::Companions => &["companion"],
        }
    }
    pub(super) fn for_event(kind: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.kinds().iter().any(|prefix|
            kind.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('_'))))
    }
    pub(super) fn bit(self) -> u8 { 1 << self as u8 }
}

pub(super) struct Signals([Arc<NativeWake>; 4]);
impl Signals {
    pub(super) fn new(parent: Arc<NativeWake>) -> Self {
        Self(std::array::from_fn(|_| Arc::new(NativeWake::child(parent.clone()))))
    }
    pub(super) fn get(&self, group: Group) -> &Arc<NativeWake> { &self.0[group as usize] }
}

pub(super) struct Source {
    pub(super) group: Option<Group>,
    wake: Arc<NativeWake>,
    observed: Option<u64>,
    capture: Observe,
    previous: Option<Snapshot>,
}
impl Source {
    pub(super) fn new(group: Option<Group>, wake: Arc<NativeWake>, capture: Observe) -> Self {
        Self { group, wake, observed: None, capture, previous: None }
    }
    pub(super) fn take(&mut self, force: bool) -> Option<observations::Batch> {
        let observed = self.wake.observed();
        if !force && observed != u64::MAX && self.observed == Some(observed) { return None; }
        // Record before capture. A racing commit must trigger another capture,
        // including when it arrives after the worker's outer wake observation.
        self.observed = Some(observed);
        let started = Instant::now();
        let mut current = (self.capture)();
        if let Some(previous) = &self.previous { current.preserve_unavailable(previous); }
        let events = self.previous.as_ref().map_or_else(|| capture::initial(&current),
            |previous| capture::diff(previous, &current));
        let kinds: Vec<_> = self.group.map_or_else(|| Group::ALL.into_iter().flat_map(Group::kinds).copied().collect(),
            |group| group.kinds().to_vec());
        let available: Vec<_> = kinds.into_iter().filter(|kind| current.available(kind)).collect();
        let batch = observations::Batch::events(events, &current, started).acknowledge(&available);
        self.previous = Some(current);
        Some(batch)
    }
}

#[cfg(test)]
mod tests;

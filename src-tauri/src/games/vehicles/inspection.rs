use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Weak,
    time::{Duration, Instant},
};
use crate::observations::TelemetryLease;

const MAX_CLIENTS: usize = 16;
const MAX_CHANNELS: usize = 512;
const LEASE: Duration = Duration::from_secs(5);

struct Lease {
    expires: Instant,
    observation: Option<Weak<TelemetryLease>>,
    channels: Vec<String>,
}

impl Lease {
    fn active(&self, now: Instant) -> bool {
        self.observation.as_ref().map_or(self.expires > now, |lease| lease.strong_count() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewal_and_rejection_preserve_independent_demands() {
        let now = Instant::now();
        let mut demand = Demand::default();
        demand.update("first", vec!["one".into()], now, None, |_| Ok(())).unwrap();
        demand.update("second", vec!["two".into()], now, None, |_| Ok(())).unwrap();
        let renewed = now + Duration::from_secs(1);
        demand.update("second", vec!["two".into()], renewed, None, |_| panic!("same selection")).unwrap();
        assert!(demand.update("second", vec!["three".into()], renewed, None, |_| Err("rejected".into())).is_err());
        assert_eq!(demand.selection(renewed), ["one", "two"]);
        assert_eq!(demand.deadline(renewed), Some(now + LEASE));
        assert_eq!(demand.selection(now + LEASE), ["two"]);
        assert!(demand.selection(renewed + LEASE).is_empty());
        assert_eq!(demand.deadline(renewed + LEASE), Some(renewed + LEASE));
        demand.pending(false);
        assert_eq!(demand.deadline(renewed + LEASE), None);
    }

    #[test]
    fn observation_selection_survives_quiet_capture_and_releases_on_close() {
        let now = Instant::now();
        let observation = std::sync::Arc::new(TelemetryLease::default());
        let lease = std::sync::Arc::downgrade(&observation);
        let mut demand = Demand::default();
        demand.update("observer", vec!["observed".into()], now, Some(lease.clone()), |_| Ok(())).unwrap();
        let later = now + LEASE;
        demand.update("explicit", vec!["explicit".into()], later, None, |_| Ok(())).unwrap();
        assert_eq!(demand.selection(later), ["explicit", "observed"]);
        drop(observation);
        assert_eq!(demand.selection(later), ["explicit"]);
        assert!(demand.update("observer", vec!["late".into()], later, Some(lease), |_| panic!("closed observation")).is_err());
    }

    #[test]
    fn bounds_reject_without_displacing_live_clients() {
        let now = Instant::now();
        let mut demand = Demand::default();
        let full = (0..MAX_CHANNELS).map(|index| index.to_string()).collect();
        demand.update("first", full, now, None, |_| Ok(())).unwrap();
        assert!(demand.update("second", vec!["overflow".into()], now, None, |_| panic!("over channel limit")).is_err());
        for index in 1..MAX_CLIENTS {
            demand.update(&format!("client-{index}"), vec!["0".into()], now, None, |_| panic!("same union")).unwrap();
        }
        assert!(demand.update("overflow", vec!["0".into()], now, None, |_| panic!("over client limit")).is_err());
        assert!(demand.update("", vec!["0".into()], now, None, |_| panic!("invalid identity")).is_err());
        assert_eq!(demand.selection(now).len(), MAX_CHANNELS);
        demand.update("replacement", vec!["new".into()], now + LEASE, None, |_| Ok(())).unwrap();
        assert_eq!(demand.selection(now + LEASE), ["new"]);
    }
}

#[derive(Default)]
pub(super) struct Demand {
    clients: BTreeMap<String, Lease>,
    selection: Vec<String>,
    pending: bool,
}

impl Demand {
    pub(super) fn update(
        &mut self,
        client: &str,
        channels: Vec<String>,
        now: Instant,
        observation: Option<Weak<TelemetryLease>>,
        apply: impl FnOnce(&[String]) -> Result<(), String>,
    ) -> Result<(), String> {
        if client.is_empty() || client.len() > 128 || client.chars().any(char::is_control) {
            return Err("Invalid telemetry inspection client.".into());
        }
        if observation.as_ref().is_some_and(|lease| lease.strong_count() == 0) {
            return Err("The telemetry observation has closed.".into());
        }
        let others = self.clients.iter()
            .filter(|(id, lease)| id.as_str() != client && lease.active(now));
        if !channels.is_empty() && others.clone().count() >= MAX_CLIENTS {
            return Err("Too many telemetry inspection clients for this game.".into());
        }
        let selected: BTreeSet<_> = others
            .flat_map(|(_, lease)| lease.channels.iter())
            .chain(&channels)
            .cloned()
            .collect();
        if selected.len() > MAX_CHANNELS {
            return Err("Inspect at most 512 X-Plane components per game.".into());
        }
        let selected = selected.into_iter().collect::<Vec<_>>();
        if self.selection != selected {
            apply(&selected)?;
            self.selection = selected;
        }
        self.clients.retain(|_, lease| lease.active(now));
        if channels.is_empty() {
            self.clients.remove(client);
        } else {
            self.clients.insert(client.into(), Lease { expires: now + LEASE, observation, channels });
        }
        Ok(())
    }

    pub(super) fn selection(&mut self, now: Instant) -> &[String] {
        let count = self.clients.len();
        self.clients.retain(|_, lease| lease.active(now));
        if count != self.clients.len() {
            let previous = self.selection.len();
            self.selection.retain(|id| self.clients.values().any(|lease| lease.channels.contains(id)));
            self.pending |= previous != self.selection.len();
        }
        &self.selection
    }

    pub(super) fn deadline(&self, now: Instant) -> Option<Instant> {
        if self.pending { return Some(now); }
        self.clients.values().filter_map(|lease| match &lease.observation {
            None => Some(lease.expires),
            Some(observation) if observation.strong_count() == 0 => Some(now),
            Some(_) => None,
        }).min()
    }

    pub(super) fn pending(&mut self, pending: bool) { self.pending = pending; }
}

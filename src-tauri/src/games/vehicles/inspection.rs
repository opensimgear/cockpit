use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const MAX_CLIENTS: usize = 16;
const MAX_CHANNELS: usize = 512;
const LEASE: Duration = Duration::from_secs(5);

struct Lease {
    expires: Instant,
    channels: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewal_and_rejection_preserve_independent_demands() {
        let now = Instant::now();
        let mut demand = Demand::default();
        demand.update("first", vec!["one".into()], now, |_| Ok(())).unwrap();
        demand.update("second", vec!["two".into()], now, |_| Ok(())).unwrap();
        let renewed = now + Duration::from_secs(1);
        demand.update("second", vec!["two".into()], renewed, |_| panic!("same selection")).unwrap();
        assert!(demand.update("second", vec!["three".into()], renewed, |_| Err("rejected".into())).is_err());
        assert_eq!(demand.selection(renewed), ["one", "two"]);
        assert_eq!(demand.selection(now + LEASE), ["two"]);
        assert!(demand.selection(renewed + LEASE).is_empty());
    }

    #[test]
    fn bounds_reject_without_displacing_live_clients() {
        let now = Instant::now();
        let mut demand = Demand::default();
        let full = (0..MAX_CHANNELS).map(|index| index.to_string()).collect();
        demand.update("first", full, now, |_| Ok(())).unwrap();
        assert!(demand.update("second", vec!["overflow".into()], now, |_| panic!("over channel limit")).is_err());
        for index in 1..MAX_CLIENTS {
            demand.update(&format!("client-{index}"), vec!["0".into()], now, |_| panic!("same union")).unwrap();
        }
        assert!(demand.update("overflow", vec!["0".into()], now, |_| panic!("over client limit")).is_err());
        assert!(demand.update("", vec!["0".into()], now, |_| panic!("invalid identity")).is_err());
        assert_eq!(demand.selection(now).len(), MAX_CHANNELS);
        demand.update("replacement", vec!["new".into()], now + LEASE, |_| Ok(())).unwrap();
        assert_eq!(demand.selection(now + LEASE), ["new"]);
    }
}

#[derive(Default)]
pub(super) struct Demand {
    clients: BTreeMap<String, Lease>,
    selection: Vec<String>,
}

impl Demand {
    pub(super) fn update(
        &mut self,
        client: &str,
        channels: Vec<String>,
        now: Instant,
        apply: impl FnOnce(&[String]) -> Result<(), String>,
    ) -> Result<(), String> {
        if client.is_empty() || client.len() > 128 || client.chars().any(char::is_control) {
            return Err("Invalid telemetry inspection client.".into());
        }
        let others = self.clients.iter()
            .filter(|(id, lease)| id.as_str() != client && lease.expires > now);
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
        self.clients.retain(|_, lease| lease.expires > now);
        if channels.is_empty() {
            self.clients.remove(client);
        } else {
            self.clients.insert(client.into(), Lease { expires: now + LEASE, channels });
        }
        Ok(())
    }

    pub(super) fn selection(&mut self, now: Instant) -> &[String] {
        let count = self.clients.len();
        self.clients.retain(|_, lease| lease.expires > now);
        if count != self.clients.len() {
            self.selection.retain(|id| self.clients.values().any(|lease| lease.channels.contains(id)));
        }
        &self.selection
    }
}

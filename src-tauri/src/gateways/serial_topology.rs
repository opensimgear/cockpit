use super::{Mutex, PortInfo, Provider};
use osg_streams::NativeWake;
use std::sync::{Arc, Weak};

type Ports = Result<Vec<PortInfo>, String>;

#[derive(Default)]
pub(super) struct SerialTopology {
    refresh_gate: Mutex<()>,
    current: Mutex<Option<Ports>>,
    inventory_wake: Mutex<Weak<NativeWake>>,
    activity_wake: Mutex<Weak<NativeWake>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateways::Adapter;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn activity_and_inventory_observe_only_changed_cached_results_with_weak_lifetimes() {
        struct SerialProvider(Mutex<Ports>);
        impl Provider for SerialProvider {
            fn ports(&self) -> Ports { self.0.lock().unwrap().clone() }
            fn adapter(&self, _: &str) -> Arc<dyn Adapter> {
                panic!("Metadata observation cannot open a gateway");
            }
        }
        let topology = SerialTopology::default();
        let provider = SerialProvider(Mutex::new(Ok(vec![])));
        let inventory = Arc::new(NativeWake::default());
        let activity = Arc::new(NativeWake::default());
        topology.watch_inventory(&inventory);
        topology.watch_activity(&activity);
        assert_eq!((inventory.observed(), activity.observed()), (1, 1));
        assert_eq!(Arc::strong_count(&activity), 1);
        assert!(topology.cached().is_err());
        topology.refresh(&provider).unwrap();
        assert_eq!((inventory.observed(), activity.observed()), (2, 2));
        topology.refresh(&provider).unwrap();
        assert_eq!((inventory.observed(), activity.observed()), (2, 2));
        *provider.0.lock().unwrap() = Err("unavailable".into());
        assert!(topology.refresh(&provider).is_err());
        assert_eq!(topology.cached().unwrap_err(), "unavailable");
        assert_eq!((inventory.observed(), activity.observed()), (3, 3));
        assert!(topology.refresh(&provider).is_err());
        assert_eq!((inventory.observed(), activity.observed()), (3, 3));
        *provider.0.lock().unwrap() = Err("different failure".into());
        assert!(topology.refresh(&provider).is_err());
        assert_eq!((inventory.observed(), activity.observed()), (4, 4));
        let replacement = Arc::new(NativeWake::default());
        topology.watch_activity(&replacement);
        *provider.0.lock().unwrap() = Ok(vec![]);
        topology.refresh(&provider).unwrap();
        assert_eq!((inventory.observed(), activity.observed(), replacement.observed()), (5, 4, 2));
        drop(replacement);
        *provider.0.lock().unwrap() = Err("gone".into());
        assert!(topology.refresh(&provider).is_err());
        assert!(topology.activity_wake.lock().unwrap().upgrade().is_none());
        assert_eq!(inventory.observed(), 6);
    }

    #[test]
    fn usb_inventory_reuses_discovery_and_observes_failure_and_recovery() {
        struct SerialProvider(AtomicUsize);
        impl Provider for SerialProvider {
            fn ports(&self) -> Ports {
                match self.0.fetch_add(1, Ordering::Relaxed) {
                    0 | 1 | 3 => Ok(Vec::new()),
                    _ => Err("Serial enumeration failed".into()),
                }
            }
            fn adapter(&self, _: &str) -> Arc<dyn Adapter> {
                panic!("Topology discovery cannot open a gateway");
            }
        }
        let provider = Arc::new(SerialProvider(AtomicUsize::new(0)));
        let topology = Arc::new(SerialTopology::default());
        let serial = {
            let provider = provider.clone();
            let topology = topology.clone();
            Arc::new(move || topology.refresh(provider.as_ref()))
        };
        let mut fleet = crate::module_usb_fleet::Fleet::new(serial, true);
        let cache = topology.clone();
        fleet.use_cached_topology(Arc::new(move || cache.cached()));
        let wake = Arc::new(NativeWake::default());
        topology.watch_inventory(&wake);
        let initial = wake.observed();

        assert!(fleet.scan_for_inventory().is_err());
        assert_eq!(provider.0.load(Ordering::Relaxed), 0);
        topology.refresh(provider.as_ref()).unwrap();
        let baseline = wake.observed();
        assert_ne!(baseline, initial);
        topology.refresh(provider.as_ref()).unwrap();
        assert_eq!(wake.observed(), baseline);
        for _ in 0..3 {
            assert!(fleet.scan_for_inventory().unwrap().is_empty());
        }
        assert_eq!(provider.0.load(Ordering::Relaxed), 2);
        assert!(topology.refresh(provider.as_ref()).is_err());
        let failed = wake.observed();
        assert_ne!(failed, baseline);
        assert_eq!(
            fleet.scan_for_inventory().unwrap_err(),
            "Serial enumeration failed"
        );
        fleet.scan().unwrap();
        assert_ne!(wake.observed(), failed);
        assert!(fleet.scan_for_inventory().unwrap().is_empty());
        assert_eq!(provider.0.load(Ordering::Relaxed), 4);
    }
}

impl SerialTopology {
    pub(super) fn watch_activity(&self, wake: &Arc<NativeWake>) {
        *self.activity_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
        wake.notify();
    }

    pub(super) fn watch_inventory(&self, wake: &Arc<NativeWake>) {
        *self.inventory_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
        wake.notify();
    }

    pub(super) fn refresh(&self, provider: &dyn Provider) -> Ports {
        let _refresh = self
            .refresh_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let result = provider.ports();
        let mut current = self.current.lock().unwrap_or_else(|p| p.into_inner());
        if current.as_ref() != Some(&result) {
            *current = Some(result.clone());
            drop(current);
            if let Some(wake) = self.inventory_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
                wake.notify();
            }
            if let Some(wake) = self.activity_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
                wake.notify();
            }
        }
        result
    }

    pub(super) fn cached(&self) -> Ports {
        self.current
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .unwrap_or_else(
                || Err("Native serial discovery has not established a baseline.".into()),
            )
    }
}

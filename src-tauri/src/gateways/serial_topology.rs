use super::{Mutex, PortInfo, Provider};

type Ports = Result<Vec<PortInfo>, String>;

#[derive(Default)]
pub(super) struct SerialTopology {
    refresh_gate: Mutex<()>,
    current: Mutex<Option<Ports>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateways::Adapter;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn usb_inventory_reuses_discovery_and_observes_failure_and_recovery() {
        struct SerialProvider(AtomicUsize);
        impl Provider for SerialProvider {
            fn ports(&self) -> Ports {
                match self.0.fetch_add(1, Ordering::Relaxed) {
                    0 | 2 => Ok(Vec::new()),
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

        assert!(fleet.scan_for_inventory().is_err());
        assert_eq!(provider.0.load(Ordering::Relaxed), 0);
        topology.refresh(provider.as_ref()).unwrap();
        for _ in 0..3 {
            assert!(fleet.scan_for_inventory().unwrap().is_empty());
        }
        assert_eq!(provider.0.load(Ordering::Relaxed), 1);
        assert!(topology.refresh(provider.as_ref()).is_err());
        assert_eq!(
            fleet.scan_for_inventory().unwrap_err(),
            "Serial enumeration failed"
        );
        fleet.scan().unwrap();
        assert!(fleet.scan_for_inventory().unwrap().is_empty());
        assert_eq!(provider.0.load(Ordering::Relaxed), 3);
    }
}

impl SerialTopology {
    pub(super) fn refresh(&self, provider: &dyn Provider) -> Ports {
        let _refresh = self
            .refresh_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let result = provider.ports();
        *self
            .current
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(result.clone());
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

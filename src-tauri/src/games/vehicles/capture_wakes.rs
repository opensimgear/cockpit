use osg_streams::NativeWake;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct CaptureWakes(BTreeMap<&'static str, Arc<NativeWake>>);

impl Default for CaptureWakes {
    fn default() -> Self {
        Self(crate::game_discovery::integration_ids()
            .map(|id| (id, Arc::default()))
            .collect())
    }
}

impl CaptureWakes {
    pub(crate) fn get(&self, integration: &str) -> Option<&Arc<NativeWake>> {
        self.0.get(integration)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&'static str, &Arc<NativeWake>)> {
        self.0.iter().map(|(&id, wake)| (id, wake))
    }

    pub(crate) fn notify_all(&self) {
        for wake in self.0.values() {
            wake.notify();
        }
    }

    #[cfg(test)]
    pub(super) fn shared(wake: Arc<NativeWake>) -> Self {
        Self(crate::game_discovery::integration_ids().map(|id| (id, wake.clone())).collect())
    }
}

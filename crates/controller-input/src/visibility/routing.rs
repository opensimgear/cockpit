use super::Service;
use osg_streams::NativeWake;
use std::sync::Arc;

impl crate::ControllerService {
    /// Register a scoped waiter before starting profile visibility.
    pub fn watch_visibility_activation(&self, wake: &Arc<NativeWake>) {
        self.visibility.watch_activation(wake);
    }

    /// Register the profile scheduler independently of inventory observers.
    pub fn watch_visibility_routing(&self, wake: &Arc<NativeWake>) {
        self.visibility.watch_routing(wake);
    }
}

impl Service {
    pub(super) fn watch_activation(&self, wake: &Arc<NativeWake>) {
        self.worker.lock().unwrap_or_else(std::sync::PoisonError::into_inner).activation = Arc::downgrade(wake);
        self.suppression.watch_activation(wake);
    }

    pub(super) fn watch_routing(&self, wake: &Arc<NativeWake>) {
        let mut worker = self.worker.lock().unwrap_or_else(|error| error.into_inner());
        worker.routing = Arc::downgrade(wake);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_registers_the_exact_profile_wake() {
        let controller = crate::ControllerService::new(None, false);
        let wake = Arc::new(NativeWake::default());
        controller.watch_visibility_routing(&wake);
        let activation = Arc::new(NativeWake::default());
        controller.watch_visibility_activation(&activation);
        let worker = controller.visibility.worker.lock().unwrap();
        assert!(Arc::ptr_eq(&worker.routing.upgrade().unwrap(), &wake));
        assert!(Arc::ptr_eq(&worker.activation.upgrade().unwrap(), &activation));
    }
}

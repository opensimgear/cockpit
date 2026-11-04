use super::Runtime;
use crate::transport::notifications::PortNotifications;
use osg_streams::NativeWake;
use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

const RETRY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct Detection {
    pub(super) wake: Arc<NativeWake>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Detection {
    pub(super) fn stop(&self) {
        self.wake.notify();
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker {
            if worker.thread().id() != thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}

impl Drop for Detection {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Runtime {
    pub fn start_detection(self: &Arc<Self>) {
        let mut worker = self.detection.worker.lock().unwrap_or_else(|p| p.into_inner());
        if worker.is_some() || self.stopped.load(Ordering::Acquire) {
            return;
        }
        let owner = Arc::downgrade(self);
        let wake = self.detection.wake.clone();
        *worker = Some(thread::spawn(move || {
            let _ = wake.bind_thread(thread::current());
            // Subscribe before enumerating so a concurrent port arrival schedules another pass.
            let _ports = PortNotifications::start(wake.clone()).map_err(|error| {
                eprintln!("Cannot watch companion USB ports: {error}");
            }).ok();
            let mut watching_hub = false;
            loop {
                let observed = wake.observed();
                let Some(runtime) = owner.upgrade() else { break };
                if runtime.stopped.load(Ordering::Acquire) {
                    break;
                }
                let retry = match runtime.services() {
                    Ok(services) => {
                        if !watching_hub {
                            services.hub.observe_record_changes(wake.clone());
                            watching_hub = true;
                        }
                        let listener_retry = runtime.start_saved_listener(services).then_some(RETRY);
                        let usb_retry = if crate::companion_panels::ready(&services.hub).unwrap_or(false) {
                            services.usb.reconnect_detected(&services.hub).unwrap_or(Some(RETRY))
                        } else {
                            None
                        };
                        listener_retry.into_iter().chain(usb_retry).min()
                    }
                    Err(error) => {
                        runtime.notify_listener(Err(&error));
                        None
                    }
                };
                drop(runtime);
                wake.wait_scheduled(observed, retry);
            }
        }));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{net::TcpListener, time::Instant};

    #[test]
    fn owner_retries_only_while_saved_listener_is_pending_and_can_stop() {
        let runtime = Arc::new(Runtime::new(None, Arc::new(super::super::CompanionProvider::new(Arc::new(|| 1)))));
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let settings = json!({
            "address":"127.0.0.1", "advertisedHost":"127.0.0.1",
            "port":occupied.local_addr().unwrap().port(), "discovery":false,
        });
        let state = runtime.request(&json!({
            "command":"companion_enable","clientId":"discovery-test","companion":settings,
        })).unwrap();
        assert_eq!(state["listener"]["configuredEnabled"], true);
        assert_eq!(state["listener"]["enabled"], false);
        runtime.start_detection();
        runtime.start_detection();
        drop(occupied);
        let services = runtime.services().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !services.server.status().enabled {
            assert!(Instant::now() < deadline, "Saved listener was not retried");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!runtime.start_saved_listener(services));
        runtime.shutdown();
        assert!(runtime.detection.worker.lock().unwrap().is_none());
        assert!(!services.server.status().enabled);
    }
}

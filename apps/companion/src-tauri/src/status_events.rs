use osg_companion_service::client::ClientRuntime;
use std::{
    sync::{atomic::{AtomicBool, Ordering}, Arc, OnceLock},
    thread::{self, Thread},
};
use tauri::{AppHandle, Emitter};

pub(super) const CHANGED: &str = "companion-status-changed";

#[derive(Clone, Default)]
pub(super) struct StatusEvents(Arc<Delivery>);

#[derive(Default)]
struct Delivery {
    pending: AtomicBool,
    stopped: AtomicBool,
    thread: OnceLock<Thread>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn unacknowledged_delivery_coalesces_native_changes() {
        let path = std::env::temp_dir().join(format!("osg-companion-status-test-{}", std::process::id()));
        let client = ClientRuntime::open(path).unwrap();
        let events = StatusEvents::default();
        let (send, receive) = mpsc::channel();
        let worker = events.start_with(&client, move || send.send(()).is_ok());
        receive.recv_timeout(Duration::from_secs(2)).unwrap();
        client.invalidate("first transition");
        client.invalidate("second transition");
        assert_eq!(receive.recv_timeout(Duration::from_millis(30)), Err(mpsc::RecvTimeoutError::Timeout));
        events.acknowledge();
        receive.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(client.status().last_error.as_deref(), Some("second transition"));
        events.acknowledge();
        assert_eq!(receive.recv_timeout(Duration::from_millis(30)), Err(mpsc::RecvTimeoutError::Timeout));
        events.stop();
        worker.join().unwrap();
    }
}

impl StatusEvents {
    pub(super) fn start(&self, app: AppHandle, client: &ClientRuntime) {
        self.start_with(client, move || app.emit(CHANGED, ()).is_ok());
    }

    fn start_with(&self, client: &ClientRuntime, emit: impl Fn() -> bool + Send + 'static) -> thread::JoinHandle<()> {
        let wake = client.status_wake();
        let delivery = self.0.clone();
        thread::spawn(move || {
            let current = thread::current();
            let _ = wake.bind_thread(current.clone());
            let _ = delivery.thread.set(current);
            let mut sent = None;
            while !delivery.stopped.load(Ordering::Acquire) {
                let observed = wake.observed();
                if sent != Some(observed) && !delivery.pending.swap(true, Ordering::AcqRel) {
                    sent = Some(observed);
                    if !emit() {
                        delivery.pending.store(false, Ordering::Release);
                    }
                }
                if !delivery.stopped.load(Ordering::Acquire) {
                    wake.wait_scheduled(observed, None);
                }
            }
        })
    }

    pub(super) fn acknowledge(&self) {
        if self.0.pending.swap(false, Ordering::AcqRel) {
            if let Some(thread) = self.0.thread.get() {
                thread.unpark();
            }
        }
    }

    pub(super) fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.0.thread.get() {
            thread.unpark();
        }
    }
}

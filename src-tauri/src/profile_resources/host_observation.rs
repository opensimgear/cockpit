use super::{Collect, Ordering, Runtime};
use osg_streams::NativeWake;
use std::{sync::Arc, thread};

pub(super) struct Worker {
    collect: Collect,
    wake: Arc<NativeWake>,
    thread: thread::JoinHandle<()>,
}

impl Runtime {
    pub(crate) fn start_host_inventory(
        &self,
        collect: Collect,
        wake: Arc<NativeWake>,
    ) -> Result<(), String> {
        let mut worker = self.host_worker.lock().unwrap_or_else(|p| p.into_inner());
        if worker.is_some() || self.stop.load(Ordering::Acquire) {
            return Err("Controller inventory cannot start.".into());
        }
        let state = self.state.clone();
        let stopped = self.stop.clone();
        let revisions = self.local_revision.clone();
        let collectors = self.collector_gate.clone();
        let capture = self.capture_gate.clone();
        let persistence = self.persistence_gate.clone();
        let source = collect.clone();
        let signal = wake.clone();
        let thread = thread::Builder::new()
            .name("profile-controller-inventory".into())
            .spawn(move || {
                signal
                    .bind_thread(thread::current())
                    .expect("controller inventory wake has one owner");
                while !stopped.load(Ordering::Acquire) {
                    let observed = signal.observed();
                    {
                        let _collector = collectors.lock().unwrap_or_else(|p| p.into_inner());
                        if stopped.load(Ordering::Acquire) {
                            break;
                        }
                        let revision = revisions.fetch_add(1, Ordering::AcqRel) + 1;
                        let batches = Self::prepare_batches(source(revision));
                        let _capture = capture.lock().unwrap_or_else(|p| p.into_inner());
                        if stopped.load(Ordering::Acquire) {
                            break;
                        }
                        Self::adopt_prepared(&state, batches);
                    }
                    Self::persist(&state, &persistence);
                    while !stopped.load(Ordering::Acquire) && signal.wait_scheduled(observed, None)
                    {
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        *worker = Some(Worker {
            collect,
            wake,
            thread,
        });
        Ok(())
    }

    pub(super) fn host_collector(&self) -> Option<Collect> {
        self.host_worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|worker| worker.collect.clone())
    }

    pub(super) fn stop_host_inventory(&self) {
        let worker = self
            .host_worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(worker) = worker {
            worker.wake.notify();
            let _ = worker.thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn aggregate_inventory_recollects_on_notification_before_recovery() {
        let (sent, received) = mpsc::channel();
        let runtime = Runtime::new(None, Arc::new(move |revision| {
            sent.send(revision).unwrap();
            Vec::new()
        }), true);
        assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        runtime.inventory_wake().notify();
        assert_eq!(received.recv_timeout(Duration::from_millis(750)).unwrap(), 2);
        runtime.shutdown();
    }

    #[test]
    fn controller_inventory_uses_its_own_wake_and_stops_cleanly() {
        let runtime = Runtime::new(
            None,
            Arc::new(|_| panic!("Unexpected aggregate scan")),
            false,
        );
        let wake = Arc::new(NativeWake::default());
        let (sent, received) = mpsc::channel();
        let collect: Collect = Arc::new(move |revision| {
            sent.send(revision).unwrap();
            Vec::new()
        });
        runtime
            .start_host_inventory(collect.clone(), wake.clone())
            .unwrap();
        assert!(runtime.start_host_inventory(collect, wake.clone()).is_err());
        assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        assert_eq!(
            received.recv_timeout(Duration::from_millis(80)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        wake.notify();
        assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
        runtime.shutdown();
        wake.notify();
        assert_eq!(received.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }
}

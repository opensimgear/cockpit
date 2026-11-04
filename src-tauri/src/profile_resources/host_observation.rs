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
        inventory_changed: impl Fn() + Send + 'static,
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
                    inventory_changed();
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
    fn aggregate_inventory_projects_usb_changes_without_restarting_discovery() {
        let (sent, received) = mpsc::channel();
        let projected = sent.clone();
        let deadline = Arc::new(std::sync::Mutex::new(None));
        let schedule = deadline.clone();
        let runtime = Runtime::new_with_projection(
            None,
            Arc::new(move |revision| {
                sent.send((revision, "discovery")).unwrap();
                Vec::new()
            }),
            Arc::new(move |revision| {
                projected.send((revision, "cached")).unwrap();
                Vec::new()
            }),
            Arc::new(move |now, _| super::super::InventorySchedule {
                retry_discovery: false,
                project_at: schedule.lock().unwrap().filter(|at| *at > now),
            }),
            true,
        );
        assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), (1, "discovery"));
        assert!(received.recv_timeout(Duration::from_millis(1100)).is_err());
        *deadline.lock().unwrap() = Some(std::time::Instant::now() + Duration::from_millis(250));
        runtime.projection_wake().notify();
        assert_eq!(received.recv_timeout(Duration::from_millis(750)).unwrap(), (2, "cached"));
        assert_eq!(received.recv_timeout(Duration::from_millis(750)).unwrap(), (3, "cached"));
        runtime.inventory_wake().notify();
        assert_eq!(received.recv_timeout(Duration::from_millis(750)).unwrap(), (4, "discovery"));
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
            .start_host_inventory(collect.clone(), wake.clone(), || {})
            .unwrap();
        assert!(runtime.start_host_inventory(collect, wake.clone(), || {}).is_err());
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

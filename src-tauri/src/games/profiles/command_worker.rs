//! A bounded command lane per game reader. Native validation and dispatch
//! run outside the routing host, with no retries after loss.
use osg_streams::NativeWake;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex, Weak,
};

const QUEUE_CAPACITY: usize = 32;
const WORKER_LIMIT: usize = 16;
static WORKERS: AtomicUsize = AtomicUsize::new(0);

type Job = Box<dyn FnOnce(&AtomicBool) -> Result<(), String> + Send>;

struct Permit;

impl Permit {
    fn acquire() -> Result<Self, String> {
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < WORKER_LIMIT).then_some(count + 1)
            })
            .map_err(|_| "The simulator command worker limit was reached.".to_owned())?;
        Ok(Self)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Default)]
struct State {
    error: Option<String>,
    wake: Option<Weak<NativeWake>>,
}

struct Shared {
    cancelled: AtomicBool,
    state: Mutex<State>,
}

impl Shared {
    fn fail(&self, error: String) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        state.error = Some(error);
        if let Some(wake) = state.wake.as_ref().and_then(Weak::upgrade) {
            wake.notify();
        }
    }

    fn notify(&self) {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !self.cancelled.load(Ordering::Acquire) {
            if let Some(wake) = state.wake.as_ref().and_then(Weak::upgrade) {
                wake.notify();
            }
        }
    }
}

pub(super) struct Worker {
    sender: Option<mpsc::SyncSender<Job>>,
    shared: Arc<Shared>,
}

impl Worker {
    pub(super) fn start(wake: Option<Weak<NativeWake>>) -> Result<Self, String> {
        let permit = Permit::acquire()?;
        let (sender, receiver) = mpsc::sync_channel::<Job>(QUEUE_CAPACITY);
        let shared = Arc::new(Shared {
            cancelled: AtomicBool::new(false),
            state: Mutex::new(State { error: None, wake }),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("simulator-command".into())
            .spawn(move || {
                let _permit = permit;
                while let Ok(job) = receiver.recv() {
                    if worker.cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let result = catch_unwind(AssertUnwindSafe(|| job(&worker.cancelled)));
                    match result {
                        Ok(Ok(())) => worker.notify(),
                        Ok(Err(error)) => {
                            worker.fail(error);
                            break;
                        }
                        Err(_) => {
                            worker.fail("The simulator command worker panicked.".into());
                            break;
                        }
                    }
                }
            })
            .map_err(|error| format!("Cannot start the simulator command worker: {error}"))?;
        Ok(Self {
            sender: Some(sender),
            shared,
        })
    }

    pub(super) fn submit(
        &self,
        job: impl FnOnce(&AtomicBool) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        self.poll()?;
        let sender = self
            .sender
            .as_ref()
            .ok_or("The simulator command worker stopped.")?;
        match sender.try_send(Box::new(job)) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                self.shared.fail(
                    "The simulator command queue overflowed; pending commands were discarded."
                        .into(),
                );
                self.poll()
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.shared
                    .fail("The simulator command worker stopped.".into());
                self.poll()
            }
        }
    }

    pub(super) fn poll(&self) -> Result<(), String> {
        if !self.shared.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        Err(state
            .error
            .clone()
            .unwrap_or_else(|| "The simulator command worker stopped.".into()))
    }

    pub(super) fn set_wake(&self, wake: Option<Weak<NativeWake>>) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .wake = wake;
    }

    pub(super) fn stop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        self.sender = None;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);

    fn wait_for_wake(wake: &NativeWake, count: u64) {
        let deadline = Instant::now() + TIMEOUT;
        while wake.observed() < count {
            assert!(
                Instant::now() < deadline,
                "the command worker did not notify its host"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn stalled_job_preserves_fifo_and_completions_wake_the_host() {
        let wake = Arc::new(NativeWake::default());
        let worker = Worker::start(None).unwrap();
        worker.set_wake(Some(Arc::downgrade(&wake)));
        let (entered, started) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        let first = completed.clone();
        worker
            .submit(move |_| {
                entered.send(()).unwrap();
                held.recv_timeout(TIMEOUT).unwrap();
                first.send(0).unwrap();
                Ok(())
            })
            .unwrap();
        started.recv_timeout(TIMEOUT).unwrap();
        for index in 1..=3 {
            let completed = completed.clone();
            worker
                .submit(move |_| {
                    completed.send(index).unwrap();
                    Ok(())
                })
                .unwrap();
        }
        worker.poll().unwrap();
        assert_eq!(wake.observed(), 0);
        release.send(()).unwrap();
        for index in 0..=3 {
            assert_eq!(results.recv_timeout(TIMEOUT).unwrap(), index);
        }
        wait_for_wake(&wake, 4);
    }

    #[test]
    fn overflow_cancels_in_flight_validation_and_discards_the_queue() {
        let wake = Arc::new(NativeWake::default());
        let worker = Worker::start(Some(Arc::downgrade(&wake))).unwrap();
        let (entered, started) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        worker
            .submit(move |cancelled| {
                entered.send(()).unwrap();
                held.recv_timeout(TIMEOUT).unwrap();
                finished.send(cancelled.load(Ordering::Acquire)).unwrap();
                Ok(())
            })
            .unwrap();
        started.recv_timeout(TIMEOUT).unwrap();
        let (submitted, replay) = mpsc::channel();
        for _ in 0..QUEUE_CAPACITY {
            let submitted = submitted.clone();
            worker
                .submit(move |_| {
                    submitted.send(()).unwrap();
                    Ok(())
                })
                .unwrap();
        }
        drop(submitted);
        let error = worker.submit(|_| Ok(())).unwrap_err();
        assert!(error.contains("overflow"));
        assert_eq!(worker.poll(), Err(error));
        wait_for_wake(&wake, 1);
        release.send(()).unwrap();
        assert!(result.recv_timeout(TIMEOUT).unwrap());
        assert_eq!(
            replay.recv_timeout(TIMEOUT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn stop_is_nonblocking_and_fences_queued_work() {
        let mut worker = Worker::start(None).unwrap();
        let (entered, started) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        worker
            .submit(move |cancelled| {
                entered.send(()).unwrap();
                held.recv_timeout(TIMEOUT).unwrap();
                finished.send(cancelled.load(Ordering::Acquire)).unwrap();
                Ok(())
            })
            .unwrap();
        started.recv_timeout(TIMEOUT).unwrap();
        let (submitted, replay) = mpsc::channel();
        worker
            .submit(move |_| {
                submitted.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        worker.stop();
        assert!(worker.poll().is_err());
        release.send(()).unwrap();
        assert!(result.recv_timeout(TIMEOUT).unwrap());
        assert_eq!(
            replay.recv_timeout(TIMEOUT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn a_command_error_revokes_the_lane_before_the_next_job() {
        let wake = Arc::new(NativeWake::default());
        let worker = Worker::start(Some(Arc::downgrade(&wake))).unwrap();
        let (entered, started) = mpsc::channel();
        let (release, held) = mpsc::channel();
        worker
            .submit(move |_| {
                entered.send(()).unwrap();
                held.recv_timeout(TIMEOUT).unwrap();
                Err("exact game identity changed".into())
            })
            .unwrap();
        started.recv_timeout(TIMEOUT).unwrap();
        let (submitted, replay) = mpsc::channel();
        worker
            .submit(move |_| {
                submitted.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        release.send(()).unwrap();
        wait_for_wake(&wake, 1);
        assert_eq!(worker.poll(), Err("exact game identity changed".into()));
        assert_eq!(
            replay.recv_timeout(TIMEOUT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
    }
}

//! Interruptible exit watch for an already selected exact game process.
use crate::game_discovery::ProcessIdentity;
use osg_streams::NativeWake;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, Weak};

pub(crate) struct ExitWatch {
    state: Arc<State>,
    #[cfg(windows)]
    stop: Arc<super::native::Handle>,
    #[cfg(windows)]
    worker: Option<std::thread::JoinHandle<()>>,
}

struct State {
    current: AtomicBool,
    wake: Mutex<Weak<NativeWake>>,
}

pub(crate) struct ExitEvidence(Weak<State>);
impl ExitEvidence {
    pub(crate) fn current(&self) -> bool {
        self.0.upgrade().is_some_and(|state| state.current.load(Ordering::Acquire))
    }
}

impl ExitWatch {
    #[cfg(all(windows, test))]
    pub(crate) fn current_process_fixture() -> ProcessIdentity {
        super::native::Handle::current_identity()
    }

    pub(crate) fn open(expected: &ProcessIdentity) -> Result<Self, String> {
        #[cfg(windows)]
        {
            use super::native::{wait_events, Handle};
            let process = Handle::process(expected)?;
            let stop = Arc::new(Handle::event()?);
            let state = Arc::new(State { current: AtomicBool::new(true), wake: Mutex::default() });
            let worker_state = state.clone();
            let worker_stop = stop.clone();
            let worker = std::thread::Builder::new().name("game-process-exit".into()).spawn(move || {
                let _ = wait_events(&worker_stop, &process);
                worker_state.current.store(false, Ordering::Release);
                if let Some(wake) = worker_state.wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
                    wake.notify();
                }
            }).map_err(|error| error.to_string())?;
            Ok(Self { state, stop, worker: Some(worker) })
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err("Native game process exit watching is unavailable.".into())
        }
    }

    pub(crate) fn current(&self) -> bool { self.state.current.load(Ordering::Acquire) }

    pub(crate) fn evidence(&self) -> ExitEvidence { ExitEvidence(Arc::downgrade(&self.state)) }

    pub(crate) fn watch(&self, wake: &Arc<NativeWake>) {
        *self.state.wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
        if !self.current() { wake.notify(); }
    }
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;
    use std::{process::{Child, Command, Stdio}, time::{Duration, Instant}};

    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn exact_process_exit_wakes_without_a_query_and_drop_interrupts_wait() {
        let name = format!("{}::parked_child", module_path!().split_once("::").unwrap().1);
        let mut child = OwnedChild(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--ignored"])
            .env("OSG_EXIT_WATCH_CHILD_FIXTURE", "1")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().unwrap());
        let identity = super::super::native::Handle::identity_for_pid(child.0.id());
        let mut replaced = identity.clone();
        replaced.epoch ^= 1;
        assert!(ExitWatch::open(&replaced).is_err());
        let cancelled = ExitWatch::open(&identity).unwrap();
        let cancelled_evidence = cancelled.evidence();
        drop(cancelled);
        assert!(!cancelled_evidence.current());
        assert!(child.0.try_wait().unwrap().is_none());

        let watch = ExitWatch::open(&identity).unwrap();
        let wake = Arc::new(NativeWake::default());
        watch.watch(&wake);
        let observed = wake.observed();
        assert!(watch.current());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while wake.observed() == observed {
            assert!(Instant::now() < deadline, "Exact process exit did not wake its observer");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!watch.current());
        let late = Arc::new(NativeWake::default());
        let observed = late.observed();
        watch.watch(&late);
        assert_ne!(late.observed(), observed);
    }

    #[test]
    #[ignore = "Only launched by the native exit watch test."]
    fn parked_child() {
        if std::env::var_os("OSG_EXIT_WATCH_CHILD_FIXTURE").is_some() { std::thread::park(); }
    }
}

#[cfg(windows)]
impl Drop for ExitWatch {
    fn drop(&mut self) {
        self.stop.signal();
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

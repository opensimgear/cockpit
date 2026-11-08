//! Interruptible exit watch for an already selected exact game process.
use crate::game_discovery::ProcessIdentity;
use osg_streams::NativeWake;
use std::sync::{atomic::{AtomicU8, Ordering}, Arc, Mutex, Weak};

pub(crate) struct ExitWatch {
    state: Arc<State>,
    #[cfg(windows)]
    stop: Arc<super::native::Handle>,
}

struct State {
    process: ProcessIdentity,
    status: AtomicU8,
    wake: Mutex<Weak<NativeWake>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ExitStatus { Watching, Exited, Unavailable }

impl State {
    fn status(&self) -> ExitStatus {
        match self.status.load(Ordering::Acquire) {
            value if value == ExitStatus::Watching as u8 => ExitStatus::Watching,
            value if value == ExitStatus::Exited as u8 => ExitStatus::Exited,
            _ => ExitStatus::Unavailable,
        }
    }
}

pub(crate) struct ExitEvidence(Weak<State>);
impl ExitEvidence {
    pub(crate) fn current(&self) -> bool {
        self.0.upgrade().is_some_and(|state| state.status() == ExitStatus::Watching)
    }

    pub(crate) fn status(&self, expected: &ProcessIdentity) -> ExitStatus {
        self.0.upgrade().filter(|state| &state.process == expected)
            .map_or(ExitStatus::Unavailable, |state| state.status())
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
            let state = Arc::new(State { process: expected.clone(),
                status: AtomicU8::new(ExitStatus::Watching as u8), wake: Mutex::default() });
            let worker_state = state.clone();
            let worker_stop = stop.clone();
            std::thread::Builder::new().name("game-process-exit".into()).spawn(move || {
                let status = if matches!(wait_events(&worker_stop, &process), Ok(true)) {
                    ExitStatus::Exited
                } else {
                    ExitStatus::Unavailable
                };
                let wake = worker_state.wake.lock().unwrap_or_else(|p| p.into_inner());
                if worker_state.status.compare_exchange(ExitStatus::Watching as u8, status as u8,
                    Ordering::AcqRel, Ordering::Acquire).is_ok()
                {
                    if let Some(wake) = wake.upgrade() { wake.notify(); }
                }
            }).map_err(|error| error.to_string())?;
            Ok(Self { state, stop })
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err("Native game process exit watching is unavailable.".into())
        }
    }

    #[cfg(all(windows, test))]
    fn current(&self) -> bool { self.state.status() == ExitStatus::Watching }

    pub(crate) fn exited(&self) -> bool { self.state.status() == ExitStatus::Exited }

    pub(crate) fn evidence(&self) -> ExitEvidence { ExitEvidence(Arc::downgrade(&self.state)) }

    pub(crate) fn watch(&self, wake: Option<&Arc<NativeWake>>) {
        *self.state.wake.lock().unwrap_or_else(|p| p.into_inner()) = wake.map_or_else(Weak::new, Arc::downgrade);
        if self.state.status() != ExitStatus::Watching {
            if let Some(wake) = wake { wake.notify(); }
        }
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
        watch.watch(Some(&wake));
        let observed = wake.observed();
        assert!(watch.current());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while wake.observed() == observed {
            assert!(Instant::now() < deadline, "Exact process exit did not wake its observer");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(watch.exited());
        let late = Arc::new(NativeWake::default());
        let observed = late.observed();
        watch.watch(Some(&late));
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
        self.watch(None);
        self.state.status.store(ExitStatus::Unavailable as u8, Ordering::Release);
        // The waiter owns both handles until it consumes the stop event.
        // Retirement never joins an OS waiter from the routing path.
        self.stop.signal();
    }
}

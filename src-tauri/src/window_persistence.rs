//! Coalesce native move/resize bursts without relying on a clean app exit.
use std::{sync::{mpsc::{self, RecvTimeoutError, SyncSender}, Mutex}, time::Duration};

pub(crate) struct SaveWorker(Mutex<Option<SyncSender<()>>>);

impl SaveWorker {
    pub(crate) fn new(delay: Duration, mut save: impl FnMut() + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while receiver.recv().is_ok() {
                loop {
                    match receiver.recv_timeout(delay) {
                        Ok(()) => {},
                        Err(RecvTimeoutError::Timeout) => { save(); break; },
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
        });
        Self(Mutex::new(Some(sender)))
    }

    pub(crate) fn changed(&self) {
        if let Some(sender) = self.0.lock().unwrap_or_else(|p|p.into_inner()).as_ref() {
            let _ = sender.try_send(());
        }
    }

    pub(crate) fn stop(&self) {
        self.0.lock().unwrap_or_else(|p|p.into_inner()).take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicU32, Ordering}};

    #[test]
    fn resize_bursts_persist_the_latest_width_before_exit_and_again_after_another_resize() {
        let directory=crate::test_directory();
        let path=directory.path().join("window-state.json");
        let width=Arc::new(AtomicU32::new(900));
        let (saved, written)=mpsc::channel();
        let worker=SaveWorker::new(Duration::from_millis(20), {
            let width=width.clone();let path=path.clone();
            move || { let value=width.load(Ordering::SeqCst);std::fs::write(&path,value.to_string()).unwrap();saved.send(value).unwrap(); }
        });
        for value in [1000,1100,1250] { width.store(value,Ordering::SeqCst);worker.changed(); }
        assert_eq!(written.recv_timeout(Duration::from_secs(2)).unwrap(),1250);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),"1250");
        assert!(written.try_recv().is_err());
        width.store(1400,Ordering::SeqCst);worker.changed();
        assert_eq!(written.recv_timeout(Duration::from_secs(2)).unwrap(),1400);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),"1400");
        worker.stop();worker.changed();
        assert!(written.recv_timeout(Duration::from_secs(2)).is_err());
    }

    #[test]
    fn shutting_down_an_idle_worker_does_not_schedule_a_late_save() {
        let (saved,written)=mpsc::channel();
        let worker=SaveWorker::new(Duration::from_millis(20),move || {saved.send(()).unwrap();});
        worker.stop();worker.changed();
        assert!(written.recv_timeout(Duration::from_secs(2)).is_err());
    }
}

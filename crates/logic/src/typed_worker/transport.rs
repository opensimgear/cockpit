//! Pipe threads publish bounded outcomes before signalling the host.
use super::{Reply, Request};
use crate::wire;
use std::{
    io::{Read, Write},
    sync::{
        mpsc::{Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
};

type Wake = Arc<dyn Fn() + Send + Sync>;

pub(super) struct Events {
    failure: Mutex<Option<String>>,
    wake: Mutex<Option<Wake>>,
}

impl Events {
    pub(super) fn new(wake: Wake) -> Self {
        Self {
            failure: Mutex::new(None),
            wake: Mutex::new(Some(wake)),
        }
    }

    pub(super) fn failure(&self) -> Option<String> {
        self.failure.lock().unwrap().clone()
    }

    fn signal(&self) {
        // Never invoke host code with a transport lock held. The callback must
        // only signal a persistent wake token (e.g. Thread::unpark).
        let wake = self.wake.lock().unwrap().clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    fn fail(&self, replies: &SyncSender<Result<Reply, String>>, error: String) {
        let mut failure = self.failure.lock().unwrap();
        if failure.is_some() {
            return;
        }
        *failure = Some(error.clone());
        drop(failure);
        // The separate terminal slot makes write failure observable even if a
        // reply fills the single mailbox. Also unblock the compile handshake.
        let _ = replies.try_send(Err(error));
        self.signal();
    }

    pub(super) fn stop(&self) {
        // Pipe threads may still be blocked in native I/O. They retain this
        // small state, but must not retain the owner's wake callback.
        self.wake.lock().unwrap().take();
    }

    fn publish(&self, replies: &SyncSender<Result<Reply, String>>, reply: Reply) -> bool {
        match replies.try_send(Ok(reply)) {
            Ok(()) => {
                self.signal();
                true
            }
            Err(TrySendError::Full(_)) => {
                self.fail(
                    replies,
                    "Script helper reply mailbox exceeded its capacity.".into(),
                );
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

pub(super) fn write_requests(
    mut input: impl Write,
    requests: Receiver<Request>,
    replies: SyncSender<Result<Reply, String>>,
    events: Arc<Events>,
) {
    while let Ok(request) = requests.recv() {
        if let Err(error) = wire::write(&request, &mut input) {
            events.fail(&replies, error);
            break;
        }
    }
}

pub(super) fn read_replies(
    mut output: impl Read,
    replies: SyncSender<Result<Reply, String>>,
    events: Arc<Events>,
) {
    loop {
        match wire::read(&mut output) {
            Ok(reply) => {
                if !events.publish(&replies, reply) {
                    break;
                }
            }
            Err(error) => {
                events.fail(&replies, error);
                break;
            }
        }
    }
}

#[cfg(test)]
#[path = "wake_tests.rs"]
mod tests;

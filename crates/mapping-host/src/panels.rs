//! Native panel adapters own their event cursors; the mapping clock never blocks on panel I/O.
use osg_controller_input::stream::SourceSnapshot;
use osg_mapping::{
    engine::{Assignment, Frame},
    profile::Profile,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
    Arc,
};
pub trait Provider: Send + Sync {
    fn list(&self) -> Result<serde_json::Value, String>;
    fn inspect(&self, id: &str) -> Result<serde_json::Value, String>;
    fn prepare(&self, profile: &Profile, assignments: &[Assignment]) -> Result<Prepared, String>;
}
pub struct Prepared {
    pub snapshots: Vec<SourceSnapshot>,
    pub feed: Feed,
}
pub struct Feed {
    receiver: Receiver<Frame>,
    pub stop: Arc<AtomicBool>,
    pub overflow: Arc<AtomicBool>,
}
impl Feed {
    pub fn channel() -> (Self, SyncSender<Frame>) {
        let (sender, receiver) = mpsc::sync_channel(512);
        (
            Self {
                receiver,
                stop: Arc::new(AtomicBool::new(false)),
                overflow: Arc::new(AtomicBool::new(false)),
            },
            sender,
        )
    }
    pub fn drain(&mut self) -> Result<Vec<Frame>, String> {
        if self.overflow.load(Ordering::Acquire) {
            return Err("Panel event queue overflow; acquire fresh baselines.".into());
        }
        let mut frames = Vec::new();
        for _ in 0..512 {
            match self.receiver.try_recv() {
                Ok(frame) => frames.push(frame),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err("Panel capture worker stopped.".into())
                }
            }
        }
        Ok(frames)
    }
}
impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_feed_detects_loss_and_cancels_owned_workers() {
        let (mut feed, sender) = Feed::channel();
        assert!(feed.drain().unwrap().is_empty());
        feed.overflow.store(true, Ordering::Release);
        assert!(feed.drain().is_err());
        let stop = feed.stop.clone();
        drop(feed);
        assert!(stop.load(Ordering::Acquire));
        drop(sender);
        let (mut feed, sender) = Feed::channel();
        drop(sender);
        assert!(feed.drain().is_err());
    }
}

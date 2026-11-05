//! Shared, event-driven foreground and interactive-session evidence. Every
//! scoped lease retains a loss epoch, so a quick away-and-back cannot disappear.
use crate::Scope;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, OnceLock, Weak,
};

mod native;
mod state;
mod lifetime;
mod registry;
use state::{Shared, Subscriber};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeSnapshot {
    pub watch_id: u64,
    pub sequence: u64,
    pub loss_epoch: u64,
    pub eligible: bool,
    pub error: Option<String>,
}
type Wake = Arc<dyn Fn(ScopeSnapshot) + Send + Sync>;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Default)]
struct Registry {
    owner: Weak<native::Owner>,
    shared: Weak<Shared>,
}
static OWNER: OnceLock<Mutex<Registry>> = OnceLock::new();

/// A scoped subscription. Neither subscribers nor the native thread retain the
/// owner, allowing the last lease to stop the observer without an Arc cycle.
pub struct ScopeWatch {
    subscriber: Arc<Subscriber>,
    owner: Arc<native::Owner>,
}
impl ScopeWatch {
    pub fn open(scope: Scope, wake: Wake) -> Result<Self, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let id = NEXT_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
            .map_err(|_| "Desktop scope watch identity exhausted.")?;
        let registry = OWNER.get_or_init(|| Mutex::new(Registry::default()));
        let owner = registry::with_available(registry, deadline, |slot, owner| {
            if let Some(owner) = owner { Ok(owner) } else {
                let shared = Arc::new(Shared::new());
                slot.shared = Arc::downgrade(&shared);
                let owner = native::Owner::open(shared, deadline)?;
                slot.owner = Arc::downgrade(&owner);
                Ok(owner)
            }
        })?;
        let subscriber = owner.shared.subscribe(id, scope, wake)?;
        Ok(Self { subscriber, owner })
    }
    pub fn snapshot(&self) -> Result<ScopeSnapshot, String> {
        self.subscriber.snapshot()
    }
    pub fn id(&self) -> u64 {
        self.subscriber.id
    }
}
impl Drop for ScopeWatch {
    fn drop(&mut self) {
        // Retirement precedes the owner's shutdown and its bounded completion
        // wait. A consumer must also fence any already-running wake by watch_id.
        self.owner.shared.unsubscribe(&self.subscriber);
    }
}

#[cfg(test)]
mod tests;

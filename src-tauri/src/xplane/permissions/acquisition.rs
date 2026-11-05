//! A setup command may await direct monitor startup; passive acquisition stays
//! nonblocking. Notifications carry no process authority and never elevate.
use super::*;
use osg_streams::NativeWake;

const MAX_WAITERS: usize = 64;
static WAITERS: OnceLock<Mutex<Vec<Weak<NativeWake>>>> = OnceLock::new();

fn waiters() -> &'static Mutex<Vec<Weak<NativeWake>>> {
    WAITERS.get_or_init(Default::default)
}

pub(super) fn notify() {
    let mut waiters = waiters().lock().unwrap_or_else(PoisonError::into_inner);
    waiters.retain(|weak| {
        if let Some(wake) = weak.upgrade() { wake.notify(); true } else { false }
    });
}

pub(crate) fn acquire_until(
    game: &str,
    expected: &ProcessIdentity,
    wake: Arc<NativeWake>,
    deadline: Instant,
    cancelled: impl Fn() -> bool,
) -> Result<Arc<Monitor>, String> {
    // The child is retained only by this wait. Registration precedes the first
    // status read, so completion cannot fall between inspection and parking.
    let lease = Arc::new(NativeWake::child(wake.clone()));
    {
        let mut waiters = waiters().lock().unwrap_or_else(PoisonError::into_inner);
        waiters.retain(|weak| weak.strong_count() > 0);
        if waiters.len() >= MAX_WAITERS { return Err("Native process startup waiter limit reached.".into()); }
        waiters.push(Arc::downgrade(&lease));
    }
    loop {
        let observed = wake.observed();
        if cancelled() { return Err("Native process monitoring wait was cancelled.".into()); }
        match acquire(game, expected) {
            Ok(monitor) => {
                if cancelled() { return Err("Native process monitoring wait was cancelled.".into()); }
                return Ok(monitor);
            }
            Err(error) => {
                let registry = entries().lock().unwrap_or_else(PoisonError::into_inner);
                let Some(entry) = registry.get(&key(expected)) else { return Err(error); };
                // Re-read the exact entry under one lock. Startup may have
                // completed between acquire's read and its notification.
                if let Some(monitor) = &entry.monitor {
                    if let Some(error) = monitor.error() { return Err(error); }
                    continue;
                }
                if !matches!(entry.state, "checking" | "starting") {
                    return Err(entry.error.clone().unwrap_or(error));
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return Err("Native process monitoring startup timed out. Recheck when its permission check finishes.".into()); }
        wake.wait(observed, remaining);
    }
}

#[cfg(test)]
mod tests;

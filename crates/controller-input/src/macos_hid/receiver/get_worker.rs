//! Native GET ownership survives the caller's deadline and session close.
use super::*;
use std::{
    collections::HashSet,
    sync::{mpsc, Mutex, OnceLock},
};
fn active() -> &'static Mutex<HashSet<u64>> {
    static ACTIVE: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    ACTIVE.get_or_init(Mutex::default)
}
struct Lease(u64);
impl Lease {
    fn acquire(id: u64) -> Result<Self, Value> {
        let mut ids = active().lock().unwrap_or_else(|e| e.into_inner());
        if ids.contains(&id) || ids.len() >= 4 {
            return Err(failure("busy","A native GET is still pending for this registry ID, or global GET capacity is exhausted"));
        }
        ids.insert(id);
        Ok(Self(id))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        active()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}
pub(super) fn read(
    registry: u64,
    timeout: Duration,
    request: impl FnOnce() -> Result<Vec<u8>, Value> + Send + 'static,
) -> Result<Vec<u8>, Value> {
    let deadline = Instant::now() + timeout;
    let lease = Lease::acquire(registry)?;
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("iohid-get".into())
        .spawn(move || {
            let _lease = lease;
            let result = if Instant::now() < deadline {
                request()
            } else {
                Err(failure("timeout", "GET deadline expired before dispatch"))
            };
            let _ = tx.send(result);
        })
        .map_err(|e| failure("io-error", e.to_string()))?;
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(failure(
            "timeout",
            "IOHID GET deadline expired; native worker retains its lease until completion",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(failure(
            "io-error",
            "IOHID GET worker ended without a reply",
        )),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_request_holds_registry_and_global_permits_until_actual_completion() {
        let mut release = Vec::new();
        for id in 100..104 {
            let (tx, rx) = mpsc::channel();
            release.push(tx);
            assert_eq!(
                read(id, Duration::from_millis(100), move || {
                    rx.recv().unwrap();
                    Ok(vec![1])
                })
                .unwrap_err()["kind"],
                "timeout"
            );
        }
        assert_eq!(
            read(100, Duration::from_millis(5), || Ok(vec![])).unwrap_err()["kind"],
            "busy"
        );
        assert_eq!(
            read(105, Duration::from_millis(5), || Ok(vec![])).unwrap_err()["kind"],
            "busy"
        );
        for tx in release {
            tx.send(()).unwrap();
        }
        let started = Instant::now();
        while !active().lock().unwrap().is_empty() {
            assert!(started.elapsed() < Duration::from_secs(1));
            std::thread::yield_now();
        }
        assert_eq!(
            read(100, Duration::from_secs(1), || Ok(vec![2])).unwrap(),
            vec![2]
        );
    }
}

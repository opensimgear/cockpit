use super::*;
use std::{sync::{atomic::AtomicUsize, mpsc}, time::{Duration, Instant}};

const ID: &str = "stm32:00112233445566778899aabb";

fn fence(runtime: &Runtime, id: &str) -> Arc<CommandFence> {
    command_fence(&mut runtime.inner.control.lock().unwrap(), id).unwrap()
}

#[test]
fn explicit_hold_revokes_prescheduled_automation_even_after_hold_release() {
    let runtime = Runtime::new();let fence = fence(&runtime, ID);
    let old_epoch = fence.epoch.load(Ordering::Acquire);
    let calls = AtomicUsize::new(0);
    let hold = runtime.hold(ID).unwrap();
    assert!(fence.dispatch(old_epoch, || calls.fetch_add(1, Ordering::AcqRel)).is_err());
    drop(hold);
    assert!(fence.dispatch(old_epoch, || calls.fetch_add(1, Ordering::AcqRel)).is_err());
    assert_eq!(calls.load(Ordering::Acquire), 0, "delayed reconnect reached its adapter after explicit control");
    let current = fence.epoch.load(Ordering::Acquire);
    fence.dispatch(current, || calls.fetch_add(1, Ordering::AcqRel)).unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[test]
fn manual_dispatch_waits_for_already_running_automation_before_native_disconnect() {
    let runtime = Runtime::new();let fence = fence(&runtime, ID);
    let epoch = fence.epoch.load(Ordering::Acquire);
    let events = Arc::new(Mutex::new(Vec::new()));
    let (entered, started) = mpsc::channel();let (release, released) = mpsc::channel();
    let automatic = {
        let fence = fence.clone();let events = events.clone();
        std::thread::spawn(move || fence.dispatch(epoch, || {
            events.lock().unwrap().push("automatic entered");entered.send(()).unwrap();
            released.recv().unwrap();events.lock().unwrap().push("automatic completed");
        }).unwrap())
    };
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let hold = runtime.hold(ID).unwrap();
    let (attempt, attempted) = mpsc::channel();let (entered, manual_started) = mpsc::channel();
    let manual = {
        let events = events.clone();
        std::thread::spawn(move || {
            attempt.send(()).unwrap();
            hold.dispatch(|| {events.lock().unwrap().push("manual disconnect");entered.send(()).unwrap();});
        })
    };
    attempted.recv_timeout(Duration::from_secs(2)).unwrap();
    let raced = manual_started.recv_timeout(Duration::from_millis(25)).is_ok();
    // Always release the running adapter before asserting, so a failed test
    // cannot leave a command thread stranded behind its dispatch gate.
    release.send(()).unwrap();automatic.join().unwrap();manual.join().unwrap();
    assert!(!raced, "manual disconnect raced an already-dispatched automatic command");
    assert_eq!(*events.lock().unwrap(), vec!["automatic entered", "automatic completed", "manual disconnect"]);
    assert!(runtime.inner.control.lock().unwrap().held.is_empty());
}

#[test]
fn failed_manual_dispatch_releases_only_its_hold_without_persisting_pause() {
    let runtime = Runtime::new();
    let first = runtime.hold(ID).unwrap();let second = runtime.hold(ID).unwrap();
    assert_eq!(runtime.inner.control.lock().unwrap().held[ID], 2);
    let result: Result<(), &str> = first.dispatch(|| Err("setup owns this gateway"));
    assert_eq!(result, Err("setup owns this gateway"));
    let before = runtime.inner.wake.observed();drop(first);
    {
        let control = runtime.inner.control.lock().unwrap();
        assert_eq!(control.held[ID], 1);assert!(!control.paused.contains_key(ID));
    }
    assert!(runtime.inner.wake.observed() > before);
    second.dispatch(|| ());drop(second);
    let control = runtime.inner.control.lock().unwrap();
    assert!(control.held.is_empty());assert!(!control.paused.contains_key(ID));
}

#[test]
fn hold_capacity_counts_concurrent_operations_and_recovers_after_release() {
    let runtime = Runtime::new();
    let mut holds = (0..MAX_GATEWAYS).map(|_| runtime.hold(ID).unwrap()).collect::<Vec<_>>();
    assert!(runtime.hold(ID).is_err());assert!(runtime.hold("another gateway").is_err());
    {
        let control = runtime.inner.control.lock().unwrap();
        assert_eq!(control.held.len(), 1);assert_eq!(control.held[ID], MAX_GATEWAYS);
        assert_eq!(control.fences.len(), 1, "rejected admission created a fence");
    }
    drop(holds.pop());let replacement = runtime.hold("another gateway").unwrap();
    assert_eq!(runtime.inner.control.lock().unwrap().held.values().sum::<usize>(), MAX_GATEWAYS);
    drop(replacement);drop(holds);
    assert!(runtime.inner.control.lock().unwrap().held.is_empty());
}

#[test]
fn command_epoch_exhaustion_rejects_hold_without_admitting_an_operation() {
    let runtime = Runtime::new();let fence = fence(&runtime, ID);
    fence.epoch.store(u64::MAX, Ordering::Release);
    assert!(runtime.hold(ID).is_err());
    assert!(runtime.inner.control.lock().unwrap().held.is_empty());
    assert_eq!(fence.epoch.load(Ordering::Acquire), u64::MAX);
}

#[test]
fn retry_fence_survives_expiry_between_planning_and_wait_selection() {
    let mut entry = Entry::new(ID.into(), json!({"gatewayId":ID,"backend":"stm32","path":"COM1"}));
    entry.job = Some((1, entry.session.clone()));entry.job_source = Some(entry.state.clone());
    entry.complete(&Completion { id:ID.into(), session:entry.session.clone(), token:1,
        action:Action::Connect, error:Some("temporary connection failure".into()) }).unwrap();
    let deadline = entry.deadline().unwrap();
    assert_eq!(entry.plan(deadline - Duration::from_nanos(1)), None);
    // Exercise the real clock boundary: the previous implementation discarded
    // this fence when projection crossed it after the planning pass.
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1));
    assert_eq!(entry.deadline(), Some(deadline));
    assert_eq!(entry.plan(Instant::now()), Some(Action::Connect));
    entry.paused = true;
    assert_eq!(entry.deadline(), None, "ineligible work must not create a zero-duration busy loop");
}

use super::during;
use osg_streams::NativeWake;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc,
    Arc,
};
use std::time::{Duration, Instant};

#[test]
fn synchronous_work_can_observe_multiple_heartbeats_before_it_finishes() {
    let epoch = Arc::new(AtomicU64::new(4));
    let renewals = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let worker_epoch = epoch.clone();
    let worker_renewals = renewals.clone();
    let worker = std::thread::spawn(move || {
        during(
            &worker_epoch,
            4,
            Instant::now() + Duration::from_secs(2),
            &NativeWake::default(),
            move || {
                worker_renewals.fetch_add(1, Ordering::AcqRel);
                Ok(Instant::now() + Duration::from_millis(35))
            },
            || {
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|error| format!("work release failed: {error}"))?;
                Ok("finished")
            },
        )
    });

    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let began = Instant::now();
    while began.elapsed() < Duration::from_millis(300) {
        assert!(began.elapsed() < Duration::from_secs(1), "heartbeat did not continue during work");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(renewals.load(Ordering::Acquire) >= 3, "renewals stopped while synchronous work was still running");
    release_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap().unwrap(), "finished");
}

#[test]
fn heartbeat_failure_is_reported_and_stops_future_renewals() {
    let epoch = AtomicU64::new(9);
    let calls = Arc::new(AtomicUsize::new(0));
    let heartbeat_calls = calls.clone();
    let result = during(
        &epoch,
        9,
        Instant::now() + Duration::from_secs(1),
        &NativeWake::default(),
        move || {
            if heartbeat_calls.fetch_add(1, Ordering::AcqRel) == 0 {
                Err("injected renewal failure".to_owned())
            } else {
                Ok(Instant::now() + Duration::from_millis(35))
            }
        },
        || {
            while calls.load(Ordering::Acquire) == 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
            std::thread::sleep(Duration::from_millis(30));
            Ok(())
        },
    );
    let count_at_return = calls.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(calls.load(Ordering::Acquire), count_at_return);
    assert!(result.unwrap_err().contains("injected renewal failure"));
}

#[test]
fn epoch_cancellation_stops_renewals_and_rejects_completed_work() {
    let epoch = Arc::new(AtomicU64::new(2));
    let calls = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let worker_epoch = epoch.clone();
    let worker_calls = calls.clone();
    let worker = std::thread::spawn(move || {
        during(
            &worker_epoch,
            2,
            Instant::now() + Duration::from_secs(2),
            &NativeWake::default(),
            move || {
                worker_calls.fetch_add(1, Ordering::AcqRel);
                Ok(Instant::now() + Duration::from_millis(35))
            },
            || {
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|error| error.to_string())?;
                Ok(())
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let started = Instant::now();
    while calls.load(Ordering::Acquire) == 0 {
        assert!(started.elapsed() < Duration::from_secs(1), "heartbeat did not start");
        std::thread::sleep(Duration::from_millis(2));
    }
    epoch.fetch_add(1, Ordering::AcqRel);
    release_tx.send(()).unwrap();
    let result = worker.join().unwrap();
    let count_at_return = calls.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(calls.load(Ordering::Acquire), count_at_return);
    assert!(result.unwrap_err().to_lowercase().contains("cancel"));
}

#[test]
fn deadline_stops_renewals_and_rejects_work_that_finishes_late() {
    let epoch = AtomicU64::new(12);
    let calls = Arc::new(AtomicUsize::new(0));
    let deadline = Instant::now() + Duration::from_millis(100);
    let heartbeat_calls = calls.clone();
    let result = during(
        &epoch,
        12,
        deadline,
        &NativeWake::default(),
        move || {
            heartbeat_calls.fetch_add(1, Ordering::AcqRel);
            Ok(Instant::now() + Duration::from_millis(35))
        },
        || {
            while Instant::now() < deadline + Duration::from_millis(30) {
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(())
        },
    );
    let count_at_return = calls.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(calls.load(Ordering::Acquire), count_at_return);
    assert!(result.unwrap_err().to_lowercase().contains("deadline"));
}

#[test]
fn work_error_is_returned_after_the_heartbeat_thread_is_stopped() {
    let epoch = AtomicU64::new(3);
    let calls = Arc::new(AtomicUsize::new(0));
    let heartbeat_calls = calls.clone();
    let result = during(
        &epoch,
        3,
        Instant::now() + Duration::from_secs(1),
        &NativeWake::default(),
        move || {
            heartbeat_calls.fetch_add(1, Ordering::AcqRel);
            Ok(Instant::now() + Duration::from_millis(35))
        },
        || Err::<(), _>("injected work failure".to_owned()),
    );
    let count_at_return = calls.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(calls.load(Ordering::Acquire), count_at_return);
    assert_eq!(result.unwrap_err(), "injected work failure");
}

#[test]
fn prepared_heartbeat_waits_for_the_renewal_deadline_or_a_fault_notification() {
    let epoch = AtomicU64::new(1);
    let wake = NativeWake::default();
    let (service, serviced) = mpsc::channel();
    let next = Instant::now() + Duration::from_secs(2);
    let mut first = true;
    let result = during(&epoch, 1, next + Duration::from_secs(1), &wake, || {
        service.send(()).unwrap();
        if std::mem::take(&mut first) { Ok(next) }
        else { Err("injected feeder fault".into()) }
    }, || {
        serviced.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(serviced.recv_timeout(Duration::from_millis(100)), Err(mpsc::RecvTimeoutError::Timeout));
        wake.notify();
        serviced.recv_timeout(Duration::from_secs(1)).expect("fault must wake before renewal");
        Ok(())
    });
    assert_eq!(result.unwrap_err(), "injected feeder fault");
}

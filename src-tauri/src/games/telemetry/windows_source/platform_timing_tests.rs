//! Opt-in measurement of the approved Assetto acquisition boundary.
use super::super::super::shared::wait::CaptureWait;
use super::*;
use crate::game_discovery::ProcessIdentity;
use osg_streams::NativeWake;
use std::{
    os::windows::io::{AsRawHandle, BorrowedHandle},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetThreadTimes(
        thread: Handle,
        created: *mut FileTime,
        exited: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}

fn cpu_time(worker: Handle) -> Duration {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    let time = |value: FileTime| (u64::from(value.high) << 32) | u64::from(value.low);
    let ok = unsafe { GetThreadTimes(worker, &mut created, &mut exited, &mut kernel, &mut user) };
    assert_ne!(ok, 0, "Read acquisition thread time");
    Duration::from_nanos((time(kernel) + time(user)) * 100)
}

fn report(times: &mut [Duration]) -> serde_json::Value {
    if times.is_empty() {
        return serde_json::Value::Null;
    }
    times.sort_unstable();
    let percentile = |percent: usize| {
        let index = (times.len() * percent).div_ceil(100) - 1;
        times[index].as_secs_f64() * 1000.
    };
    serde_json::json!({
        "count": times.len(),
        "p50Ms": percentile(50),
        "p95Ms": percentile(95),
        "p99Ms": percentile(99),
    })
}

struct Receipt {
    packet: u32,
    capture_sequence: u64,
    captured: Instant,
    received: Instant,
}

struct StopReader<'a> {
    stop: &'a AtomicBool,
    wake: &'a NativeWake,
}
impl Drop for StopReader<'_> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.wake.notify();
    }
}

#[test]
#[ignore = "opt-in Windows software qualification; private mappings, no simulator"]
fn private_pages_through_shared_worker_report_per_game_timing_and_delivery() {
    let games = [
        (Game::Corsa, "acs.exe", 10),
        (Game::Competizione, "ac2-win64-shipping.exe", 5),
        (Game::Evo, "assettocorsaevo.exe", 5),
        (Game::Rally, "acr.exe", 16),
    ];
    for (game, expected_image, interval_ms) in games {
        measure(game, expected_image, Duration::from_millis(interval_ms));
    }
}

fn measure(game: Game, expected_image: &str, interval: Duration) {
    const WRITES: usize = 240;
    let prefix = format!("Local\\osg-qualification-{}", uuid::Uuid::new_v4());
    let (physics_len, graphics_len, statics_len) = game.full_page_lengths();
    let physics = Producer::new(&format!("{prefix}_physics"), physics_len);
    let _graphics = Producer::new(&format!("{prefix}_graphics"), graphics_len);
    let _statics = Producer::new(&format!("{prefix}_static"), statics_len);
    let pid = std::process::id();
    let process = Process::open(pid).unwrap();
    let identity = ProcessIdentity {
        pid,
        epoch: process.epoch,
        path: process.image.clone(),
    };
    let (monitor, _scope) = crate::xplane::process_events::test_monitor();
    let mut capture_owner = CaptureOwner::new();
    capture_owner.begin_generation().unwrap();
    let source = WindowsSource {
        game,
        pid,
        expected_image: expected_image.into(),
        process: Some(process),
        identity: None,
        pages: Some(Pages::open_named(game, &prefix).unwrap()),
        capture_owner,
        last_packet: None,
        process_error: None,
        monitor: Arc::new(monitor),
    };
    let mut reader = SharedSource::fixture(&identity, game, source).unwrap();
    // This duplicate remains valid after the owner has been released.
    let worker = unsafe { BorrowedHandle::borrow_raw(reader.fixture_worker_handle()) }
        .try_clone_to_owned()
        .unwrap();
    let wake = Arc::new(NativeWake::default());
    let watch = reader.watch(wake.clone()).unwrap().unwrap();
    let writer_timer = CaptureWait::new().unwrap();
    let mut stores = Vec::with_capacity(WRITES);
    let stop = AtomicBool::new(false);
    let (ready, baseline) = mpsc::sync_channel(1);
    let (receipts, elapsed, cpu) = thread::scope(|scope| {
        let stop_reader = StopReader {
            stop: &stop,
            wake: &wake,
        };
        let consumer = scope.spawn(|| {
            let mut receipts = Vec::with_capacity(WRITES);
            wake.bind_thread(thread::current()).unwrap();
            loop {
                let observed = wake.observed();
                while let Some(sample) = reader
                    .poll()
                    .expect("ordered reader must retain continuity")
                {
                    let received = Instant::now();
                    let capture = sample.capture.expect("original native page capture");
                    let packet = u32::from_le_bytes(sample.physics[..4].try_into().unwrap());
                    if packet == 0 {
                        ready
                            .try_send(())
                            .expect("the initial packet must be unique");
                    } else {
                        receipts.push(Receipt {
                            packet,
                            capture_sequence: capture.physics.sequence,
                            captured: capture.physics.captured_at,
                            received,
                        });
                    }
                }
                if stop.load(Ordering::Acquire) {
                    return receipts;
                }
                wake.wait_scheduled(observed, None);
            }
        });
        baseline
            .recv_timeout(Duration::from_secs(2))
            .expect("initial native sample");
        let cpu_before = cpu_time(worker.as_raw_handle());
        let active = Instant::now();
        for packet in 1..=WRITES {
            let before = Instant::now();
            physics.packet(packet as u32);
            stores.push((before, Instant::now()));
            // The fixture writer owns its own high-resolution timer. Its
            // schedule never waits for the reader to capture a packet.
            writer_timer.wait(interval).unwrap();
        }
        let elapsed = active.elapsed();
        let cpu = cpu_time(worker.as_raw_handle()) - cpu_before;
        writer_timer.wait(Duration::from_millis(50)).unwrap();
        drop(stop_reader);
        (consumer.join().unwrap(), elapsed, cpu)
    });
    drop(reader);
    assert_eq!(
        unsafe { WaitForSingleObject(worker.as_raw_handle(), 2000) },
        0
    );
    assert!(watch.is_closed());

    let mut capture_to_receipt = Vec::new();
    let mut writer_to_capture_lower = Vec::new();
    let mut writer_to_capture_upper = Vec::new();
    let mut ambiguous = 0;
    let mut previous = 0;
    let mut previous_capture = 0;
    for receipt in &receipts {
        let packet = receipt.packet as usize;
        assert!(
            previous < packet && packet <= WRITES,
            "Unexpected or duplicate packet {packet}"
        );
        assert!(previous_capture < receipt.capture_sequence);
        previous = packet;
        previous_capture = receipt.capture_sequence;
        capture_to_receipt.push(
            receipt
                .received
                .checked_duration_since(receipt.captured)
                .unwrap(),
        );
        let (begin, end) = stores[packet - 1];
        // Capture timestamps precede the page copy. An observation that may
        // have started before the store completed cannot bound detection.
        if let Some(lower) = receipt.captured.checked_duration_since(end) {
            writer_to_capture_lower.push(lower);
            writer_to_capture_upper.push(receipt.captured.duration_since(begin));
        } else {
            ambiguous += 1;
        }
    }
    assert_eq!(previous, WRITES, "the reader must catch the last packet");
    assert!(!writer_to_capture_lower.is_empty());
    let mut cadence = stores
        .windows(2)
        .map(|pair| pair[1].1 - pair[0].1)
        .collect::<Vec<_>>();
    let observed_hz = (WRITES - 1) as f64 / (stores[WRITES - 1].1 - stores[0].1).as_secs_f64();
    println!(
        "ASSETTO_ACQUISITION {}",
        serde_json::json!({
            "game": game.id(),
            "requestedWriterIntervalMs": interval.as_secs_f64() * 1000.,
            "observedWriterHz": observed_hz,
            "writerIntervals": report(&mut cadence),
            "writes": WRITES,
            "received": receipts.len(),
            "missing": WRITES - receipts.len(),
            "ambiguousWriterCaptures": ambiguous,
            "writerToCaptureLower": report(&mut writer_to_capture_lower),
            "writerToCaptureUpper": report(&mut writer_to_capture_upper),
            "captureToOrderedReader": report(&mut capture_to_receipt),
            "acquisitionWorkerCpuMs": cpu.as_secs_f64() * 1000.,
            "workerCpuPercentOfOneCore": 100. * cpu.as_secs_f64() / elapsed.as_secs_f64(),
        })
    );
}

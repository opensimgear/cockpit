//! Passive reader startup has its own worker per integration so native opens
//! and watch registration cannot hold up process inventory or capture reads.
use super::*;
use crate::game_vehicles::{CaptureWakes, VehicleObservation};
use osg_streams::NativeWake;

pub(super) fn start(
    inner: Arc<Inner>,
    stop: Arc<AtomicBool>,
    wakes: &CaptureWakes,
) -> Vec<thread::JoinHandle<()>> {
    wakes.iter().map(|(integration, wake)| {
        let inner = inner.clone();
        let stop = stop.clone();
        let wake = wake.clone();
        thread::spawn(move || run(&inner, &stop, integration, &wake, |work| {
            finish_open(&inner, integration, work);
        }))
    }).collect()
}

pub(super) fn run(
    inner: &Inner,
    stop: &AtomicBool,
    integration: &str,
    wake: &NativeWake,
    mut finish: impl FnMut(VehicleObservation),
) {
    let _ = wake.bind_thread(thread::current());
    while !inner.closed.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
        let observed = wake.observed();
        let work = inner.update_vehicles(|vehicles| vehicles.take_open(integration));
        if let Some(work) = work {
            finish(work);
        } else {
            wake.wait_scheduled(observed, None);
        }
    }
}

fn finish_open(inner: &Inner, integration: &str, work: VehicleObservation) {
    let verify = |work: VehicleObservation| {
        let games = inner.vehicle_telemetry_games();
        work.verify_ready(&games, |game| inner.setup.vehicle_setup_revision(game),
            |process| inner.platform.alive(process))
    };
    let mut work = verify(work);
    if work.can_open() {
        work = work.read();
        if let Some(wake) = inner.changes.vehicle_capture_wakes.get(integration) {
            work = work.prepare_watch(wake);
        }
        work = verify(work);
    }
    if !inner.closed.load(Ordering::Acquire) {
        inner.update_vehicles(|vehicles| vehicles.apply_observation(work));
    }
}

pub(super) fn finish_inline(inner: &Inner, integration: &str, work: VehicleObservation) {
    let observed = work.read();
    let games = inner.vehicle_telemetry_games();
    let observed = observed.verify(&games, &inner.setup.provider_setup_revision(integration),
        |process| inner.platform.alive(process));
    inner.update_vehicles(|vehicles| vehicles.apply_observation(observed));
}

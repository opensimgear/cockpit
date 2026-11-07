//! Fixed software workload for the explicitly enabled desktop qualification build.
use super::Change;
use crate::{gateways::GatewayManager, profile_session::RunOwner};
use osg_mapping::engine::Counter;
use osg_profiles::PortValue;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Mutex,
    },
    thread,
    time::Duration,
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const BURST_MS: u64 = 192;
const NS_PER_MS: u64 = 1_000_000;
const MAX_ADMISSION_BATCH: usize = 8;
const CLIENT_ID: &str = "desktop-reactive-qualification";
static BURST: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    command: String,
    owner: RunOwner,
    nodes: [String; 2],
    start_ns: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Submission {
    at_ns: String,
    dispatch_at_ns: String,
    admitted_at_ns: String,
    round_trip_ms: f64,
    coordinator_wait_ms: f64,
    coordinator_work_ms: f64,
    batch_index: usize,
    pending_frames: u64,
    value: bool,
}

struct Offer {
    sequence: u64,
    at: u64,
    value: bool,
}

struct Burst<'a> {
    manager: &'a GatewayManager,
    request: Request,
    clock: Clock,
    expires: u64,
    streams: [String; 2],
    cancelled: AtomicBool,
    attempted: [AtomicU64; 2],
}

impl Burst<'_> {
    fn frame(&self, lane: usize, sequence: u64, value: bool) -> super::Request {
        super::Request {
            command: "profile_debug_controller_input".into(),
            owner: self.request.owner.clone(),
            node_id: self.request.nodes[lane].clone(),
            stream_id: Some(self.streams[lane].clone()),
            sequence: Some(Counter(sequence)),
            changes: Some(vec![Change {
                port_id: "button-1".into(),
                value: PortValue::Boolean(value),
            }]),
        }
    }

    fn offer(&self, start: u64, lanes: [mpsc::SyncSender<Offer>; 2]) -> Result<(), String> {
        for offset in 0..=BURST_MS {
            self.clock.wait_until(start + offset * NS_PER_MS)?;
            if self.cancelled.load(Ordering::Acquire) {
                break;
            }
            for (lane, interval) in [1, 8].into_iter().enumerate() {
                if offset % interval != 0 {
                    continue;
                }
                let index = offset / interval;
                lanes[lane]
                    .try_send(Offer {
                        sequence: index + 1,
                        at: self.clock.now()?,
                        value: index % 2 == 0,
                    })
                    .map_err(|error| {
                        format!(
                            "Qualification lane {lane} could not accept the offered edge: {error}"
                        )
                    })?;
            }
        }
        Ok(())
    }

    fn submit(
        &self,
        lane: usize,
        offers: mpsc::Receiver<Offer>,
    ) -> Result<Vec<Submission>, String> {
        let result = (|| {
            let mut submissions = Vec::with_capacity([193, 25][lane]);
            while let Ok(first) = offers.recv() {
                if self.cancelled.load(Ordering::Acquire) {
                    break;
                }
                let first_dispatch = self.clock.now()?;
                self.manager.qualify_debug_controller_batch(|admit, wait| {
                    // Drain only edges already waiting after admission opens.
                    let ready = std::iter::once(first)
                        .chain(offers.try_iter().take(MAX_ADMISSION_BATCH - 1));
                    for (batch_index, offer) in ready.enumerate() {
                        if self.cancelled.load(Ordering::Acquire) {
                            break;
                        }
                        let now = self.clock.now()?;
                        if now >= self.expires {
                            return Err(
                                "The qualification burst exceeded its delivery window.".into()
                            );
                        }
                        let dispatch = if batch_index == 0 {
                            first_dispatch
                        } else {
                            now
                        };
                        self.attempted[lane].store(offer.sequence, Ordering::Release);
                        let frame = self.frame(lane, offer.sequence, offer.value);
                        let (state, work) = admit(&frame).map_err(|error| {
                            format!(
                                "Qualification lane {lane} frame {}: {error}",
                                offer.sequence
                            )
                        })?;
                        let admitted = self.clock.now()?;
                        if admitted >= self.expires {
                            return Err(
                                "The qualification burst exceeded its delivery window.".into()
                            );
                        }
                        if state["sequence"] != offer.sequence.to_string()
                            || state["replayed"] != false
                        {
                            return Err(format!(
                                "Qualification lane {lane} did not accept frame {}.",
                                offer.sequence
                            ));
                        }
                        let pending_frames = state["pendingFrames"]
                            .as_u64()
                            .filter(|count| (1..=256).contains(count))
                            .ok_or("Qualification frame did not enter the ordered queue.")?;
                        submissions.push(Submission {
                            at_ns: offer.at.to_string(),
                            dispatch_at_ns: dispatch.to_string(),
                            admitted_at_ns: admitted.to_string(),
                            round_trip_ms: (admitted - dispatch) as f64 / NS_PER_MS as f64,
                            coordinator_wait_ms: if batch_index == 0 {
                                wait.as_secs_f64() * 1000.0
                            } else {
                                0.0
                            },
                            coordinator_work_ms: work.as_secs_f64() * 1000.0,
                            batch_index,
                            pending_frames,
                            value: offer.value,
                        });
                    }
                    Ok(())
                })?;
            }
            Ok(submissions)
        })();
        if result.is_err() {
            self.cancelled.store(true, Ordering::Release);
        }
        result
    }

    fn release(&self) {
        for (lane, attempted) in self.attempted.iter().enumerate() {
            let sequence = attempted.load(Ordering::Acquire);
            if sequence > 0 {
                let frame = self.frame(lane, sequence + 1, false);
                let _ = self
                    .manager
                    .qualify_debug_controller_batch(|admit, _| admit(&frame));
            }
        }
    }
}

struct Clock(u64);

impl Clock {
    fn new() -> Result<Self, String> {
        let mut frequency = 0;
        if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
            return Err("Windows performance counter frequency is unavailable.".into());
        }
        Ok(Self(frequency as u64))
    }

    fn now(&self) -> Result<u64, String> {
        let mut ticks = 0;
        if unsafe { QueryPerformanceCounter(&mut ticks) } == 0 || ticks < 0 {
            return Err("Windows performance counter is unavailable.".into());
        }
        u64::try_from(ticks as u128 * 1_000_000_000 / u128::from(self.0))
            .map_err(|_| "Windows performance counter timestamp overflowed.".into())
    }

    fn wait_until(&self, deadline: u64) -> Result<(), String> {
        loop {
            let now = self.now()?;
            if now >= deadline {
                return Ok(());
            }
            // Sleep only outside the final quarter millisecond of the fixed schedule.
            let remaining = deadline - now;
            if remaining > 250_000 {
                std::thread::sleep(Duration::from_nanos(remaining - 250_000));
            } else {
                std::thread::yield_now();
            }
        }
    }
}

fn run_burst(manager: &GatewayManager, request: Value) -> Result<[Vec<Submission>; 2], String> {
    if !matches!(
        std::env::var("OSG_REACTIVE_QUALIFICATION").as_deref(),
        Ok("1")
    ) {
        return Err("The desktop qualification source is disabled.".into());
    }
    let _guard = BURST
        .try_lock()
        .map_err(|_| "A qualification burst is already running.")?;
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    if request.command != "qualification_debug_burst" || request.nodes[0] == request.nodes[1] {
        return Err("Choose two distinct applied debug controllers.".into());
    }
    for node in &request.nodes {
        let reply = manager.request(json!({
            "command":"profile_debug_controller_status", "clientId":CLIENT_ID,
            "owner":request.owner, "nodeId":node,
        }));
        if reply["ok"] != true {
            return Err(format!(
                "Qualification lane {node} is unavailable: {}",
                reply["error"]
            ));
        }
        let neutral = reply["state"]["debugController"]["values"]
            .as_array()
            .is_some_and(|ports| {
                ports
                    .iter()
                    .any(|port| port["portId"] == "button-1" && port["value"] == false)
            });
        if reply["state"]["debugController"]["pendingFrames"] != 0 || !neutral {
            return Err("Both qualification lanes require an applied neutral debug button.".into());
        }
    }
    let clock = Clock::new()?;
    let start = request
        .start_ns
        .parse::<u64>()
        .map_err(|_| "Invalid qualification start timestamp.")?;
    let now = clock.now()?;
    if !(now..=now.saturating_add(500 * NS_PER_MS)).contains(&start) {
        return Err("Qualification start must be within the next 500 ms.".into());
    }
    let burst = Burst {
        manager,
        request,
        clock,
        expires: start + 1_000 * NS_PER_MS,
        streams: std::array::from_fn(|_| uuid::Uuid::new_v4().to_string()),
        cancelled: AtomicBool::new(false),
        attempted: std::array::from_fn(|_| AtomicU64::new(0)),
    };
    let (fast_tx, fast_rx) = mpsc::sync_channel(193);
    let (peer_tx, peer_rx) = mpsc::sync_channel(25);
    let result = thread::scope(|scope| {
        let fast = scope.spawn(|| burst.submit(0, fast_rx));
        let peer = scope.spawn(|| burst.submit(1, peer_rx));
        let offered = burst.offer(start, [fast_tx, peer_tx]);
        if offered.is_err() {
            burst.cancelled.store(true, Ordering::Release);
        }
        let fast = fast
            .join()
            .map_err(|_| "Fast qualification lane panicked.")?;
        let peer = peer
            .join()
            .map_err(|_| "Peer qualification lane panicked.")?;
        let lanes = [fast?, peer?];
        offered?;
        Ok(lanes)
    });
    if result.is_err() {
        // Release only through the original owner fence. The fixture's Stop
        // remains the fallback if the queue is full or the owner is retired.
        burst.release();
    }
    result
}

pub(crate) fn run(manager: &GatewayManager, request: Value) -> Value {
    match run_burst(manager, request) {
        Ok(lanes) => json!({"ok":true,"state":{"qualificationBurst":lanes}}),
        Err(error) => json!({"ok":false,"error":error.chars().take(512).collect::<String>()}),
    }
}

#[cfg(test)]
#[path = "qualification_tests.rs"]
mod tests;

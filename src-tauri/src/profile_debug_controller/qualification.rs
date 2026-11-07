//! Fixed software workload for the explicitly enabled desktop qualification build.
use crate::{gateways::GatewayManager, profile_session::RunOwner};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::Mutex, time::Duration};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const BURST_MS: u64 = 192;
const NS_PER_MS: u64 = 1_000_000;
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
    round_trip_ms: f64,
    pending_frames: u64,
    value: bool,
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
    let expires = start + 1_000 * NS_PER_MS;
    let stream_ids = std::array::from_fn::<_, 2, _>(|_| uuid::Uuid::new_v4().to_string());
    let send = |index: usize, sequence: String, value| {
        manager.request(json!({
            "command":"profile_debug_controller_input", "clientId":CLIENT_ID, "owner":request.owner,
            "nodeId":request.nodes[index], "streamId":stream_ids[index], "sequence":sequence,
            "changes":[{"portId":"button-1","value":value}]
        }))
    };
    let mut lanes = [Vec::with_capacity(193), Vec::with_capacity(25)];
    let mut attempted = [0, 0];
    let result = (|| -> Result<(), String> {
        for offset in 0..=BURST_MS {
            clock.wait_until(start + offset * NS_PER_MS)?;
            for (index, interval) in [1, 8].into_iter().enumerate() {
                if offset % interval != 0 {
                    continue;
                }
                let frame = offset / interval;
                let sequence = (frame + 1).to_string();
                let value = frame % 2 == 0;
                let at = clock.now()?;
                if at >= expires {
                    return Err("The qualification burst exceeded its delivery window.".into());
                }
                attempted[index] = frame + 1;
                let reply = send(index, sequence.clone(), value);
                let done = clock.now()?;
                if done >= expires {
                    return Err("The qualification burst exceeded its delivery window.".into());
                }
                let state = &reply["state"]["debugController"];
                if reply["ok"] != true
                    || state["sequence"] != sequence
                    || state["replayed"] != false
                {
                    return Err(format!(
                        "Qualification lane {index} frame {sequence} was rejected: {}",
                        reply["error"]
                    ));
                }
                let pending_frames = state["pendingFrames"]
                    .as_u64()
                    .filter(|count| (1..=256).contains(count))
                    .ok_or("Qualification frame did not enter the ordered queue.")?;
                lanes[index].push(Submission {
                    at_ns: at.to_string(),
                    round_trip_ms: (done - at) as f64 / NS_PER_MS as f64,
                    pending_frames,
                    value,
                });
            }
        }
        Ok(())
    })();
    if result.is_err() {
        // Release only through the original owner fence. The fixture's Stop
        // remains the fallback if the queue is full or the owner is retired.
        for (index, sequence) in attempted.into_iter().enumerate().filter(|(_, n)| *n > 0) {
            let _ = send(index, (sequence + 1).to_string(), false);
        }
    }
    result?;
    Ok(lanes)
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

//! Bound debug capture-to-reducer time by the end of its accepted batch.
use super::Request;
use crate::profile_session::RunOwner;
use osg_mapping::engine::{typed::native::Admission, Reason};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex,
    },
    time::Duration,
};

#[path = "qualification_histogram.rs"]
mod histogram;
use histogram::Histogram;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static TRACKER: Mutex<Option<Tracker>> = Mutex::new(None);

pub(super) struct Scope(mpsc::Receiver<Result<Value, String>>);

impl Scope {
    pub(super) fn start(
        owner: RunOwner,
        nodes: [String; 2],
        streams: [String; 2],
        counts: [u64; 2],
    ) -> Self {
        let (done, result) = mpsc::channel();
        let lanes = std::array::from_fn(|index| Lane {
            node: nodes[index].clone(),
            stream: streams[index].clone(),
            count: counts[index],
            first: None,
            captured: 0,
            applied: 0,
            timing: Histogram::default(),
        });
        // The caller holds the existing exclusive qualification burst guard.
        *TRACKER.lock().unwrap_or_else(|p| p.into_inner()) = Some(Tracker {
            owner,
            lanes,
            done: Some(done),
        });
        ACTIVE.store(true, Ordering::Release);
        Self(result)
    }

    pub(super) fn finish(self) -> Result<Value, String> {
        self.0.recv_timeout(Duration::from_secs(2)).map_err(|_| {
            "The reducer did not apply every qualification edge before the evidence deadline."
        })?
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
        *TRACKER.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

struct Lane {
    node: String,
    stream: String,
    count: u64,
    first: Option<(u64, u64)>,
    captured: u64,
    applied: u64,
    timing: Histogram,
}

impl Lane {
    fn capture(&mut self, generation: u64, sequence: u64) -> Result<(), String> {
        let (first_generation, first_sequence) = *self.first.get_or_insert((generation, sequence));
        if first_generation != generation
            || sequence.checked_sub(first_sequence) != Some(self.captured)
            || self.captured == self.count
        {
            return Err("The qualification capture lost its exact source sequence.".into());
        }
        self.captured += 1;
        Ok(())
    }

    fn apply(
        &mut self,
        generation: u64,
        sequence: u64,
        captured_us: u64,
        applied_us: u64,
        clean: bool,
    ) -> Result<(), String> {
        let Some((first_generation, first_sequence)) = self.first else {
            return Ok(());
        };
        let Some(offset) = sequence
            .checked_sub(first_sequence)
            .filter(|offset| *offset < self.captured)
        else {
            return Ok(());
        };
        if generation != first_generation || offset != self.applied || !clean {
            return Err(
                "The reducer did not cleanly apply the exact qualification sequence.".into(),
            );
        }
        let elapsed = applied_us
            .checked_sub(captured_us)
            .ok_or("Reducer timing used a different clock from input capture.")?;
        self.timing.record(elapsed);
        self.applied += 1;
        Ok(())
    }
}

struct Tracker {
    owner: RunOwner,
    lanes: [Lane; 2],
    done: Option<mpsc::Sender<Result<Value, String>>>,
}

impl Tracker {
    fn complete(&mut self, result: Result<Value, String>) {
        if let Some(done) = self.done.take() {
            let _ = done.send(result);
        }
    }
}

pub(super) fn captured(request: &Request, generation: u64, sequence: u64) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let mut slot = TRACKER.lock().unwrap_or_else(|p| p.into_inner());
    let Some(tracker) = slot
        .as_mut()
        .filter(|t| t.done.is_some() && t.owner == request.owner)
    else {
        return;
    };
    let Some(lane) = tracker.lanes.iter_mut().find(|lane| {
        request.node_id == lane.node && request.stream_id.as_deref() == Some(lane.stream.as_str())
    }) else {
        return;
    };
    if let Err(error) = lane.capture(generation, sequence) {
        tracker.complete(Err(error));
    }
}

pub(crate) fn accepted(
    owner: &RunOwner,
    admissions: &[Admission],
    diagnostics: &[String],
    clock: impl FnOnce() -> u64,
) {
    if admissions.is_empty() || !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let applied_us = clock();
    let mut slot = TRACKER.lock().unwrap_or_else(|p| p.into_inner());
    let Some(tracker) = slot
        .as_mut()
        .filter(|t| t.done.is_some() && &t.owner == owner)
    else {
        return;
    };
    for admission in admissions {
        let Admission::Direct(frame) = admission else {
            continue;
        };
        if frame.reason != Reason::Change {
            continue;
        }
        let Some(lane) = tracker
            .lanes
            .iter_mut()
            .find(|lane| lane.node == frame.source)
        else {
            continue;
        };
        if let Err(error) = lane.apply(
            frame.generation.0,
            frame.sequence.0,
            frame.captured_us.0,
            applied_us,
            diagnostics.is_empty(),
        ) {
            tracker.complete(Err(error));
            return;
        }
    }
    if tracker.lanes.iter().all(|lane| lane.applied == lane.count) {
        tracker.complete(Ok(json!(tracker
            .lanes
            .each_ref()
            .map(|lane| lane.timing.summary()))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_times_only_the_captured_generation_and_ordered_range() {
        let mut lane = Lane {
            node: "debug".into(),
            stream: "fixture".into(),
            count: 2,
            first: None,
            captured: 0,
            applied: 0,
            timing: Histogram::default(),
        };
        lane.apply(3, 39, 50, 55, true).unwrap();
        lane.capture(3, 40).unwrap();
        assert!(lane.capture(3, 42).is_err());
        lane.capture(3, 41).unwrap();
        assert!(lane.capture(3, 42).is_err());
        assert!(lane.apply(4, 40, 50, 55, true).is_err());
        assert!(lane.apply(3, 41, 50, 55, true).is_err());
        assert!(lane.apply(3, 40, 50, 55, false).is_err());
        assert!(lane.apply(3, 40, 55, 50, true).is_err());
        lane.apply(3, 40, 50, 150, true).unwrap();
        lane.apply(3, 41, 80, 150, true).unwrap();
        lane.apply(3, 42, 100, 150, true).unwrap();
        assert_eq!(
            lane.timing.summary(),
            json!({
                "count": 2, "p50": 0.1, "p95": 0.1, "p99": 0.1, "max": 0.1,
            })
        );
    }
}

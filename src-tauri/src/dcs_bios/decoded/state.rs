//! Pure state machine. Acquisition timestamps and continuity are evidence;
//! reading or publishing a snapshot never renews that evidence.
use super::{Snapshot, Spec};
use crate::dcs_bios::{Block, CapturedDatagram, ExportEvent, Memory, Parser};
use std::{sync::Arc, time::Instant};

const EXHAUSTED: &str = "DCS decode epoch exhausted.";
const MAX_FRAME_EVENTS: usize = 16;

pub(crate) struct State {
    spec: Spec,
    parser: Parser,
    snapshot: Snapshot,
    input: Option<(String, u64, Instant)>,
    qualified: bool,
}

impl State {
    pub fn new(spec: Spec, producer_id: String) -> Self {
        Self {
            parser: Parser::new(spec.counter.address),
            snapshot: Snapshot {
                producer_id,
                epoch: 1,
                capture_sequence: 0,
                aircraft: None,
                counter: None,
                captured_at: None,
                freshness_ms: spec.freshness_ms,
                status: "waiting",
                malformed_frames: 0,
                error: None,
                memory: Arc::new(Memory::new()),
            },
            spec,
            input: None,
            qualified: false,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    // Always withdraw validity before checking the generation increment. Even
    // exhaustion must leave no usable bytes or renewable lease behind.
    fn reset(&mut self, status: &'static str, retain_metadata: bool) -> Result<(), String> {
        Arc::make_mut(&mut self.snapshot.memory).clear();
        self.snapshot.captured_at = None;
        self.snapshot.status = status;
        self.qualified = false;
        if !retain_metadata {
            self.snapshot.aircraft = None;
            self.snapshot.counter = None;
        }
        self.snapshot.epoch = match self.snapshot.epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => {
                self.snapshot.status = "error";
                self.snapshot.error = Some(EXHAUSTED.into());
                self.snapshot.aircraft = None;
                self.snapshot.counter = None;
                return Err(EXHAUSTED.into());
            }
        };
        Ok(())
    }

    pub fn invalidate(&mut self, message: &str) -> Result<Snapshot, String> {
        if self.snapshot.status != "error" {
            self.reset("error", false)?;
            self.parser = Parser::new(self.spec.counter.address);
            self.snapshot.error = Some(message.chars().take(512).collect());
        }
        Ok(self.snapshot())
    }

    pub fn expire(&mut self, now: Instant) -> Result<Option<Snapshot>, String> {
        if self.snapshot.deadline().is_some_and(|deadline| now >= deadline) {
            // Retain the last BIOS counter: an identical full replay cannot
            // repeatedly resurrect a simulator which stopped exporting.
            let captured_at = self.snapshot.captured_at;
            self.reset("stale", true)?;
            // Keep the age of the last real export observable. Stale status
            // removes the deadline and freshness without changing its origin.
            self.snapshot.captured_at = captured_at;
            self.parser = Parser::new(self.spec.counter.address);
            return Ok(Some(self.snapshot()));
        }
        Ok(None)
    }

    fn malformed(&mut self) -> Result<(), String> {
        self.snapshot.malformed_frames = self.snapshot.malformed_frames.saturating_add(1);
        self.reset("waiting", false)
    }

    pub fn feed(&mut self, report: &CapturedDatagram, now: Instant) -> Result<Vec<Snapshot>, String> {
        if self.snapshot.status == "error" {
            return Err(self.snapshot.error.clone().unwrap_or_else(|| "DCS decoder stopped.".into()));
        }
        let mut publications: Vec<_> = self.expire(now)?.into_iter().collect();
        if report.sequence == 0 || report.producer_id.is_empty() {
            self.input = None;
            self.reset("waiting", false)?;
            self.parser = Parser::new(self.spec.counter.address);
            publications.push(self.snapshot());
            return Ok(publications);
        }
        if let Some((producer, sequence, _)) = &self.input {
            if producer == &report.producer_id && report.sequence <= *sequence {
                return Ok(publications);
            }
        }
        let interrupted = self.input.as_ref().is_some_and(|(producer, sequence, _)| {
            producer != &report.producer_id || sequence.checked_add(1) != Some(report.sequence)
        });
        let regressed_time = self.input.as_ref().is_some_and(|(producer, _, captured)| {
            producer == &report.producer_id && report.captured_at < *captured
        });
        self.input = Some((report.producer_id.clone(), report.sequence, report.captured_at));
        if interrupted {
            self.reset("waiting", false)?;
            self.parser = Parser::new(self.spec.counter.address);
            self.snapshot.capture_sequence = report.sequence;
            publications.push(self.snapshot());
        }
        let usable = now.checked_duration_since(report.captured_at)
            .is_some_and(|age| age.as_millis() <= self.spec.freshness_ms as u128);
        if !usable || regressed_time {
            // Invalid transport timing cannot be used to reconstruct a frame.
            // Keep the remembered counter so a following full frozen replay
            // cannot turn rejected timing into a new baseline.
            if self.snapshot.captured_at.is_some() || self.snapshot.memory.valid.iter().any(|v| *v) {
                self.reset("waiting", true)?;
                self.snapshot.capture_sequence = report.sequence;
                publications.push(self.snapshot());
            }
            self.parser = Parser::new(self.spec.counter.address);
            return Ok(publications);
        }
        let events = self.parser.feed(&report.bytes);
        if events.len() > MAX_FRAME_EVENTS {
            let message = "DCS decoded batch exceeded its 16-frame bound; acquire a new source baseline.";
            self.invalidate(message)?;
            return Err(message.into());
        }
        for event in events {
            let publish = match event {
                ExportEvent::Reset => { self.malformed()?; true },
                ExportEvent::Frame(blocks) => self.frame(&blocks, report.captured_at)?,
            };
            if publish {
                self.snapshot.capture_sequence = report.sequence;
                publications.push(self.snapshot());
            }
        }
        Ok(publications)
    }

    fn frame(&mut self, blocks: &[Block], captured_at: Instant) -> Result<bool, String> {
        let Some(counter) = self.spec.counter.read_blocks(blocks)
            .filter(|counter| *counter <= self.spec.counter.maximum) else {
            self.malformed()?;
            return Ok(true);
        };
        let advance = self.snapshot.counter.map(|previous| counter.wrapping_sub(previous) & 255);
        if advance.is_some_and(|delta| delta >= 128) {
            // Never apply any bytes from the regressed frame, even its name.
            self.reset("waiting", false)?;
            return Ok(true);
        }
        let gap = advance.is_some_and(|delta| delta > 1);
        if gap {
            // Sparse state is only meaningful across consecutive BIOS frames.
            // In particular, never copy a pre-gap aircraft header into this one.
            self.reset("waiting", false)?;
        }
        let mut candidate = (*self.snapshot.memory).clone();
        candidate.apply(blocks);
        let name = candidate.string(self.spec.aircraft_address, self.spec.aircraft_length);
        let shutdown = name.as_ref().is_some_and(|name| name.trim().is_empty() || name == "NONE");
        if advance == Some(0) {
            if shutdown && self.snapshot.aircraft.is_some() {
                self.reset("waiting", true)?;
                self.snapshot.aircraft = None;
                self.snapshot.status = "stopped";
                self.snapshot.captured_at = Some(captured_at);
                return Ok(true);
            }
            return Ok(false);
        }
        let start = self.spec.aircraft_address as usize;
        let range = start..start + self.spec.aircraft_length;
        if name.is_none() && candidate.valid[range.clone()].iter().all(|v| *v) {
            self.malformed()?;
            return Ok(true);
        }
        if shutdown {
            if self.snapshot.aircraft.is_some() || self.qualified {
                self.reset("waiting", false)?;
            } else {
                Arc::make_mut(&mut self.snapshot.memory).clear();
            }
            self.snapshot.counter = Some(counter);
            self.snapshot.aircraft = None;
            self.snapshot.status = "stopped";
            self.snapshot.captured_at = Some(captured_at);
            return Ok(true);
        }
        let identity_changed = name.as_ref().is_some_and(|name| self.snapshot.aircraft.as_ref() != Some(name));
        let can_live = advance == Some(1) && self.qualified && !gap && !identity_changed;
        if identity_changed {
            self.reset("waiting", false)?;
            let mut current = Memory::new();
            current.apply(blocks);
            // A changed aircraft may be expressed by a sparse header update.
            // Only its now-complete candidate header survives the old context.
            current.bytes[range.clone()].copy_from_slice(&candidate.bytes[range.clone()]);
            current.valid[range.clone()].copy_from_slice(&candidate.valid[range]);
            candidate = current;
        }
        self.qualified = name.is_some();
        if name.is_some() {
            self.snapshot.aircraft = name;
        }
        self.snapshot.memory = Arc::new(candidate);
        self.snapshot.counter = Some(counter);
        self.snapshot.captured_at = Some(captured_at);
        self.snapshot.status = if can_live && self.qualified { "live" } else { "waiting" };
        Ok(true)
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

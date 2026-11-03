//! Pure state machine. Acquisition timestamps and continuity are evidence;
//! reading or publishing a snapshot never renews that evidence.
use super::{CapturedMemory, CounterObservation, Snapshot, Spec};
use crate::dcs_bios::{CapturedBlock, CapturedDatagram, CapturedExportEvent, CapturedParser};
use std::{sync::Arc, time::Instant};

const EXHAUSTED: &str = "DCS decode epoch exhausted.";

pub(crate) struct State {
    spec: Spec,
    parser: CapturedParser,
    snapshot: Snapshot,
    input: Option<(String, u64, Instant)>,
    qualified: bool,
}

impl State {
    pub fn new(spec: Spec, producer_id: String) -> Self {
        Self {
            parser: CapturedParser::new(spec.counter.address),
            snapshot: Snapshot {
                producer_id,
                epoch: 1,
                aircraft: None,
                counter: None,
                counter_observation: None,
                freshness_ms: spec.freshness_ms,
                status: "waiting",
                malformed_frames: 0,
                error: None,
                memory: Arc::new(CapturedMemory::new(String::new())),
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
        self.snapshot.counter_observation = None;
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
            self.parser = CapturedParser::new(self.spec.counter.address);
            self.snapshot.error = Some(message.chars().take(512).collect());
        }
        Ok(self.snapshot())
    }

    pub fn expire(&mut self, now: Instant) -> Result<Option<Snapshot>, String> {
        if self
            .snapshot
            .deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            // Retain the last BIOS counter: an identical full replay cannot
            // repeatedly resurrect a simulator which stopped exporting.
            let observation = self.snapshot.counter_observation.clone();
            self.reset("stale", true)?;
            // Keep the age of the last real export observable. Stale status
            // removes the deadline and freshness without changing its origin.
            self.snapshot.counter_observation = observation;
            self.parser = CapturedParser::new(self.spec.counter.address);
            return Ok(Some(self.snapshot()));
        }
        Ok(None)
    }

    fn malformed(&mut self) -> Result<(), String> {
        self.snapshot.malformed_frames = self.snapshot.malformed_frames.saturating_add(1);
        self.reset("waiting", false)
    }

    pub fn feed(
        &mut self,
        report: &CapturedDatagram,
        now: Instant,
    ) -> Result<Vec<Snapshot>, String> {
        if self.snapshot.status == "error" {
            return Err(self
                .snapshot
                .error
                .clone()
                .unwrap_or_else(|| "DCS decoder stopped.".into()));
        }
        let mut publications: Vec<_> = self.expire(now)?.into_iter().collect();
        if report.sequence == 0 || (report.producer_id.is_empty() || report.producer_id.len() > 512)
        {
            self.input = None;
            self.reset("waiting", false)?;
            self.parser = CapturedParser::new(self.spec.counter.address);
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
        self.input = Some((
            report.producer_id.clone(),
            report.sequence,
            report.captured_at,
        ));
        if interrupted {
            self.reset("waiting", false)?;
            self.parser = CapturedParser::new(self.spec.counter.address);

            publications.push(self.snapshot());
        }
        let usable = now
            .checked_duration_since(report.captured_at)
            .is_some_and(|age| age.as_millis() <= self.spec.freshness_ms as u128);
        if !usable || regressed_time {
            // Invalid transport timing cannot be used to reconstruct a frame.
            // Keep the remembered counter so a following full frozen replay
            // cannot turn rejected timing into a new baseline.
            if self.snapshot.counter_observation.is_some()
                || self.snapshot.memory.valid.iter().any(|v| *v)
            {
                self.reset("waiting", true)?;

                publications.push(self.snapshot());
            }
            self.parser = CapturedParser::new(self.spec.counter.address);
            return Ok(publications);
        }
        let events = match self.parser.feed(report) {
            Ok(events) => events,
            Err(_) => {
                let message =
                    "DCS decoded batch exceeded its 16-frame bound; acquire a new source baseline.";
                self.invalidate(message)?;
                return Err(message.into());
            }
        };
        if self.snapshot.memory.acquisition_id() != report.producer_id {
            self.snapshot.memory = Arc::new(CapturedMemory::new(report.producer_id.clone()));
        }
        for event in events {
            let publish = match event {
                CapturedExportEvent::Reset => {
                    self.malformed()?;
                    true
                }
                CapturedExportEvent::Frame(blocks) => self.frame(&blocks, now)?,
            };
            if publish {
                publications.push(self.snapshot());
            }
        }
        Ok(publications)
    }

    fn frame(&mut self, blocks: &[CapturedBlock], now: Instant) -> Result<bool, String> {
        let counter = blocks.iter().rev().find_map(|block| {
            let offset =
                (self.spec.counter.address as usize).checked_sub(block.block().address as usize)?;
            let bytes = block.block().data.get(offset..offset + 2)?;
            let captures = block.captures().get(offset..offset + 2)?;
            let latest = captures.iter().max_by_key(|capture| capture.position)?;
            Some((
                (u16::from_le_bytes([bytes[0], bytes[1]]) & self.spec.counter.mask)
                    >> self.spec.counter.shift,
                *latest,
            ))
        });
        let Some((counter, capture)) =
            counter.filter(|(counter, _)| *counter <= self.spec.counter.maximum)
        else {
            self.malformed()?;
            return Ok(true);
        };
        if !now
            .checked_duration_since(capture.captured_at)
            .is_some_and(|age| age.as_millis() <= self.spec.freshness_ms as u128)
        {
            self.reset("waiting", true)?;
            return Ok(true);
        }
        let observation = Some(CounterObservation {
            acquisition_id: self.snapshot.memory.acquisition_id().into(),
            position: capture.position,
            captured_at: capture.captured_at,
        });
        let advance = self
            .snapshot
            .counter
            .map(|previous| counter.wrapping_sub(previous) & 255);
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
        candidate.apply(blocks)?;
        let name = candidate.string(self.spec.aircraft_address, self.spec.aircraft_length);
        let shutdown = name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty() || name == "NONE");
        if advance == Some(0) {
            if shutdown && self.snapshot.aircraft.is_some() {
                self.reset("waiting", true)?;
                self.snapshot.aircraft = None;
                self.snapshot.status = "stopped";
                self.snapshot.counter_observation = observation;
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
            self.snapshot.counter_observation = observation;
            return Ok(true);
        }
        let identity_changed = name
            .as_ref()
            .is_some_and(|name| self.snapshot.aircraft.as_ref() != Some(name));
        let can_live = advance == Some(1) && self.qualified && !gap && !identity_changed;
        if identity_changed {
            self.reset("waiting", false)?;
            let mut current = CapturedMemory::new(candidate.acquisition_id().into());
            current.apply(blocks)?;
            // A changed aircraft may be expressed by a sparse header update.
            // Only its now-complete candidate header survives the old context.
            current.copy_range_from(&candidate, range)?;
            candidate = current;
        }
        self.qualified = name.is_some();
        if name.is_some() {
            self.snapshot.aircraft = name;
        }
        self.snapshot.memory = Arc::new(candidate);
        self.snapshot.counter = Some(counter);
        self.snapshot.counter_observation = observation;
        self.snapshot.status = if can_live && self.qualified {
            "live"
        } else {
            "waiting"
        };
        Ok(true)
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

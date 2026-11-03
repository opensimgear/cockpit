use super::{
    parse_for_game, schema, CaptureSet, Channel, ChannelStatus, Context, Game, RawSample, Snapshot,
    Source, SourceIdentity, CHANNELS,
};
use std::collections::BTreeMap;
use std::time::Instant;

pub(crate) struct Session {
    game: Game,
    freshness_ms: u64,
    epoch: u64,
    source: Option<SourceIdentity>,
    context: Option<Context>,
    packet_id: Option<u32>,
    capture: Option<CaptureSet>,
    last_native_capture: Option<CaptureSet>,
    producer_highwater: Option<(String, u64, u64)>,
    capture_mode: Option<bool>,
    capture_scope: Option<(String, u64)>,
    last_seen: Option<u64>,
    qualified: bool,
    live: bool,
    ever_fresh: bool,
    status: String,
    channels: BTreeMap<String, Channel>,
    error: Option<String>,
    exhausted: bool,
}
impl Session {
    pub fn new(freshness_ms: u64) -> Result<Self, String> {
        Self::new_for_game(freshness_ms, Game::Rally)
    }
    pub fn new_for_game(freshness_ms: u64, game: Game) -> Result<Self, String> {
        if !(100..=10000).contains(&freshness_ms) {
            return Err("Telemetry freshness must be 100..10000 milliseconds.".into());
        }
        Ok(Self {
            game,
            freshness_ms,
            epoch: 1,
            source: None,
            context: None,
            packet_id: None,
            capture: None,
            last_native_capture: None,
            producer_highwater: None,
            capture_mode: None,
            capture_scope: None,
            last_seen: None,
            qualified: false,
            live: false,
            ever_fresh: false,
            status: "waiting".into(),
            channels: BTreeMap::new(),
            error: None,
            exhausted: false,
        })
    }
    fn disarm(&mut self, status: &str) {
        match self.epoch.checked_add(1) {
            Some(epoch) => {
                self.epoch = epoch;
                if epoch == u64::MAX {
                    self.exhausted = true;
                }
            }
            None => self.exhausted = true,
        }
        self.qualified = false;
        self.channels.clear();
        self.capture = None;
        if self.exhausted {
            self.status = "error".into();
            self.error = Some("Assetto telemetry session generation exhausted.".into());
        } else {
            self.status = status.into();
        }
    }
    pub fn reset(&mut self) {
        self.disarm("waiting");
        self.source = None;
        self.context = None;
        self.packet_id = None;
        self.capture_mode = None;
        self.capture_scope = None;
        // Reset withdraws authority; an earlier capture is still not a new
        // baseline when this same native owner is observed again.
        self.last_seen = None;
        self.live = false;
        self.ever_fresh = false;
        if !self.exhausted {
            self.error = None;
        }
    }
    pub fn tick(&mut self, now_ms: u64) -> Snapshot {
        self.tick_at(now_ms, Instant::now())
    }
    fn tick_at(&mut self, now_ms: u64, observed_at: Instant) -> Snapshot {
        if self.exhausted {
            return self.snapshot_at(now_ms, observed_at);
        }
        let stale = if let Some(capture) = &self.capture {
            !capture.current_at(observed_at, self.freshness_ms)
        } else if self.capture_mode == Some(true) {
            false
        } else {
            self.qualified
                && self
                    .last_seen
                    .is_some_and(|last| now_ms < last || now_ms - last > self.freshness_ms)
        };
        if self.qualified && stale {
            self.disarm("stale");
        }
        self.snapshot_at(now_ms, observed_at)
    }
    pub fn feed(&mut self, sample: &RawSample, now_ms: u64) -> Snapshot {
        self.feed_at(sample, now_ms, Instant::now())
    }
    pub(crate) fn feed_at(
        &mut self,
        sample: &RawSample,
        now_ms: u64,
        observed_at: Instant,
    ) -> Snapshot {
        if self.exhausted {
            return self.snapshot_at(now_ms, observed_at);
        }
        self.tick_at(now_ms, observed_at);
        if self.exhausted {
            return self.snapshot_at(now_ms, observed_at);
        }
        if let Some(capture) = &sample.capture {
            if !capture.current_at(observed_at, self.freshness_ms) {
                return self.failed(
                    "Assetto native page evidence is invalid, future, or stale.".into(),
                    now_ms,
                    observed_at,
                );
            }
        }
        let capture_advanced = match self.capture_progress(sample.capture.as_ref()) {
            Ok(advanced) => advanced,
            Err(error) => return self.failed(error, now_ms, observed_at),
        };
        let reading = match parse_for_game(sample, self.game) {
            Ok(reading) => reading,
            Err(error) => return self.failed(error, now_ms, observed_at),
        };
        let capture_scope = sample
            .capture
            .as_ref()
            .map(|capture| (capture.producer_id.clone(), capture.generation));
        let source_changed = self.source.as_ref() != Some(&sample.source);
        if !source_changed
            && self
                .capture_mode
                .is_some_and(|native| native != sample.capture.is_some())
        {
            return self.failed(
                "Assetto capture mode changed within a source generation.".into(),
                now_ms,
                observed_at,
            );
        }
        let scope_changed = source_changed
            || self.context.as_ref() != Some(&reading.context)
            || self.capture_scope != capture_scope;
        if scope_changed {
            if sample.capture.is_some() && !capture_advanced {
                return self.failed(
                    "Assetto baseline reused native page evidence.".into(),
                    now_ms,
                    observed_at,
                );
            }
            self.disarm(if reading.telemetry_active() {
                "waiting"
            } else {
                "inactive"
            });
            if self.exhausted {
                return self.snapshot_at(now_ms, observed_at);
            }
            self.source = Some(sample.source.clone());
            if source_changed || self.capture_mode.is_none() {
                self.capture_mode = Some(sample.capture.is_some());
            }
            self.live = reading.telemetry_active();
            self.context = Some(reading.context);
            self.packet_id = Some(reading.packet_id);
            self.capture_scope = capture_scope;
            self.remember_capture(sample.capture.as_ref());
            self.last_seen = None;
            self.ever_fresh = false;
            self.error = None;
            return self.snapshot_at(now_ms, observed_at);
        }
        if capture_advanced {
            self.remember_capture(sample.capture.as_ref());
        }
        if !reading.telemetry_active() {
            if self.live || self.qualified || self.status != "inactive" {
                self.disarm("inactive");
                if self.exhausted {
                    return self.snapshot_at(now_ms, observed_at);
                }
            }
            self.live = false;
            self.packet_id = Some(reading.packet_id);
            self.capture = None;
            self.error = None;
            return self.snapshot_at(now_ms, observed_at);
        }
        if !self.live {
            self.disarm("waiting");
            if self.exhausted {
                return self.snapshot_at(now_ms, observed_at);
            }
            self.live = true;
            self.packet_id = Some(reading.packet_id);
            self.capture = None;
            self.last_seen = None;
            self.error = None;
            return self.snapshot_at(now_ms, observed_at);
        }
        let advance = reading.packet_id.wrapping_sub(self.packet_id.unwrap());
        if advance == 0 {
            return self.snapshot_at(now_ms, observed_at);
        }
        if self.capture_mode == Some(true) && !capture_advanced {
            return self.failed(
                "Assetto advancing packet reused native page evidence.".into(),
                now_ms,
                observed_at,
            );
        }
        if advance >= 0x80000000 {
            self.disarm("waiting");
            if self.exhausted {
                return self.snapshot_at(now_ms, observed_at);
            }
            self.packet_id = Some(reading.packet_id);
            self.last_seen = None;
            self.error = None;
            return self.snapshot_at(now_ms, observed_at);
        }
        self.packet_id = Some(reading.packet_id);
        self.last_seen = Some(now_ms);
        self.qualified = true;
        self.ever_fresh = true;
        self.channels = reading.channels;
        self.capture = sample.capture.clone();
        self.last_native_capture = sample.capture.clone();
        self.status = "active".into();
        self.error = None;
        self.snapshot_at(now_ms, observed_at)
    }
    fn failed(&mut self, error: String, now_ms: u64, observed_at: Instant) -> Snapshot {
        if self.status != "error" {
            self.disarm("error");
        }
        self.error = Some(error);
        self.snapshot_at(now_ms, observed_at)
    }
    pub fn poll(&mut self, source: &mut dyn Source, now_ms: u64) -> Snapshot {
        match source.poll() {
            Ok(Some(sample)) => self.feed(&sample, now_ms),
            Ok(None) => self.tick(now_ms),
            Err(error) => {
                source.close();
                self.failed(error, now_ms, Instant::now())
            }
        }
    }
    pub fn snapshot(&self, now_ms: u64) -> Snapshot {
        self.snapshot_at(now_ms, Instant::now())
    }
    fn snapshot_at(&self, now_ms: u64, observed_at: Instant) -> Snapshot {
        let fresh = self.qualified
            && !self.exhausted
            && self.live
            && match &self.capture {
                Some(capture) => capture.current_at(observed_at, self.freshness_ms),
                None if self.capture_mode == Some(true) => false,
                None => self
                    .last_seen
                    .is_some_and(|last| now_ms >= last && now_ms - last <= self.freshness_ms),
            };
        let channels = CHANNELS
            .iter()
            .copied()
            .chain(schema::ids(self.game))
            .map(|id| {
                let channel = if fresh {
                    self.channels
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| Channel::unknown("No current channel value."))
                } else {
                    Channel {
                        value: None,
                        status: if self.ever_fresh {
                            ChannelStatus::Stale
                        } else {
                            ChannelStatus::Unknown
                        },
                        error: None,
                    }
                };
                (id.to_string(), channel)
            })
            .collect();
        Snapshot {
            game: self.game,
            epoch: self.epoch,
            fresh,
            age_ms: match &self.capture {
                Some(capture) => capture_age_ms(capture, observed_at),
                None if self.capture_mode == Some(true) => None,
                None => self.last_seen.map(|last| now_ms.saturating_sub(last)),
            },
            status: if !fresh && self.qualified {
                "stale".into()
            } else {
                self.status.clone()
            },
            source: self.source.clone(),
            context: self.context.clone(),
            packet_id: self.packet_id,
            capture: self.capture.clone(),
            channels,
            error: self.error.clone(),
        }
    }

    fn capture_progress(&self, incoming: Option<&CaptureSet>) -> Result<bool, String> {
        let Some(incoming) = incoming else {
            return Ok(false);
        };
        let Some((producer_id, generation, sequence)) = &self.producer_highwater else {
            return Ok(true);
        };
        if incoming.producer_id != *producer_id {
            return Ok(true);
        }
        if incoming.generation < *generation {
            return Err("Assetto native generation regressed within one producer.".into());
        }
        if incoming.generation > *generation {
            if incoming.physics.sequence <= *sequence {
                return Err("Assetto remap reused an earlier native capture sequence.".into());
            }
            if self.last_native_capture.as_ref().is_some_and(|previous| {
                incoming.physics.captured_at < previous.physics.captured_at
                    || incoming.graphics.captured_at < previous.graphics.captured_at
                    || incoming.statics.captured_at < previous.statics.captured_at
            }) {
                return Err("Assetto remap regressed native acquisition time.".into());
            }
            return Ok(true);
        }
        if let Some(previous) = &self.last_native_capture {
            if incoming == previous {
                return Ok(false);
            }
            if !incoming.advances_after(previous) {
                return Err("Assetto native page evidence regressed within its generation.".into());
            }
            return Ok(true);
        }
        if incoming.physics.sequence <= *sequence {
            return Err("Assetto native capture sequence regressed within its generation.".into());
        }
        Ok(true)
    }

    fn remember_capture(&mut self, incoming: Option<&CaptureSet>) {
        let Some(incoming) = incoming else {
            self.last_native_capture = None;
            return;
        };
        let sequence = incoming.statics.sequence;
        match &mut self.producer_highwater {
            Some((producer_id, generation, high_sequence))
                if *producer_id == incoming.producer_id =>
            {
                *generation = (*generation).max(incoming.generation);
                *high_sequence = (*high_sequence).max(sequence);
            }
            _ => {
                self.producer_highwater =
                    Some((incoming.producer_id.clone(), incoming.generation, sequence));
            }
        }
        self.last_native_capture = Some(incoming.clone());
    }
}

fn capture_age_ms(capture: &CaptureSet, observed_at: Instant) -> Option<u64> {
    if !capture.valid() {
        return None;
    }
    let oldest = [capture.physics, capture.graphics, capture.statics]
        .into_iter()
        .map(|page| page.captured_at)
        .min()?;
    observed_at
        .checked_duration_since(oldest)
        .map(|age| u64::try_from(age.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
#[path = "session_capture_tests.rs"]
mod tests;

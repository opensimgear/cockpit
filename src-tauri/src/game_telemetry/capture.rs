use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Physics,
    Graphics,
    Statics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageCapture {
    pub sequence: u64,
    pub captured_at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CaptureSet {
    pub producer_id: String,
    pub generation: u64,
    pub physics: PageCapture,
    pub graphics: PageCapture,
    pub statics: PageCapture,
}

impl CaptureSet {
    pub fn new(
        owner: &CaptureOwner,
        physics: PageCapture,
        graphics: PageCapture,
        statics: PageCapture,
    ) -> Result<Self, String> {
        let set = Self {
            producer_id: owner.producer_id.clone(),
            generation: owner.generation,
            physics,
            graphics,
            statics,
        };
        if !set.valid() {
            return Err("Invalid Assetto page capture evidence.".into());
        }
        Ok(set)
    }

    pub fn page(&self, page: Page) -> PageCapture {
        match page {
            Page::Physics => self.physics,
            Page::Graphics => self.graphics,
            Page::Statics => self.statics,
        }
    }

    pub fn valid(&self) -> bool {
        uuid(&self.producer_id)
            && self.generation > 0
            && self.physics.sequence > 0
            && self.graphics.sequence > 0
            && self.statics.sequence > 0
            && self.physics.sequence < self.graphics.sequence
            && self.graphics.sequence < self.statics.sequence
            && self.physics.captured_at <= self.graphics.captured_at
            && self.graphics.captured_at <= self.statics.captured_at
    }

    pub fn advances_after(&self, previous: &Self) -> bool {
        self.producer_id == previous.producer_id
            && self.generation == previous.generation
            && self.physics.sequence > previous.physics.sequence
            && self.graphics.sequence > previous.graphics.sequence
            && self.statics.sequence > previous.statics.sequence
            && self.physics.captured_at >= previous.physics.captured_at
            && self.graphics.captured_at >= previous.graphics.captured_at
            && self.statics.captured_at >= previous.statics.captured_at
    }

    pub fn current_at(&self, observed_at: Instant, freshness_ms: u64) -> bool {
        if !self.valid() {
            return false;
        }
        let limit = Duration::from_millis(freshness_ms);
        [self.physics, self.graphics, self.statics]
            .into_iter()
            .all(|capture| {
                observed_at >= capture.captured_at
                    && observed_at.duration_since(capture.captured_at) <= limit
            })
    }
}

pub(crate) struct CaptureOwner {
    pub producer_id: String,
    pub generation: u64,
    sequence: u64,
}

impl CaptureOwner {
    pub fn new() -> Self {
        Self {
            producer_id: uuid::Uuid::new_v4().to_string(),
            generation: 0,
            sequence: 0,
        }
    }

    pub fn begin_generation(&mut self) -> Result<u64, String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("Assetto native generation exhausted.")?;
        Ok(self.generation)
    }

    pub fn capture(&mut self, captured_at: Instant) -> Result<PageCapture, String> {
        if self.generation == 0 {
            return Err("Assetto mappings have no native generation.".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Assetto native capture sequence exhausted.")?;
        Ok(PageCapture {
            sequence: self.sequence,
            captured_at,
        })
    }
}

fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| !parsed.is_nil() && parsed.to_string() == value)
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;

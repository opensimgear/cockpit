//! Capture-only learning consumes the native journal independently of UI polling.
use osg_controller_input::{registry::Kind, stream};
use serde_json::{json, Value};
pub struct Learn {
    pub session: String,
    pub cursor: u64,
    source: stream::SourceSnapshot,
    until: u64,
    phase: &'static str,
    reason: String,
    candidates: Vec<String>,
}
impl Learn {
    pub fn new(mut snapshot: stream::Snapshot) -> Result<Self, String> {
        if snapshot.sources.len() != 1
            || snapshot.sources[0].baseline.reason != stream::Reason::Baseline
        {
            return Err(
                "Learning requires one physical source with a complete current baseline.".into(),
            );
        }
        let source = snapshot.sources.remove(0);
        Ok(Self {
            session: snapshot.session,
            cursor: snapshot.cursor,
            source,
            until: snapshot.captured_us.saturating_add(30_000_000),
            phase: "listening",
            reason: String::new(),
            candidates: vec![],
        })
    }
    pub fn active(&self) -> bool {
        self.phase == "listening"
    }
    fn fail(&mut self, reason: &str) {
        self.phase = "stopped";
        self.reason = reason.into();
    }
    pub fn pump(&mut self, page: stream::Page) {
        if !self.active() {
            return;
        }
        if page.session != self.session || page.gap {
            self.fail("Capture history changed; restart learning from a fresh baseline.");
            return;
        }
        self.cursor = page.cursor;
        for event in page.events {
            if event.source != self.source.id {
                continue;
            }
            if event.captured_us > self.until {
                self.fail("Learning timed out.");
                return;
            }
            if event.generation != self.source.baseline.generation
                || event.reason != stream::Reason::Change
                || event.sequence != self.source.baseline.sequence.saturating_add(1)
            {
                self.fail("Selected controller changed or lost reports; restart learning.");
                return;
            }
            self.source.baseline.sequence = event.sequence;
            for control in &self.source.controls {
                let Some(value) = event.values.get(&control.id) else {
                    continue;
                };
                let Some(before) = self.source.baseline.values.get(&control.id) else {
                    continue;
                };
                let changed = match control.kind {
                    Kind::Button => value.normalized == Some(1) && before.normalized != Some(1),
                    Kind::Axis => value
                        .normalized
                        .zip(before.normalized)
                        .is_some_and(|(a, b)| a.abs_diff(b) >= 2048),
                    Kind::Hat => {
                        value.normalized.is_some() && value.normalized != before.normalized
                    }
                    Kind::Diagnostic => false,
                };
                if changed && self.candidates.len() < 32 {
                    self.candidates.push(control.id.clone());
                }
                if control.kind != Kind::Axis {
                    self.source
                        .baseline
                        .values
                        .insert(control.id.clone(), value.clone());
                }
            }
            if !self.candidates.is_empty() {
                self.phase = "learned";
                self.reason = if self.candidates.len() == 1 {
                    "Control captured. Save the assignment explicitly."
                } else {
                    "Several controls changed together. Choose the intended control explicitly."
                }
                .into();
                return;
            }
        }
        if page.captured_us >= self.until {
            self.fail("Learning timed out without a deliberate control change.");
        }
    }
    pub fn status(&self) -> Value {
        json!({"sourceId":self.source.id,"phase":self.phase,"reason":self.reason,"candidates":self.candidates})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use osg_controller_input::registry::{Control, Sample};
    use std::collections::BTreeMap;
    fn snapshot() -> stream::Snapshot {
        stream::Snapshot {
            session: "session".into(),
            cursor: 1,
            captured_us: 1,
            sources: vec![stream::SourceSnapshot {
                id: "physical".into(),
                controls: vec![
                    Control::new(9, 1, 0, 0, 1),
                    Control::new(1, 0x30, 0, 0, 2),
                    Control::new(1, 0x39, 0, 0, 3),
                ],
                baseline: stream::Event {
                    cursor: 1,
                    source: "physical".into(),
                    generation: 1,
                    sequence: 1,
                    captured_us: 1,
                    reason: stream::Reason::Baseline,
                    values: BTreeMap::new(),
                },
            }],
        }
    }
    fn setup() -> Learn {
        let mut s = snapshot();
        for c in s.sources[0].controls.clone() {
            s.sources[0].baseline.values.insert(
                c.id.clone(),
                Sample {
                    native: 0,
                    normalized: Some(0),
                },
            );
        }
        Learn::new(s).unwrap()
    }
    fn page(l: &Learn) -> stream::Page {
        let mut event = l.source.baseline.clone();
        event.sequence += 1;
        event.reason = stream::Reason::Change;
        stream::Page {
            session: l.session.clone(),
            cursor: 2,
            captured_us: 10,
            gap: false,
            events: vec![event],
        }
    }
    #[test]
    fn learns_first_deliberate_change_without_output_and_reports_ambiguity() {
        let mut l = setup();
        let mut p = page(&l);
        p.events[0]
            .values
            .get_mut(&l.source.controls[1].id)
            .unwrap()
            .normalized = Some(2047);
        l.pump(p);
        assert!(l.active());
        let mut p = page(&l);
        p.events[0]
            .values
            .get_mut(&l.source.controls[0].id)
            .unwrap()
            .normalized = Some(1);
        l.pump(p);
        assert_eq!(l.status()["candidates"].as_array().unwrap().len(), 1);
        assert!(!l.active());
        l.pump(page(&l));
        let mut l = setup();
        let mut p = page(&l);
        for v in p.events[0].values.values_mut() {
            v.normalized = Some(3000);
        }
        p.events[0]
            .values
            .get_mut(&l.source.controls[0].id)
            .unwrap()
            .normalized = Some(1);
        l.pump(p);
        assert!(l.status()["candidates"].as_array().unwrap().len() > 1);
    }
    #[test]
    fn loss_and_timeout_require_explicit_restart() {
        assert!(Learn::new(stream::Snapshot {
            sources: vec![],
            ..snapshot()
        })
        .is_err());
        for mode in 0..5 {
            let mut l = setup();
            let mut p = page(&l);
            match mode {
                0 => p.gap = true,
                1 => p.session = "new".into(),
                2 => p.events[0].generation = 2,
                3 => p.events[0].sequence = 9,
                _ => p.captured_us = 30_000_001,
            }
            l.pump(p);
            assert_eq!(l.status()["phase"], "stopped");
        }
        let mut l = setup();
        let mut p = page(&l);
        p.events[0].source = "unrelated".into();
        l.pump(p);
        assert!(l.active());
    }
}

use osg_controller_input::{
    registry::{Control, Kind as NativeKind, Sample},
    stream,
};
use osg_mapping::{
    engine::{self, Assignment, Change, Counter, Frame, RunToken, Source},
    profile::{AxisRange, Kind, Origin, Profile, Value},
    runtime::Runtime,
};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Router {
    pub runtime: Runtime,
    session: String,
    cursor: u64,
    now: u64,
    expected: BTreeMap<Source, BTreeMap<String, Kind>>,
    synthetic_sequences: BTreeMap<Source, u64>,
    fault: Option<String>,
}
fn native_kind(control: &Control) -> Option<Kind> {
    match control.kind {
        NativeKind::Button => Some(Kind::Button),
        NativeKind::Axis => Some(Kind::Axis {
            range: if control.min < 0 {
                AxisRange::Bipolar
            } else {
                AxisRange::Unipolar
            },
        }),
        NativeKind::Hat => Some(Kind::Hat),
        NativeKind::Diagnostic => None,
    }
}
fn value(kind: Kind, sample: &Sample) -> Result<Value, String> {
    let result = match kind {
        Kind::Button => match sample.normalized {
            Some(0) => Value::Button(false),
            Some(1) => Value::Button(true),
            _ => return Err("Invalid captured button.".into()),
        },
        Kind::Axis { .. } => Value::Axis(
            sample
                .normalized
                .and_then(|v| i32::try_from(v).ok())
                .ok_or("Invalid captured axis.")?,
        ),
        Kind::Hat => Value::Hat(
            sample
                .normalized
                .map(u8::try_from)
                .transpose()
                .map_err(|_| "Invalid captured hat.")?,
        ),
    };
    if !kind.accepts(result) {
        return Err("Captured value does not match the assigned control.".into());
    }
    Ok(result)
}
impl Router {
    pub fn start(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
    ) -> Result<RunToken, String> {
        engine::validate_assignments(&profile, &assignments)?;
        if snapshot.session.is_empty() || snapshot.captured_us == 0 {
            return Err("Invalid capture session.".into());
        }
        let mut expected: BTreeMap<Source, BTreeMap<String, Kind>> = BTreeMap::new();
        for assignment in &assignments {
            let role = profile
                .roles
                .iter()
                .find(|r| r.id == assignment.role)
                .ok_or("Unknown role.")?;
            if assignment.source.origin == Origin::OsgPanelEvents {
                return Err(
                    "Panel-event mapping is not available until game integration qualification."
                        .into(),
                );
            }
            for binding in &assignment.controls {
                let control = role
                    .controls
                    .iter()
                    .find(|c| c.id == binding.control)
                    .ok_or("Unknown role control.")?;
                if assignment.source.origin == Origin::HostController {
                    let source = snapshot
                        .sources
                        .iter()
                        .find(|s| s.id == assignment.source.id)
                        .ok_or(
                            "Assigned controller is not ready; acquire a complete baseline first.",
                        )?;
                    if source
                        .controls
                        .iter()
                        .find(|c| c.id == binding.source_control)
                        .and_then(native_kind)
                        != Some(control.kind)
                    {
                        return Err("Assigned controller capability differs from the profile; edit the assignment explicitly.".into());
                    }
                }
                expected
                    .entry(assignment.source.clone())
                    .or_default()
                    .insert(binding.source_control.clone(), control.kind);
            }
        }
        // Build every baseline before replacing the existing run.
        let mut baselines = Vec::new();
        let mut sequences = BTreeMap::new();
        for (source, controls) in &expected {
            let (generation, sequence, changes) = if source.origin == Origin::Synthetic {
                sequences.insert(source.clone(), 1);
                (
                    1,
                    1,
                    controls
                        .iter()
                        .map(|(id, kind)| Change {
                            control: id.clone(),
                            value: match kind {
                                Kind::Button => Value::Button(false),
                                Kind::Axis { .. } => Value::Axis(0),
                                Kind::Hat => Value::Hat(None),
                            },
                        })
                        .collect(),
                )
            } else {
                let event = &snapshot
                    .sources
                    .iter()
                    .find(|s| s.id == source.id)
                    .ok_or("Missing source baseline.")?
                    .baseline;
                if event.reason != stream::Reason::Baseline
                    || event.generation == 0
                    || event.sequence == 0
                {
                    return Err("Source baseline is not ready.".into());
                }
                (
                    event.generation,
                    event.sequence,
                    Self::changes(controls, event)?,
                )
            };
            baselines.push(Frame {
                source: source.clone(),
                generation: Counter(generation),
                sequence: Counter(sequence),
                captured_us: Counter(snapshot.captured_us),
                reason: engine::Reason::Baseline,
                changes,
            });
        }
        let token = self.runtime.start(profile, assignments)?;
        self.expected = expected;
        self.synthetic_sequences = sequences;
        self.session = snapshot.session;
        self.cursor = snapshot.cursor;
        self.now = snapshot.captured_us;
        self.fault = None;
        for frame in baselines {
            if let Err(error) = self.runtime.accept_for(&token, frame) {
                return self.fail(error);
            }
        }
        Ok(token)
    }
    fn changes(
        controls: &BTreeMap<String, Kind>,
        event: &stream::Event,
    ) -> Result<Vec<Change>, String> {
        controls
            .iter()
            .map(|(id, kind)| {
                Ok(Change {
                    control: id.clone(),
                    value: value(
                        *kind,
                        event
                            .values
                            .get(id)
                            .ok_or("Incomplete captured source report.")?,
                    )?,
                })
            })
            .collect()
    }
    fn fail<T>(&mut self, error: String) -> Result<T, String> {
        self.runtime.stop();
        self.fault = Some(error.clone());
        Err(error)
    }
    pub fn cursor(&self) -> (&str, u64) {
        (&self.session, self.cursor)
    }
    pub fn pump(&mut self, page: stream::Page) -> Result<(), String> {
        let Ok(token) = self.runtime.token() else {
            return Ok(());
        };
        let result = (|| {
            if page.gap
                || page.session != self.session
                || page.cursor < self.cursor
                || page.captured_us < self.now
            {
                return Err(
                    "Capture history was lost; mapping stopped. Start again with fresh baselines."
                        .into(),
                );
            }
            for event in page.events {
                let source = Source {
                    origin: Origin::HostController,
                    id: event.source.clone(),
                };
                let Some(controls) = self.expected.get(&source) else {
                    continue;
                };
                let lost = matches!(
                    event.reason,
                    stream::Reason::Gap | stream::Reason::Disconnect
                );
                let changes = if lost {
                    Vec::new()
                } else {
                    Self::changes(controls, &event)?
                };
                let reason = match event.reason {
                    stream::Reason::Baseline => engine::Reason::Baseline,
                    stream::Reason::Change => engine::Reason::Change,
                    stream::Reason::Gap => engine::Reason::Gap,
                    stream::Reason::Disconnect => engine::Reason::Disconnect,
                };
                self.runtime.accept_for(
                    &token,
                    Frame {
                        source,
                        generation: Counter(event.generation),
                        sequence: Counter(event.sequence.max(1)),
                        captured_us: Counter(event.captured_us),
                        reason,
                        changes,
                    },
                )?;
            }
            self.runtime
                .advance_for(&token, Counter(page.captured_us))?;
            self.cursor = page.cursor;
            self.now = page.captured_us;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error),
        }
    }
    pub fn simulate(
        &mut self,
        token: &RunToken,
        source: &Source,
        changes: Vec<Change>,
    ) -> Result<(), String> {
        if &self.runtime.token()? != token {
            return Err("Stale preview session.".into());
        }
        let sequence = self
            .synthetic_sequences
            .get_mut(source)
            .ok_or("Only explicitly assigned synthetic sources accept simulated input.")?;
        let next = sequence
            .checked_add(1)
            .ok_or("Synthetic sequence exhausted.")?;
        self.runtime.accept_for(
            token,
            Frame {
                source: source.clone(),
                generation: Counter(1),
                sequence: Counter(next),
                captured_us: Counter(self.now),
                reason: engine::Reason::Change,
                changes,
            },
        )?;
        *sequence = next;
        Ok(())
    }
    pub fn stop(&mut self) {
        self.runtime.stop();
    }
    pub fn status(&self) -> serde_json::Value {
        let engine = self.runtime.engine();
        json!({"running":engine.is_some_and(|e| e.active()),"ready":engine.is_some_and(|e| e.ready()),"mode":"recording","token":self.runtime.token().ok(),"profileId":engine.map(|e| &e.profile().id),"fault":self.fault.as_deref().or_else(|| engine.and_then(|e| e.fault())),"recordingCursor":self.runtime.recording.cursor()})
    }
}
#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;

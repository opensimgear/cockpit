//! Native HID history adapted to the shared typed reducer. Inventory pages and
//! browser snapshots never supply values. A fresh native snapshot repairs a
//! journal gap without replaying the missing transitions.
use osg_controller_input::{registry::Control, stream};
use osg_mapping::{
    engine::{
        typed::{Change, Frame, Phase},
        Counter, Reason,
    },
    program::SignalSample,
};
use osg_profiles::compiler_models::{ExecutionPlan, PlanReadiness};
use osg_profiles::resources::{ResourceToken, Transport};
use std::collections::BTreeMap;

/// The existing native stream is the only production implementation. Keeping
/// capture separate lets continuity tests use actual stream snapshots/pages.
pub(crate) trait HidCapture {
    fn snapshot(&self, ids: &[String]) -> Result<stream::Snapshot, String>;
    fn page(&self, session: &str, cursor: u64) -> stream::Page;
    fn watch_reports(&self, ids: &[String], wake: std::sync::Arc<dyn Fn() + Send + Sync>)
        -> Result<Option<stream::ReportWatch>, String>;
}
impl HidCapture for osg_controller_input::ControllerService {
    fn snapshot(&self, ids: &[String]) -> Result<stream::Snapshot, String> {
        self.stream_snapshot(ids)
    }
    fn page(&self, session: &str, cursor: u64) -> stream::Page {
        self.stream_page(session, cursor)
    }
    fn watch_reports(&self, ids: &[String], wake: std::sync::Arc<dyn Fn() + Send + Sync>)
        -> Result<Option<stream::ReportWatch>, String> {
        osg_controller_input::ControllerService::watch_reports(self, ids, wake).map(Some)
    }
}

#[derive(Clone)]
struct Port {
    logical: String,
    observed: String,
    token: ResourceToken,
}
#[derive(Clone)]
struct Source {
    logical: String,
    physical: String,
    ports: Vec<Port>,
    controls: Vec<Control>,
    generation: u64,
    provider_sequence: u64,
    sequence: u64,
    at: u64,
    ready: bool,
}
pub(crate) struct HidInputs {
    sources: Vec<Source>,
    session: String,
    cursor: u64,
    watch: Option<stream::ReportWatch>,
}
impl HidInputs {
    pub(crate) fn capture(
        plan: &ExecutionPlan,
        controllers: &impl HidCapture,
        wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<(Self, Vec<Frame>), String> {
        let mut sources: Vec<Source> = vec![];
        for source in &plan.sources {
            if source.readiness != PlanReadiness::Ready {
                continue;
            }
            let Some(route) = source
                .route
                .as_ref()
                .filter(|route| route.transport == Transport::Hid)
            else {
                continue;
            };
            let physical = route
                .id
                .strip_prefix("hid:")
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                .ok_or("The HID route has no exact retained physical identity.")?;
            let token = source
                .token
                .as_ref()
                .ok_or("A native HID delivery token is required.")?;
            validate(token)?;
            let port = Port {
                logical: source.endpoint.port_id.clone(),
                observed: source
                    .observed_port_id
                    .clone()
                    .ok_or("The selected HID control is missing.")?,
                token: token.clone(),
            };
            if let Some(existing) = sources
                .iter_mut()
                .find(|item| item.logical == source.endpoint.node_id)
            {
                if existing.physical != physical {
                    return Err("One logical source cannot mix physical HID identities.".into());
                }
                existing.ports.push(port);
            } else {
                sources.push(Source {
                    logical: source.endpoint.node_id.clone(),
                    physical: physical.into(),
                    ports: vec![port],
                    controls: vec![],
                    generation: 1,
                    provider_sequence: 0,
                    sequence: 0,
                    at: 0,
                    ready: false,
                });
            }
        }
        let mut reader = Self {
            sources,
            session: String::new(),
            cursor: 0,
            watch: None,
        };
        if !reader.sources.is_empty() {
            if let Some(wake) = wake { reader.watch = controllers.watch_reports(&reader.ids(), wake)?; }
        }
        let frames = reader.baseline(controllers, &validate)?;
        Ok((reader, frames))
    }
    fn ids(&self) -> Vec<String> {
        self.sources
            .iter()
            .map(|source| source.physical.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    /// Native connection evidence keeps latched HID state alive without
    /// manufacturing samples or replaying the snapshot's baseline as an edge.
    pub(crate) fn refresh_liveness(
        &self,
        capture: &osg_controller_input::ControllerService,
        reducer: &mut Phase,
        now: Counter,
    ) -> Result<(), String> {
        self.refresh_verified_liveness(capture, reducer, now)
    }
    fn refresh_verified_liveness(
        &self,
        capture: &impl HidCapture,
        reducer: &mut Phase,
        now: Counter,
    ) -> Result<(), String> {
        if self.sources.is_empty() { return Ok(()); }
        // ControllerService snapshots admit only online sources with a complete
        // current baseline, under the same native registry lock as disconnects.
        let snapshot = capture.snapshot(&self.ids())?;
        if snapshot.session != self.session || snapshot.captured_us < now.0 {
            return Err("Native HID liveness belongs to an old capture session or clock.".into());
        }
        for source in &self.sources {
            if !source.ready { continue; }
            let current = snapshot.sources.iter().find(|current| current.id == source.physical)
                .ok_or("The native HID liveness snapshot is incomplete.")?;
            if current.baseline.reason != stream::Reason::Baseline
                || current.baseline.generation != source.generation
                || current.controls != source.controls
                || source.ports.iter().any(|port| port.token.provider_generation != current.baseline.generation.to_string())
            {
                return Err("Native HID liveness no longer matches the admitted route generation or controls.".into());
            }
            if current.baseline.sequence != source.provider_sequence {
                // New native reports are pending. Preserve their ordered edges;
                // the next journal poll will admit their current values.
                continue;
            }
            if source.ports.iter().any(|port| !current.baseline.values.contains_key(&port.observed)) {
                return Err("Native HID liveness omitted an admitted control.".into());
            }
            reducer.observe_liveness(&source.logical, Counter(source.generation), now)?;
        }
        Ok(())
    }
    fn baseline(
        &mut self,
        controllers: &impl HidCapture,
        validate: &impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<Vec<Frame>, String> {
        if self.sources.is_empty() {
            return Ok(vec![]);
        }
        let snapshot = controllers.snapshot(&self.ids())?;
        let mut frames = vec![];
        let mut captured = self.sources.clone();
        for source in &mut captured {
            for port in &source.ports {
                validate(&port.token)?;
            }
            let current = snapshot
                .sources
                .iter()
                .find(|current| current.id == source.physical)
                .ok_or("The native source snapshot is incomplete.")?;
            if current.baseline.generation < source.generation
                || current.baseline.captured_us < source.at
                || current.baseline.reason != stream::Reason::Baseline
            {
                return Err("A native baseline predates the admitted source.".into());
            }
            source.controls = current.controls.clone();
            frames.push(source.frame(&current.baseline, true)?);
        }
        self.session = snapshot.session;
        self.cursor = snapshot.cursor;
        if let Some(watch) = &self.watch { watch.advance(self.cursor); }
        self.sources = captured;
        Ok(frames)
    }
    pub(crate) fn poll(
        &mut self,
        controllers: &impl HidCapture,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<(Vec<Frame>, Vec<String>), String> {
        if self.sources.is_empty() {
            return Ok((vec![], vec![]));
        }
        let page = controllers.page(&self.session, self.cursor);
        // The page owns copies of every retained report. A later eviction
        // cannot erase that work, even while the reducer is still admitting it.
        if let Some(watch) = &self.watch { watch.advance(page.cursor); }
        let mut frames = vec![];
        let mut degraded = vec![];
        for source in &mut self.sources {
            if page.gap
                || source
                    .ports
                    .iter()
                    .any(|port| validate(&port.token).is_err())
            {
                frames.push(source.loss(page.captured_us, Reason::Gap)?);
                degraded.push(format!("{} needs a fresh native baseline.", source.logical));
            }
        }
        if page.gap {
            // No missing event is reconstructed. A new complete native capture
            // can seed state, and the reducer suppresses baseline events.
            if let Ok(baselines) = self.baseline(controllers, &validate) {
                frames.extend(baselines);
                degraded.clear();
            }
            return Ok((frames, degraded));
        }
        for event in page.events {
            for source in self
                .sources
                .iter_mut()
                .filter(|source| source.physical == event.source)
            {
                if source
                    .ports
                    .iter()
                    .any(|port| validate(&port.token).is_err())
                {
                    continue;
                }
                let baseline = event.reason == stream::Reason::Baseline;
                if !baseline
                    && (!source.ready
                        || source.generation != event.generation
                        || source.provider_sequence.checked_add(1) != Some(event.sequence))
                {
                    frames.push(source.loss(event.captured_us, Reason::Gap)?);
                    degraded.push(format!("{} lost native report continuity.", source.logical));
                    continue;
                }
                match source.frame(&event, baseline) {
                    Ok(frame) => frames.push(frame),
                    Err(error) => {
                        frames.push(source.loss(event.captured_us, Reason::Gap)?);
                        degraded.push(format!("{}: {error}", source.logical));
                    }
                }
            }
        }
        self.cursor = page.cursor;
        if self.sources.iter().any(|source| !source.ready) {
            if let Ok(baselines) = self.baseline(controllers, &validate) {
                frames.extend(baselines);
                degraded.clear();
            }
        }
        Ok((frames, degraded))
    }
}
impl Source {
    fn next(&mut self) -> Result<u64, String> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Profile source sequence exhausted.")?;
        Ok(self.sequence)
    }
    fn loss(&mut self, at: u64, reason: Reason) -> Result<Frame, String> {
        self.ready = false;
        self.at = self.at.max(at);
        Ok(Frame {
            source: self.logical.clone(),
            generation: Counter(self.generation),
            sequence: Counter(self.next()?),
            captured_us: Counter(self.at),
            reason,
            changes: vec![],
        })
    }
    fn frame(&mut self, event: &stream::Event, baseline: bool) -> Result<Frame, String> {
        if event.captured_us < self.at {
            return Err("A native report predates the admitted source.".into());
        }
        if self
            .ports
            .iter()
            .any(|port| port.token.provider_generation != event.generation.to_string())
        {
            return Err("The native HID generation no longer matches its captured route.".into());
        }
        if matches!(
            event.reason,
            stream::Reason::Gap | stream::Reason::Disconnect
        ) {
            return self.loss(
                event.captured_us,
                if event.reason == stream::Reason::Disconnect {
                    Reason::Disconnect
                } else {
                    Reason::Gap
                },
            );
        }
        let values = event
            .values
            .iter()
            .map(|(id, sample)| (id.as_str(), sample))
            .collect::<BTreeMap<_, _>>();
        let mut changes = vec![];
        for port in &self.ports {
            let sample = match values.get(port.observed.as_str()) {
                Some(sample) => {
                    let control = self
                        .controls
                        .iter()
                        .find(|control| control.id == port.observed)
                        .ok_or("The captured HID control descriptor changed.")?;
                    SignalSample::scalar(osg_mapping_host::typed_sources::controller_value(
                        control, sample,
                    )?)
                }
                None if baseline => {
                    return Err("A native baseline omitted a selected HID control.".into())
                }
                None => continue,
            };
            changes.push(Change {
                port_id: port.logical.clone(),
                sample,
            });
        }
        self.generation = event.generation;
        self.provider_sequence = event.sequence;
        self.at = event.captured_us;
        self.ready = true;
        Ok(Frame {
            source: self.logical.clone(),
            generation: Counter(self.generation),
            sequence: Counter(self.next()?),
            captured_us: Counter(self.at),
            reason: if baseline {
                Reason::Baseline
            } else {
                Reason::Change
            },
            changes,
        })
    }
}

#[cfg(test)]
#[path = "profile_session/inputs_tests.rs"]
mod tests;

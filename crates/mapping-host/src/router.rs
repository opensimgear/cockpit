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
    panels: Option<crate::panels::Feed>,
    companions: Option<crate::companions::Feed>,
    panel_sequences: BTreeMap<Source, (Counter, Counter)>,
    scripts: BTreeMap<String, osg_logic::worker::Worker>,
    pub learner: Option<crate::learn::Learn>,
    pub runtime: Runtime,
    session: String,
    cursor: u64,
    now: u64,
    expected: BTreeMap<Source, BTreeMap<String, Kind>>,
    synthetic_sequences: BTreeMap<Source, u64>,
    fault: Option<String>,
    output: Option<osg_virtual_controller::session::Session>,
    // Output drops first if the whole router is destroyed during unwinding.
    pub(crate) visibility: Option<GameVisibility>,
}
pub(crate) type VisibilityCall = std::sync::Arc<
    dyn Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync,
>;
pub(crate) struct GameVisibility {
    pub owner: String,
    pub call: VisibilityCall,
}
impl GameVisibility {
    fn check(&self) -> Result<(), String> {
        let status = (self.call)("poll", &json!({"owner":self.owner}))?;
        if status["phase"] != "active" {
            return Err(format!(
                "Game visibility stopped: {}",
                status["message"]
                    .as_str()
                    .unwrap_or("restore the game session")
            ));
        }
        Ok(())
    }
    fn stop(&self) -> Result<(), String> {
        (self.call)("stop", &json!({"owner":self.owner})).map(|_| ())
    }
}
impl Drop for GameVisibility {
    fn drop(&mut self) {
        let _ = self.stop();
    }
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
        self.start_prepared(profile, assignments, snapshot, BTreeMap::new())
    }
    pub fn start_prepared(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
        workers: BTreeMap<String, osg_logic::worker::Worker>,
    ) -> Result<RunToken, String> {
        self.start_with_panels(profile, assignments, snapshot, workers, None)
    }
    pub fn start_with_panels(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
        workers: BTreeMap<String, osg_logic::worker::Worker>,
        panels: Option<crate::panels::Feed>,
    ) -> Result<RunToken, String> {
        self.start_with_sources(profile, assignments, snapshot, workers, panels, None)
    }
    pub(crate) fn start_with_sources(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
        workers: BTreeMap<String, osg_logic::worker::Worker>,
        panels: Option<crate::panels::Feed>,
        mut companions: Option<crate::companions::Prepared>,
    ) -> Result<RunToken, String> {
        if workers.len() != profile.scripts.len()
            || profile.scripts.iter().any(|script| {
                workers.get(&script.id).is_none_or(|worker| {
                    worker.source_hash != osg_logic::worker::hash(&script.source)
                })
            })
        {
            return Err("Every script must compile in its own helper before activation.".into());
        }
        if self
            .output
            .as_ref()
            .is_some_and(|o| o.active() || o.recovery_required())
        {
            return Err("Stop or recover live output before replacing this mapping.".into());
        }
        if self.learner.as_ref().is_some_and(|l| l.active()) {
            return Err("Stop input learning before starting a mapping.".into());
        }
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
            if assignment.source.origin == Origin::OsgPanelEvents && panels.is_none() {
                return Err("Panel-event mapping requires an owned capture subscription.".into());
            }
            for binding in &assignment.controls {
                let control = role
                    .controls
                    .iter()
                    .find(|c| c.id == binding.control)
                    .ok_or("Unknown role control.")?;
                if assignment.source.origin == Origin::Companion {
                    if companions
                        .as_ref()
                        .and_then(|prepared| prepared.controls.get(&assignment.source))
                        .and_then(|controls| controls.get(&binding.source_control))
                        != Some(&control.kind)
                    {
                        return Err("Companion mapping requires its registered descriptor and owned producer feed.".into());
                    }
                } else if assignment.source.origin != Origin::Synthetic {
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
            if source.origin == Origin::Companion {
                continue;
            }
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
                captured_us: Counter(if source.origin == Origin::OsgPanelEvents {
                    snapshot
                        .sources
                        .iter()
                        .find(|s| s.id == source.id)
                        .unwrap()
                        .baseline
                        .captured_us
                } else {
                    snapshot.captured_us
                }),
                reason: engine::Reason::Baseline,
                changes,
            });
        }
        if let Some(prepared) = &mut companions {
            baselines.extend(prepared.feed.initial()?);
        }
        let token = self.runtime.start(profile, assignments)?;
        if let Some(trace) = self.runtime.trace.trace.as_mut() {
            trace.metadata["nativeCapabilities"]=serde_json::json!(snapshot.sources.iter().map(|source|serde_json::json!({"id":source.id,"controls":source.controls,"generation":source.baseline.generation.to_string()})).collect::<Vec<_>>());
        }
        self.panels = panels;
        self.companions = companions.map(|prepared| prepared.feed);
        self.panel_sequences = baselines
            .iter()
            .filter(|f| f.source.origin == Origin::OsgPanelEvents)
            .map(|f| (f.source.clone(), (f.generation, f.sequence)))
            .collect();
        self.scripts = workers;
        self.learner = None;
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
        self.poll_scripts();
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
        let error = match self.stop_result() {
            Ok(()) => error,
            Err(recovery) => format!("{error}; {recovery}"),
        };
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
            if let Some(visibility) = &self.visibility {
                visibility.check()?;
            }
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
            self.poll_panels(&token, page.captured_us)?;
            self.poll_companions(&token);
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
                .advance_for(&token, Counter(page.captured_us.max(self.now)))?;
            self.cursor = page.cursor;
            self.now = self.now.max(page.captured_us);
            Ok(())
        })();
        self.poll_scripts();
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error),
        }
    }
    fn poll_panels(&mut self, token: &RunToken, now: u64) -> Result<(), String> {
        let Some(feed) = self.panels.as_mut() else {
            return Ok(());
        };
        let frames = match feed.drain() {
            Ok(frames) => frames,
            Err(error) => {
                self.panels = None;
                self.fault = Some(error);
                self.panel_sequences
                    .iter()
                    .map(|(source, (generation, sequence))| Frame {
                        source: source.clone(),
                        generation: *generation,
                        sequence: Counter(sequence.0.saturating_add(1)),
                        captured_us: Counter(now),
                        reason: engine::Reason::Gap,
                        changes: vec![],
                    })
                    .collect()
            }
        };
        for frame in frames {
            if frame.source.origin != Origin::OsgPanelEvents
                || !self.expected.contains_key(&frame.source)
            {
                return Err("Unexpected panel source on capture subscription.".into());
            }
            self.now = self.now.max(frame.captured_us.0);
            let key = frame.source.clone();
            let sequence = (frame.generation, frame.sequence);
            self.runtime.accept_for(token, frame)?;
            self.panel_sequences.insert(key, sequence);
        }
        Ok(())
    }
    fn poll_companions(&mut self, token: &RunToken) {
        let frames = self
            .companions
            .as_mut()
            .map_or_else(Vec::new, |feed| feed.drain());
        for frame in frames {
            // A bad peer must never stop unrelated physical, panel or companion producers.
            let source = frame.source.clone();
            if source.origin != Origin::Companion || !self.expected.contains_key(&source) {
                continue;
            }
            self.now = self.now.max(frame.captured_us.0);
            if let Err(error) = self.runtime.accept_for(token, frame) {
                self.fault = Some(format!("Companion {}: {error}", source.id));
                if let Some(feed) = &self.companions {
                    feed.reject(&source.id);
                }
            }
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
        self.poll_scripts();
        Ok(())
    }
    pub fn poll_scripts(&mut self) {
        let mut obsolete = Vec::new();
        for (id, worker) in &mut self.scripts {
            if !worker.alive() {
                self.runtime.script_fault(
                    id,
                    "Script helper terminated; its outputs and timers were released.",
                );
                obsolete.push(id.clone());
                continue;
            }
            if self.runtime.engine().is_none_or(|e| !e.script_enabled(id)) {
                obsolete.push(id.clone());
                continue;
            }
            if worker.pending().is_some_and(|ticket| {
                self.runtime
                    .engine()
                    .is_none_or(|e| !e.script_ticket_current(id, ticket))
            }) {
                obsolete.push(id.clone());
                continue;
            }
            if let Some((ticket, result)) = worker.poll() {
                match result {
                    Ok(transaction) => {
                        if self.runtime.script_reply(id, &ticket, transaction).is_err() {
                            obsolete.push(id.clone());
                        }
                    }
                    Err(error) => {
                        self.runtime.script_fault(id, &error);
                        obsolete.push(id.clone());
                    }
                }
            }
        }
        for id in obsolete {
            self.scripts.remove(&id);
        }
        for (id, request) in self.runtime.script_requests() {
            let result = self
                .scripts
                .get_mut(&id)
                .ok_or_else(|| "Script helper unavailable.".to_string())
                .and_then(|worker| worker.submit(request));
            if let Err(error) = result {
                self.runtime.script_fault(&id, &error);
                self.scripts.remove(&id);
            }
        }
    }
    pub fn live(&self) -> bool {
        self.output.as_ref().is_some_and(|o| o.active())
    }
    pub fn start_live(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
        bindings: Vec<osg_virtual_controller::vjoy::Binding>,
        factory: osg_virtual_controller::session::Factory,
    ) -> Result<RunToken, String> {
        self.start_live_with_journal(profile, assignments, snapshot, bindings, factory, None)
    }
    pub fn start_live_with_journal(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
        bindings: Vec<osg_virtual_controller::vjoy::Binding>,
        factory: osg_virtual_controller::session::Factory,
        path: Option<std::path::PathBuf>,
    ) -> Result<RunToken, String> {
        self.start_live_prepared(
            profile,
            assignments,
            snapshot,
            bindings,
            factory,
            path,
            BTreeMap::new(),
            None,
        )
    }
    pub fn start_live_prepared(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
        bindings: Vec<osg_virtual_controller::vjoy::Binding>,
        factory: osg_virtual_controller::session::Factory,
        path: Option<std::path::PathBuf>,
        workers: BTreeMap<String, osg_logic::worker::Worker>,
        panels: Option<crate::panels::Feed>,
    ) -> Result<RunToken, String> {
        self.start_live_sources(
            profile,
            assignments,
            snapshot,
            bindings,
            factory,
            path,
            workers,
            panels,
            None,
        )
    }
    pub(crate) fn start_live_sources(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
        snapshot: stream::Snapshot,
        bindings: Vec<osg_virtual_controller::vjoy::Binding>,
        factory: osg_virtual_controller::session::Factory,
        path: Option<std::path::PathBuf>,
        workers: BTreeMap<String, osg_logic::worker::Worker>,
        panels: Option<crate::panels::Feed>,
        companions: Option<crate::companions::Prepared>,
    ) -> Result<RunToken, String> {
        if self.runtime.token().is_ok()
            || self.output.as_ref().is_some_and(|o| o.recovery_required())
        {
            return Err(
                "Stop the current mapping and resolve recovery before starting live output.".into(),
            );
        }
        if profile.requirements.device_hiding {
            return Err(
                "This run requires device hiding, which is not available in a standalone output session."
                    .into(),
            );
        }
        crate::settings::validate_bindings(&profile, &bindings)?;
        let mut candidate = Router::default();
        let token = candidate.start_with_sources(
            profile,
            assignments,
            snapshot,
            workers,
            panels,
            companions,
        )?;
        let cursor = self.runtime.recording.cursor();
        // Acquire and confirm every target before replacing the stopped workspace.
        let output = osg_virtual_controller::session::Session::start_with_journal(
            factory,
            token.clone(),
            bindings,
            cursor,
            path,
        )?;
        self.adopt_game(candidate, Some(output))
    }
    pub(crate) fn adopt_game(
        &mut self,
        mut candidate: Router,
        output: Option<osg_virtual_controller::session::Session>,
    ) -> Result<RunToken, String> {
        if self.runtime.token().is_ok()
            || self
                .output
                .as_ref()
                .is_some_and(|o| o.active() || o.recovery_required())
        {
            return Err("A newer mapping or pending recovery must be preserved.".into());
        }
        let token = candidate.runtime.token()?;
        if output
            .as_ref()
            .is_some_and(|o| o.token() != &token || o.cursor() != self.runtime.recording.cursor())
        {
            return Err(
                "Prepared output ownership or recording cursor changed before arming.".into(),
            );
        }
        let mut reports = Vec::new();
        let mut after = Counter(0);
        loop {
            let page = candidate.runtime.recording.page(after, 32)?;
            let count = page.records.len();
            after = page.next;
            reports.extend(page.records.into_iter().map(|r| r.report));
            if count < 32 {
                break;
            }
        }
        candidate.runtime.recording = std::mem::take(&mut self.runtime.recording);
        candidate.runtime.recording.record(reports);
        candidate.output = output;
        *self = candidate;
        self.flush_output()?;
        Ok(token)
    }
    pub fn flush_output(&mut self) -> Result<(), String> {
        let Some(output) = &mut self.output else {
            return Ok(());
        };
        let result = if self.runtime.token().is_ok() && output.active() {
            output.poll(&self.runtime.recording)
        } else if output.active() {
            output.stop()
        } else {
            Ok(())
        };
        if let Err(error) = result {
            self.runtime.stop();
            self.scripts.clear();
            self.panels = None;
            self.companions = None;
            self.visibility = None;
            self.fault = Some(error.clone());
            return Err(error);
        }
        Ok(())
    }
    pub fn recover_durable(
        &mut self,
        factory: osg_virtual_controller::session::Factory,
        path: std::path::PathBuf,
    ) -> Result<(), String> {
        if self.runtime.token().is_ok() {
            return Err("Stop mapping before recovery.".into());
        }
        if self
            .output
            .as_ref()
            .is_some_and(|o| o.active() || o.recovery_required())
            || !osg_virtual_controller::output_journal::inspect(&path)?.is_some_and(|r| r.pending())
        {
            return self.recover();
        }
        osg_virtual_controller::session::Session::recover_stored(factory, path)?;
        self.fault = None;
        Ok(())
    }
    pub fn recover(&mut self) -> Result<(), String> {
        if self.runtime.token().is_ok() {
            return Err("Stop mapping before recovery.".into());
        }
        let Some(output) = &mut self.output else {
            return Err("No interrupted live run is recorded in this session.".into());
        };
        output.recover()?;
        self.fault = None;
        Ok(())
    }
    pub fn stop_result(&mut self) -> Result<(), String> {
        self.learner = None;
        self.panels = None;
        self.companions = None;
        self.runtime.stop();
        self.scripts.clear();
        let mut failure = None;
        if let Some(output) = &mut self.output {
            if let Err(error) = output.stop() {
                self.fault = Some(error.clone());
                failure = Some(error);
            }
        }
        if let Some(visibility) = self.visibility.take() {
            if let Err(error) = visibility.stop() {
                failure = Some(failure.map_or(error.clone(), |prior| format!("{prior}; {error}")));
            }
        }
        failure.map_or(Ok(()), Err)
    }
    pub fn stop(&mut self) {
        let _ = self.stop_result();
    }
    pub fn status(&self) -> serde_json::Value {
        let engine = self.runtime.engine();
        json!({"scripts":engine.map(|e|e.script_status()),"scriptLiveQualified":false,"running":engine.is_some_and(|e| e.active()),"ready":engine.is_some_and(|e| e.ready()),"mode":if self.live() { crate::service::BACKEND } else { "recording" },"recoveryRequired":self.output.as_ref().is_some_and(|o| o.recovery_required()),"token":self.runtime.token().ok(),"profileId":engine.map(|e| &e.profile().id),"fault":self.fault.as_deref().or_else(|| engine.and_then(|e| e.fault())),"recordingCursor":self.runtime.recording.cursor()})
    }
}
#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;

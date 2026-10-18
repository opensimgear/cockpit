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
}
impl HidCapture for osg_controller_input::ControllerService {
    fn snapshot(&self, ids: &[String]) -> Result<stream::Snapshot, String> {
        self.stream_snapshot(ids)
    }
    fn page(&self, session: &str, cursor: u64) -> stream::Page {
        self.stream_page(session, cursor)
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
}
impl HidInputs {
    pub(crate) fn capture(
        plan: &ExecutionPlan,
        controllers: &impl HidCapture,
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
        };
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
mod tests {
    use super::*;
    use osg_controller_input::registry::Kind;
    use osg_profiles::resources::Access;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    const PHYSICAL: &str = "11111111-1111-4111-8111-111111111111";
    struct Capture {
        sources: RefCell<Vec<stream::SourceSnapshot>>,
        pages: RefCell<VecDeque<stream::Page>>,
        fail: Cell<bool>,
        ids: RefCell<Vec<String>>,
    }
    impl HidCapture for Capture {
        fn snapshot(&self, ids: &[String]) -> Result<stream::Snapshot, String> {
            *self.ids.borrow_mut() = ids.to_vec();
            if self.fail.get() {
                return Err("Native capture unavailable.".into());
            }
            Ok(stream::Snapshot {
                session: "native-session".into(),
                cursor: 10,
                captured_us: 100,
                sources: self.sources.borrow().clone(),
            })
        }
        fn page(&self, session: &str, cursor: u64) -> stream::Page {
            assert_eq!(session, "native-session");
            assert!(cursor >= 10);
            self.pages.borrow_mut().pop_front().unwrap_or(stream::Page {
                session: session.into(), cursor, captured_us: 100,
                gap: false, events: vec![],
            })
        }
    }
    fn captured() -> (ExecutionPlan, Capture) {
        use osg_profiles::compiler_models::{BackendCapabilities, CompileSnapshot, SourcePlan};
        let (source, mut event) = fixture();
        event.source = PHYSICAL.into();
        let snapshot = CompileSnapshot {
            inventory_session: "inventory".into(), inventory_revision: "1".into(),
            assignments: vec![], resources: vec![], game_context: None,
            backend: BackendCapabilities::default(), inventory_diagnostics: vec![], read_only: false,
        };
        let mut plan = osg_profiles::compiler::compile(&osg_profiles::default_document(), &snapshot).plan.unwrap();
        let mut token = source.ports[0].token.clone();
        token.route_id = format!("hid:{PHYSICAL}");
        plan.sources.push(SourcePlan {
            endpoint: osg_profiles::Endpoint {node_id: "pedal".into(),port_id:"raw".into()},
            inventory_resource_id: Some("device".into()),
            observed_port_id: Some(source.controls[0].id.clone()),
            contract: osg_profiles::resources_ports::PortContract::State {
                schema: osg_profiles::resources_ports::ScalarSchema::Number {min:-2048.0,max:2047.0,unit:None},
                freshness_ms: 500, fallback: None,
            },
            token: Some(token), preparation: None,
            route: Some(osg_profiles::resources::Route {
                id: format!("hid:{PHYSICAL}"), transport: Transport::Hid,
                provider_generation: "7".into(), attributes: BTreeMap::new(),
                baseline_ready:true,writable:false,priority:1,
            }), readiness: PlanReadiness::Ready,
        });
        (plan, Capture {
            sources: RefCell::new(vec![stream::SourceSnapshot {id:PHYSICAL.into(),controls:source.controls,baseline:event}]),
            pages:RefCell::new(VecDeque::new()),fail:Cell::new(false),ids:RefCell::new(vec![]),
        })
    }
    fn page(event: Option<stream::Event>, gap: bool) -> stream::Page {
        stream::Page {session:"native-session".into(),cursor:11,captured_us:110,gap,events:event.into_iter().collect()}
    }
    #[test]
    fn capture_groups_exact_physical_controls_and_keeps_logical_source_ownership_separate() {
        let (mut plan, capture) = captured();
        let mut second = plan.sources[0].clone();
        second.endpoint.port_id = "second-axis".into();
        plan.sources.push(second.clone());
        second.endpoint.node_id = "independent-route".into();
        plan.sources.push(second);
        let mut dormant = plan.sources[0].clone();
        dormant.readiness = PlanReadiness::Dormant;
        dormant.token = None;
        plan.sources.push(dormant);
        let (mut inputs, frames) = HidInputs::capture(&plan, &capture, |_| Ok(())).unwrap();
        assert_eq!(*capture.ids.borrow(), vec![PHYSICAL]);
        assert_eq!(inputs.sources.len(), 2);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].changes.len(), 2);
        assert!(frames.iter().all(|frame| frame.reason == Reason::Baseline && frame.sequence.0 == 1));
        let mut event = capture.sources.borrow()[0].baseline.clone();
        event.reason = stream::Reason::Change;
        event.sequence += 1;
        event.captured_us += 1;
        capture.pages.borrow_mut().push_back(page(Some(event),false));
        let (frames, errors) = inputs.poll(&capture, |_| Ok(())).unwrap();
        assert!(errors.is_empty());
        assert_eq!(frames.len(),2);
        assert!(frames.iter().all(|frame| frame.reason == Reason::Change && frame.sequence.0 == 2));
    }
    #[test]
    fn capture_requires_exact_native_identity_token_and_complete_current_controls() {
        for change in 0..7 {
            let (mut plan, capture) = captured();
            match change {
                0 => plan.sources[0].route.as_mut().unwrap().id = "hid:guessed".into(),
                1 => plan.sources[0].token = None,
                2 => plan.sources[0].observed_port_id = None,
                3 => capture.sources.borrow_mut().clear(),
                4 => capture.sources.borrow_mut()[0].baseline.values.clear(),
                5 => capture.fail.set(true),
                _ => {
                    let mut second = plan.sources[0].clone();
                    second.route.as_mut().unwrap().id = "hid:22222222-2222-4222-8222-222222222222".into();
                    plan.sources.push(second);
                }
            }
            assert!(HidInputs::capture(&plan,&capture,|_|Ok(())).is_err(),"capture case {change}");
        }
        let (plan, capture) = captured();
        assert!(HidInputs::capture(&plan,&capture,|_|Err("revoked".into())).is_err());
        let mut no_hid = plan.clone();
        no_hid.sources[0].route.as_mut().unwrap().transport = Transport::Usb;
        let (mut inputs, frames) = HidInputs::capture(&no_hid,&capture,|_|Err("unused".into())).unwrap();
        assert!(frames.is_empty());
        assert!(inputs.poll(&capture,|_|Err("unused".into())).unwrap().0.is_empty());
    }
    #[test]
    fn native_history_gap_discards_transitions_and_recaptures_only_a_complete_baseline() {
        let (plan, capture) = captured();
        let (mut inputs, _) = HidInputs::capture(&plan,&capture,|_|Ok(())).unwrap();
        capture.sources.borrow_mut()[0].baseline.captured_us=110;
        capture.pages.borrow_mut().push_back(page(None,true));
        let (frames, errors) = inputs.poll(&capture,|_|Ok(())).unwrap();
        assert!(errors.is_empty());
        assert_eq!(frames.iter().map(|frame|frame.reason.clone()).collect::<Vec<_>>(),vec![Reason::Gap,Reason::Baseline]);
        assert_eq!(frames[1].sequence.0,3);
        capture.fail.set(true);
        capture.pages.borrow_mut().push_back(page(None,true));
        let (frames, errors) = inputs.poll(&capture,|_|Ok(())).unwrap();
        assert_eq!(frames.len(),1);
        assert!(!errors.is_empty());
        assert!(!inputs.sources[0].ready);
        capture.fail.set(false);
        let (frames, errors) = inputs.poll(&capture,|_|Ok(())).unwrap();
        assert!(errors.is_empty());
        assert_eq!(frames[0].reason,Reason::Baseline);
        assert!(inputs.sources[0].ready);
    }
    #[test]
    fn lost_report_or_route_replacement_withdraws_without_replaying_or_faulting_other_sources() {
        let (plan,capture) = captured();
        let (mut inputs,_) = HidInputs::capture(&plan,&capture,|_|Ok(())).unwrap();
        let mut event = capture.sources.borrow()[0].baseline.clone();
        event.reason=stream::Reason::Change;
        event.sequence += 2;
        event.captured_us=110;
        let mut baseline=event.clone();
        baseline.reason=stream::Reason::Baseline;
        capture.sources.borrow_mut()[0].baseline=baseline;
        capture.pages.borrow_mut().push_back(page(Some(event),false));
        let (frames,errors)=inputs.poll(&capture,|_|Ok(())).unwrap();
        assert!(errors.is_empty());
        assert_eq!(frames[0].reason,Reason::Gap);
        assert_eq!(frames[1].reason,Reason::Baseline);
        capture.pages.borrow_mut().push_back(page(None,false));
        let (frames,errors)=inputs.poll(&capture,|_|Err("route revoked".into())).unwrap();
        assert_eq!(frames[0].reason,Reason::Gap);
        assert!(!errors.is_empty());
        assert!(!inputs.sources[0].ready);
        let mut replacement=capture.sources.borrow()[0].baseline.clone();
        replacement.reason=stream::Reason::Baseline;
        replacement.generation+=1;
        replacement.captured_us+=1;
        capture.sources.borrow_mut()[0].baseline=replacement.clone();
        capture.pages.borrow_mut().push_back(page(Some(replacement),false));
        let (frames,errors)=inputs.poll(&capture,|_|Ok(())).unwrap();
        assert_eq!(frames[0].reason,Reason::Gap);
        assert!(!errors.is_empty());
        assert!(!inputs.sources[0].ready);
    }
    #[test]
    fn failed_multisource_baseline_does_not_partially_arm_a_reader() {
        let (mut plan,capture)=captured();
        let mut second=plan.sources[0].clone();
        second.endpoint.node_id="second".into();
        plan.sources.push(second);
        let (mut inputs,_)=HidInputs::capture(&plan,&capture,|_|Ok(())).unwrap();
        for source in &mut inputs.sources {source.ready=false;}
        inputs.sources[1].ports[0].observed="missing".into();
        assert!(inputs.baseline(&capture,&|_|Ok(())).is_err());
        assert!(inputs.sources.iter().all(|source|!source.ready && source.sequence==1));
        inputs.sources[1].ports[0].observed=capture.sources.borrow()[0].controls[0].id.clone();
        capture.sources.borrow_mut()[0].baseline.captured_us=99;
        assert!(inputs.baseline(&capture,&|_|Ok(())).is_err());
        let (mut source, event)=fixture();
        source.sequence=u64::MAX;
        assert!(source.frame(&event,true).is_err());
        assert!(source.loss(100,Reason::Gap).is_err());
    }
    fn fixture() -> (Source, stream::Event) {
        let mut control = Control::new(1, 0x30, 0, 0, 0);
        control.kind = Kind::Axis;
        control.bits = 16;
        control.min = -2048;
        control.max = 2047;
        let token = ResourceToken {
            inventory_session: "inventory".into(),
            resource_id: "device".into(),
            generation: "1".into(),
            capability_revision: "1".into(),
            route_id: "hid:physical".into(),
            provider_session: "capture".into(),
            provider_generation: "7".into(),
            port_id: control.id.clone(),
            contract_hash: "hash".into(),
            access: Access::Read,
        };
        let source = Source {
            logical: "pedal".into(),
            physical: "physical".into(),
            ports: vec![Port {
                logical: "raw".into(),
                observed: control.id.clone(),
                token,
            }],
            controls: vec![control.clone()],
            generation: 7,
            provider_sequence: 0,
            sequence: 0,
            at: 0,
            ready: false,
        };
        let event = stream::Event {
            cursor: 10,
            source: "physical".into(),
            generation: 7,
            sequence: 42,
            captured_us: 100,
            reason: stream::Reason::Baseline,
            values: BTreeMap::from([(control.id.clone(), control.decode(0xffff).unwrap())]),
        };
        (source, event)
    }
    #[test]
    fn exact_native_axis_baseline_and_changes_keep_the_raw_domain() {
        let (mut source, mut event) = fixture();
        let frame = source.frame(&event, true).unwrap();
        assert_eq!(frame.reason, Reason::Baseline);
        assert_eq!(frame.source, "pedal");
        assert_eq!(frame.sequence.0, 1);
        assert_eq!(
            frame.changes[0].sample,
            SignalSample::scalar(osg_profiles::PortValue::Number(-1.0))
        );
        event.reason = stream::Reason::Change;
        event.sequence += 1;
        event.captured_us += 1;
        event.values.clear();
        let unchanged = source.frame(&event, false).unwrap();
        assert!(unchanged.changes.is_empty());
        assert_eq!(source.provider_sequence, 43);
        assert_eq!(unchanged.sequence.0, 2);
    }
    #[test]
    fn gap_withdraws_values_and_a_current_snapshot_starts_a_new_admission_sequence() {
        let (mut source, event) = fixture();
        source.frame(&event, true).unwrap();
        let gap = source.loss(120, Reason::Gap).unwrap();
        assert!(gap.changes.is_empty());
        assert!(!source.ready);
        assert_eq!(gap.sequence.0, 2);
        let mut fresh = event.clone();
        fresh.captured_us = 125;
        let baseline = source.frame(&fresh, true).unwrap();
        assert_eq!(baseline.reason, Reason::Baseline);
        assert_eq!(baseline.sequence.0, 3);
        assert_eq!(source.provider_sequence, 42);
        assert!(source.ready);
    }
    #[test]
    fn missing_or_changed_controls_and_stale_route_generations_cannot_become_samples() {
        let (mut source, mut event) = fixture();
        event.values.clear();
        assert!(source.frame(&event, true).is_err());
        let (mut source, event) = fixture();
        source.controls.clear();
        assert!(source.frame(&event, true).is_err());
        let (mut source, mut event) = fixture();
        event.generation += 1;
        assert!(source.frame(&event, true).is_err());
        let (mut source, mut event) = fixture();
        event.reason = stream::Reason::Disconnect;
        assert_eq!(
            source.frame(&event, false).unwrap().reason,
            Reason::Disconnect
        );
        assert!(!source.ready);
    }
}

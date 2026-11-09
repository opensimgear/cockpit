use super::*;
use osg_controller_input::registry::Kind;
use osg_profiles::resources::Access;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
#[path = "inputs_watch_tests.rs"]
mod watches;

const PHYSICAL: &str = "11111111-1111-4111-8111-111111111111";
struct Capture {
    sources: RefCell<Vec<stream::SourceSnapshot>>,
    pages: RefCell<VecDeque<stream::Page>>,
    fail: Cell<bool>,
    ids: RefCell<Vec<String>>,
}
impl HidCapture for Capture {
    fn watch_reports(&self, _: &[String], _: std::sync::Arc<dyn Fn() + Send + Sync>)
        -> Result<Option<stream::ReportWatch>, String> { Ok(None) }
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
    let (mut inputs, frames) = HidInputs::capture(&plan, &capture,None, |_| Ok(())).unwrap();
    assert_eq!(*capture.ids.borrow(), vec![PHYSICAL]);
    assert_eq!(inputs.sources.len(), 2);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].changes.len(), 2);
    assert!(frames.iter().all(|frame| frame.reason == Reason::Baseline && frame.sequence.0 == 1));
    let mut event = capture.sources.borrow()[0].baseline.clone();
    event.reason = stream::Reason::Change;
    event.sequence += 1;
    event.captured_us += 1;
    capture.pages.borrow_mut().push_back(page(Some(event.clone()),false));
    let (frames, errors) = inputs.poll(&capture, |_| Ok(())).unwrap();
    assert!(frames.is_empty());
    assert!(errors.is_empty());
    event.sequence += 1;
    event.captured_us += 1;
    let control = &inputs.sources[0].controls[0];
    event.values.insert(control.id.clone(), control.decode(12).unwrap());
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
        assert!(HidInputs::capture(&plan,&capture,None,|_|Ok(())).is_err(),"capture case {change}");
    }
    let (plan, capture) = captured();
    assert!(HidInputs::capture(&plan,&capture,None,|_|Err("revoked".into())).is_err());
    let mut no_hid = plan.clone();
    no_hid.sources[0].route.as_mut().unwrap().transport = Transport::Usb;
    let (mut inputs, frames) = HidInputs::capture(&no_hid,&capture,None,|_|Err("unused".into())).unwrap();
    assert!(frames.is_empty());
    assert!(inputs.poll(&capture,|_|Err("unused".into())).unwrap().0.is_empty());
}
#[test]
fn native_history_gap_discards_transitions_and_recaptures_only_a_complete_baseline() {
    let (plan, capture) = captured();
    let (mut inputs, _) = HidInputs::capture(&plan,&capture,None,|_|Ok(())).unwrap();
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
    let (mut inputs,_) = HidInputs::capture(&plan,&capture,None,|_|Ok(())).unwrap();
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
    let (mut inputs,_)=HidInputs::capture(&plan,&capture,None,|_|Ok(())).unwrap();
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
        levels: BTreeMap::new(),
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
        native_read_us: None,
        reason: stream::Reason::Baseline,
        values: BTreeMap::from([(control.id.clone(), control.decode(0xffff).unwrap())]),
    };
    (source, event)
}
#[test]
fn exact_native_axis_baseline_and_changes_keep_the_raw_domain() {
    let (mut source, mut event) = fixture();
    let frame = source.frame(&event, true).unwrap().unwrap();
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
    assert!(source.frame(&event, false).unwrap().is_none());
    assert_eq!(source.provider_sequence, 43);
    assert_eq!(source.sequence, 1);
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
    let baseline = source.frame(&fresh, true).unwrap().unwrap();
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
        source.frame(&event, false).unwrap().unwrap().reason,
        Reason::Disconnect
    );
    assert!(!source.ready);
}

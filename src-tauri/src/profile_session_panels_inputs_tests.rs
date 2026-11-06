use super::*;
use osg_profiles::{
    compiler_models::{BackendCapabilities, CompileSnapshot, SourcePlan},
    resources::*,
    resources_ports::*,
};
use std::sync::Condvar;

#[test]
fn poisoned_panel_mailbox_still_wakes_subscribers_and_revokes_on_stop() {
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io).unwrap();
    assert!(!inputs.is_empty());
    let mailbox = inputs.mailbox.clone();
    assert!(thread::spawn(move || {
        let _guard = mailbox.lock().unwrap();
        panic!("Simulate an interrupted panel mailbox update");
    }).join().is_err());
    assert!(inputs.mailbox.is_poisoned());

    let baseline = Arc::new(NativeWake::default());
    let runtime = Arc::new(NativeWake::default());
    inputs.watch_baseline(&baseline);
    inputs.set_wake(Some(runtime.clone()));
    let baseline_before = baseline.observed();
    let runtime_before = runtime.observed();

    inputs.acquire(0).unwrap();
    assert!(inputs.acquire(0).is_err(), "A reader cannot acquire a second clock origin");
    let frames = inputs.wait_baseline(&plan, Duration::from_secs(2), |_| Ok(()), || false).unwrap();
    assert!(!frames.is_empty());
    assert!(baseline.observed() > baseline_before);
    assert!(runtime.observed() > runtime_before);
    inputs.start().unwrap();
    assert!(inputs.active.load(Ordering::Acquire));
    assert!(inputs.start().is_err(), "An active reader cannot be admitted twice");
    inputs.stop();
    assert!(inputs.revoked.load(Ordering::Acquire));
    assert!(!inputs.active.load(Ordering::Acquire));
    assert!(!inputs.admitted.load(Ordering::Acquire));
}

#[test]
fn capture_can_commit_loss_while_host_validates_the_route() {
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io).unwrap();
    inputs.bound = true;
    let mailbox = inputs.mailbox.clone();
    let logical = inputs.specs[0].logical.clone();
    inputs.tokens.insert(logical.clone(), plan.sources.iter()
        .filter(|source| source.endpoint.node_id == logical)
        .filter_map(|source| source.token.clone())
        .collect());
    let lost = std::cell::Cell::new(false);

    let result = inputs.poll(|_| {
        if !lost.replace(true) {
            mailbox.try_lock().expect("Capture cannot wait behind route validation")
                .loss(&logical, "Capture lost continuity during route validation.").unwrap();
        }
        Ok(())
    }).unwrap();

    assert!(lost.get());
    assert_eq!(result.frames.len(), 1);
    assert_eq!(result.frames[0].source, logical);
    assert_eq!(result.frames[0].reason, Reason::Gap);
    assert_eq!(result.diagnostics, ["Capture lost continuity during route validation."]);
}

#[test]
fn native_baseline_keeps_raw_axis_and_event_occurrences_unknown_until_actual_sample_advances() {
    for can in [true] {
        let (plan, io) = fixture(can, 1);
        let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
        let mut source = make_source(inputs.specs[0].clone());
        let first = source.baseline(io.as_ref(), 10).unwrap();
        assert_eq!(sample(&first, "axis:0"), Some(&SignalSample::Unknown));
        assert_eq!(
            sample(&first, "button-event:0"),
            Some(&SignalSample::Unknown)
        );
        io.data.lock().unwrap()["sampledAtMs"] = json!(11);
        let next = source.baseline(io.as_ref(), 20).unwrap();
        assert_eq!(
            sample(&next, "axis:0"),
            Some(&SignalSample::scalar(PortValue::Number(65535.0)))
        );
        assert!(
            source.page(io.as_ref(), 30).unwrap().is_empty(),
            "frozen sample cannot refresh state freshness"
        );
        io.data.lock().unwrap()["axisAvailable"][0] = json!(false);
        let loss = source.page(io.as_ref(), 40).unwrap();
        assert_eq!(sample(&loss[0], "axis:0"), Some(&SignalSample::Unavailable));
    }
}
#[test]
fn short_press_journal_is_ordered_without_replay_or_axis_normalization() {
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    source.baseline(io.as_ref(), 10).unwrap();
    io.press(true);
    io.press(false);
    let frames = source.page(io.as_ref(), 20).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(
        sample(&frames[0], "button-event:0"),
        Some(&SignalSample::scalar(PortValue::Boolean(true)))
    );
    assert_eq!(
        sample(&frames[1], "button-event:0"),
        Some(&SignalSample::scalar(PortValue::Boolean(false)))
    );
    assert_eq!(frames[0].captured_us, frames[1].captured_us);
    assert!(frames[0].sequence.0 < frames[1].sequence.0);
    assert!(source.page(io.as_ref(), 30).unwrap().is_empty());
    io.press(true);
    let _ = source.page(io.as_ref(), 40).unwrap();
    io.data.lock().unwrap()["sampledAtMs"] = json!(50);
    let held = source.page(io.as_ref(), 50).unwrap();
    assert_eq!(
        sample(&held[0], "button:0"),
        Some(&SignalSample::scalar(PortValue::Boolean(true)))
    );
    assert_eq!(sample(&held[0], "button-event:0"), None);
}
#[test]
fn partial_pages_do_not_jump_state_ahead_and_corrupt_journals_admit_no_partial_frame() {
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    source.baseline(io.as_ref(), 1).unwrap();
    for n in 0..19 {
        io.press(n % 2 == 0);
    }
    let page = source.page(io.as_ref(), 2).unwrap();
    assert_eq!(page.len(), 16);
    assert!(page.iter().all(|frame| sample(frame, "axis:0").is_none()));
    assert_eq!(source.cursor, Some((7, 16)));
    let tail = source.page(io.as_ref(), 3).unwrap();
    assert_eq!(tail.len(), 4);
    io.press(false);
    io.data.lock().unwrap()["held"][0] = json!(true);
    let sequence = source.sequence;
    assert!(source.page(io.as_ref(), 4).is_err());
    assert_eq!(source.sequence, sequence);
    assert_eq!(source.cursor, Some((7, 19)));
    io.data.lock().unwrap()["oldest"] = json!(21);
    assert!(source.page(io.as_ref(), 5).is_err());
}
#[test]
fn sample_clock_wrap_is_accepted_but_backwards_clock_contract_and_identity_changes_are_rejected() {
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    io.data.lock().unwrap()["sampledAtMs"] = json!(u32::MAX);
    source.baseline(io.as_ref(), 1).unwrap();
    io.data.lock().unwrap()["sampledAtMs"] = json!(0);
    assert!(source.baseline(io.as_ref(), 2).is_ok());
    io.data.lock().unwrap()["sampledAtMs"] = json!(u32::MAX);
    assert!(source.baseline(io.as_ref(), 3).is_err());
    assert_eq!(
        source.sampled_at,
        Some(0),
        "a rejected old clock cannot move the sample fence"
    );
    io.data.lock().unwrap()["sampledAtMs"] = json!(1);
    io.data.lock().unwrap()["axisNeutral"][0] = json!(0);
    assert!(source.baseline(io.as_ref(), 4).is_err());
    io.routes
        .lock()
        .unwrap()
        .get_mut("panel0")
        .unwrap()
        .provider_generation = json!([1, 2, 3, 5, 7]).to_string();
    assert!(Target::from_route(plan.sources[0].route.as_ref().unwrap(), io.as_ref()).is_err());
    let (plan, io) = fixture(false, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    io.usb.lock().unwrap().connection_generation = 2;
    assert!(source.baseline(io.as_ref(), 1).is_err());
    assert!(Target::from_route(plan.sources[0].route.as_ref().unwrap(), io.as_ref()).is_err());
}
#[test]
fn actual_panel_frames_feed_typed_reducer_and_baseline_gap_stop_cannot_replay_occurrences() {
    use osg_mapping::{
        engine::typed::Phase,
        program::{Endpoint, Program, SourcePort},
    };
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    let program = Program {
        sources: plan
            .sources
            .iter()
            .map(|s| SourcePort {
                endpoint: Endpoint {
                    node_id: s.endpoint.node_id.clone(),
                    port_id: s.endpoint.port_id.clone(),
                },
                contract: s.contract.clone(),
            })
            .collect(),
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    let mut phase = Phase::new(
        program,
        Counter(1),
        "11111111-1111-4111-8111-111111111111".into(),
    )
    .unwrap();
    let event = Endpoint {
        node_id: "logical0".into(),
        port_id: "button-event:0".into(),
    };
    crate::profile_admission_test_clock::frame(&mut phase, source.baseline(io.as_ref(), 10).unwrap())
        .unwrap();
    assert_eq!(phase.sample(&event), Some(&SignalSample::Unknown));
    io.press(true);
    io.press(false);
    let frames = source.page(io.as_ref(), 20).unwrap();
    crate::profile_admission_test_clock::frame(&mut phase, frames[0].clone()).unwrap();
    assert_eq!(
        phase.sample(&event),
        Some(&SignalSample::scalar(PortValue::Boolean(true)))
    );
    crate::profile_admission_test_clock::frame(&mut phase, frames[1].clone()).unwrap();
    assert_eq!(
        phase.sample(&event),
        Some(&SignalSample::scalar(PortValue::Boolean(false)))
    );
    crate::profile_admission_test_clock::frame(&mut phase, source.frame(30, Reason::Gap, vec![]).unwrap())
        .unwrap();
    crate::profile_admission_test_clock::frame(&mut phase, source.baseline(io.as_ref(), 40).unwrap())
        .unwrap();
    assert_eq!(phase.sample(&event), Some(&SignalSample::Unknown));
    phase.stop();
    io.press(true);
    for frame in source.page(io.as_ref(), 50).unwrap() {
        assert!(crate::profile_admission_test_clock::frame(&mut phase, frame).is_err());
    }
    assert!(!phase.active());
    assert!(phase.drain().is_empty());
}
#[test]
fn invalid_native_generations_and_acquisition_budgets_fail_before_any_device_request() {
    let (plan, io) = fixture(true, 1);
    let initial = io.calls.load(Ordering::Acquire);
    for generation in ["bad", "[]", "[1,0,3,4,7]", "[1,2,3,4,8]"] {
        let mut route = plan.sources[0].route.clone().unwrap();
        route.provider_generation = generation.into();
        assert!(Target::from_route(&route, io.as_ref()).is_err());
    }
    let mut wrong = plan.sources[0].route.clone().unwrap();
    wrong.attributes.remove("moduleId");
    assert!(Target::from_route(&wrong, io.as_ref()).is_err());
    wrong = plan.sources[0].route.clone().unwrap();
    wrong.attributes.insert("node".into(), "bad".into());
    assert!(Target::from_route(&wrong, io.as_ref()).is_err());
    assert_eq!(io.calls.load(Ordering::Acquire), initial);
    let (oversized, large) = fixture(true, 17);
    assert!(PanelInputs::prepare_io(&oversized, large).is_err());
    let mut dormant = plan.clone();
    dormant.sources[0].readiness = PlanReadiness::Dormant;
    dormant.sources[0].preparation = None;
    assert!(PanelInputs::prepare_io(&dormant, io.clone()).is_ok());
    let mut route = native_route(false, "missing");
    assert!(Target::from_route(&route, io.as_ref()).is_err());
    route.transport = Transport::Hid;
    assert!(Target::from_route(&route, io.as_ref()).is_err());
}
#[test]
fn source_errors_rebaseline_without_reconstructing_missing_transitions() {
    use crate::stm32_panel_wire::{Edge, Packet, Payload, Record};
    let (plan, io) = fixture_with_stream(true, 1, true);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    let send = |sequence, cursor, sampled_at_ms, payload| io.capture.accept(Record::Event {
        module_id: "0123456789abcdef0123456789abcdef".into(),
        source_node: 17,
        packet: Packet { node: 5, boot_session: 2, lease: 3, gateway_epoch: 1,
            configuration_generation: 4, sequence, cursor, sampled_at_ms, payload },
    }, Instant::now());
    send(1, 0, 10, Payload::Layout { contact_count: 1, encoder_count: 0, selector_count: 0, maintained: [0;16] });
    let state = || Payload::State { held: [0;16], axes: [0;8], axis_available: 1 };
    send(2, 0, 11, state());
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.start().unwrap();
    eventually(|| {
        inputs
            .poll(|_| Ok(()))
            .unwrap()
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline)
            .then_some(())
    });
    send(3, 2, 12, Payload::Edges(vec![
        Edge { control: 0, pressed: true, captured_at_ms: 12 },
        Edge { control: 0, pressed: false, captured_at_ms: 12 },
    ]));
    let pressed = eventually(|| {
        let frames = inputs.poll(|_| Ok(())).unwrap().frames;
        (!frames.is_empty()).then_some(frames)
    });
    assert_eq!(pressed.iter().filter_map(|frame| sample(frame, "button-event:0")).cloned().collect::<Vec<_>>(),
        [SignalSample::scalar(PortValue::Boolean(true)), SignalSample::scalar(PortValue::Boolean(false))]);
    io.capture.accept(Record::Reset, Instant::now());
    let error = eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Gap)
            .then_some(result)
    });
    assert!(error.diagnostics.iter().all(|s| !s.contains('\n')));
    send(4, 2, 13, Payload::Layout { contact_count: 1, encoder_count: 0, selector_count: 0, maintained: [0;16] });
    send(5, 2, 14, state());
    let frames = eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline)
            .then_some(result.frames)
    });
    assert!(frames
        .iter()
        .flat_map(|f| &f.changes)
        .filter(|c| c.port_id == "button-event:0")
        .all(|c| c.sample == SignalSample::Unknown));
    inputs.stop();
    assert_eq!(io.calls.load(Ordering::Acquire), 0);
}

#[test]
fn native_usb_companion_routes_belong_to_the_companion_adapter() {
    let (mut plan, io) = fixture(false, 1);
    for source in &mut plan.sources {
        source.route.as_mut().unwrap().id = "companion:standalone-panel".into();
        source.preparation = None;
    }
    let inputs = PanelInputs::prepare_io(&plan, io).unwrap();
    assert!(inputs.specs.is_empty());
}

include!("profile_session_panels_inputs/test_support.rs");
#[path = "profile_session_panels_inputs/can_tests.rs"]
mod can_tests;
#[path = "profile_session_panels_inputs/usb_tests.rs"]
mod usb_tests;

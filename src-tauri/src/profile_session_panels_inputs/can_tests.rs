use super::*;

#[test]
fn activation_requires_a_new_live_read_and_consumes_its_baseline_exactly_once() {
    for can in [true] {
        let (plan, io) = fixture(can, 1);
        io.automatic_clock.store(true, Ordering::Release);
        let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
        inputs.acquire(0).unwrap();
        let original = eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());

        let inactive = Arc::new(Block::new());
        *io.block.lock().unwrap() = Some(("panel0".into(), inactive.clone()));
        inactive.entered();
        // These edges predate activation and must never become occurrences.
        io.press(true);
        io.press(false);
        inputs.begin_live_baseline().unwrap();
        assert!(inputs.live_baseline(&plan, |_| Ok(())).unwrap().is_none());

        let live = Arc::new(Block::new());
        *io.block.lock().unwrap() = Some(("panel0".into(), live.clone()));
        inactive.release();
        live.entered();
        assert!(
            inputs.live_baseline(&plan, |_| Ok(())).unwrap().is_none(),
            "A late inactive reply cannot acknowledge live activation"
        );
        assert!(inputs.mailbox.lock().unwrap().frames.is_empty());

        live.release();
        let baseline = eventually(|| inputs.live_baseline(&plan, |_| Ok(())).unwrap());
        assert_eq!(baseline.len(), 1);
        assert_eq!(baseline[0].reason, Reason::Baseline);
        assert!(baseline[0].sequence > original[0].sequence);
        assert_eq!(
            sample(&baseline[0], "button-event:0"),
            Some(&SignalSample::Unknown)
        );
        let pending = inputs.poll(|_| Ok(())).unwrap();
        assert!(
            pending
                .frames
                .iter()
                .all(|frame| frame.reason != Reason::Baseline),
            "The baseline already admitted by the Host cannot reset its live reducer"
        );
        io.press(true);
        let changes = eventually(|| {
            let poll = inputs.poll(|_| Ok(())).unwrap();
            poll.frames
                .iter()
                .any(|frame| {
                    sample(frame, "button-event:0")
                        == Some(&SignalSample::scalar(PortValue::Boolean(true)))
                })
                .then_some(poll.frames)
        });
        assert!(changes
            .iter()
            .all(|frame| frame.sequence > baseline[0].sequence));
        inputs.stop();
    }
}
#[test]
fn verified_panel_reads_keep_latched_buttons_fresh_for_five_seconds_without_inventing_edges() {
    use osg_mapping::{
        engine::typed::Phase,
        program::{Program, SourcePort},
    };
    let (plan, io) = fixture(true, 1);
    io.press(true);
    let sampled_at = io.data.lock().unwrap()["sampledAtMs"].clone();
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.begin_live_baseline().unwrap();
    let baseline = eventually(|| inputs.live_baseline(&plan, |_| Ok(())).unwrap());
    let program = Program {
        sources: plan.sources.iter().map(|source| SourcePort {
            endpoint: osg_mapping::program::Endpoint {
                node_id: source.endpoint.node_id.clone(),
                port_id: source.endpoint.port_id.clone(),
            },
            contract: source.contract.clone(),
        }).collect(),
        nodes: vec![], sinks: vec![], controllers: vec![],
    };
    let mut phase = Phase::new(program, Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    for frame in baseline { crate::profile_admission_test_clock::frame(&mut phase, frame).unwrap(); }
    phase.drain();
    let button = osg_mapping::program::Endpoint {
        node_id: "logical0".into(), port_id: "button:0".into(),
    };
    let mut first_verified = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "Verified read clock did not advance across five seconds");
        // Await an actual verified read rather than requiring a polling rate
        // from a shared CI runner. Frozen sampledAt and held state must produce
        // liveness markers only, without synthetic state/event frames.
        let (poll, verified) = eventually(|| {
            let poll = inputs.poll(|_| Ok(())).unwrap();
            assert!(poll.frames.is_empty(), "Stationary panel control must not manufacture edges");
            if poll.liveness.is_empty() { return None; }
            let verified = inputs.mailbox.lock().unwrap().liveness["logical0"].2;
            Some((poll, verified))
        });
        let (epoch, origin) = inputs.clock.unwrap();
        let now = Counter(origin + epoch.elapsed().as_micros() as u64);
        for (source, generation, ports) in poll.liveness {
            assert_eq!(ports, vec!["button:0"]);
            phase.observe_port_liveness(&source, generation, now, &ports).unwrap();
        }
        phase.advance(now).unwrap();
        assert_eq!(phase.sample(&button), Some(&SignalSample::scalar(PortValue::Boolean(true))));
        let first = *first_verified.get_or_insert(verified);
        if verified.saturating_sub(first) > 5_000_000 { break; }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(io.data.lock().unwrap()["sampledAtMs"], sampled_at);
    io.fail.store(true, Ordering::Release);
    let loss = eventually(|| {
        let poll = inputs.poll(|_| Ok(())).unwrap();
        poll.frames.iter().any(|frame| frame.reason == Reason::Gap).then_some(poll)
    });
    assert!(loss.liveness.is_empty());
    crate::profile_admission_test_clock::batch(&mut phase, loss.frames).unwrap();
    assert!(!matches!(phase.sample(&button), Some(SignalSample::Known(_))));
    inputs.stop();
}

#[test]
fn delayed_live_peer_keeps_other_baselines_fresh_and_admits_only_postbaseline_changes() {
    let (plan, io) = fixture(true, 2);
    io.automatic_clock.store(true, Ordering::Release);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    let fast = inputs
        .specs
        .iter()
        .find(|spec| matches!(&spec.target,Target::Can{gateway,..} if gateway=="panel1"))
        .unwrap()
        .logical
        .clone();
    let slow = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel0".into(), slow.clone()));
    inputs.begin_live_baseline().unwrap();
    {
        let mut mailbox = inputs.mailbox.lock().unwrap();
        mailbox.loss_epoch += 1;
        mailbox.baselines.clear();
    }
    slow.entered();
    eventually(|| {
        inputs
            .mailbox
            .lock()
            .unwrap()
            .baselines
            .contains_key(&fast)
            .then_some(())
    });
    thread::sleep(Duration::from_millis(320));
    assert!(inputs.live_baseline(&plan, |_| Ok(())).unwrap().is_none());
    assert!(
        inputs.mailbox.lock().unwrap().frames.is_empty(),
        "Unadmitted snapshots cannot queue event delivery"
    );

    let in_flight = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel1".into(), in_flight.clone()));
    in_flight.entered();
    slow.release();
    let baseline = eventually(|| inputs.live_baseline(&plan, |_| Ok(())).unwrap());
    assert_eq!(baseline.len(), 2);
    let accepted = baseline
        .iter()
        .find(|frame| frame.source == fast)
        .unwrap()
        .clone();
    let first = inputs.poll(|_| Ok(())).unwrap();
    assert!(
        first
            .frames
            .iter()
            .all(|frame| !matches!(frame.reason, Reason::Baseline | Reason::Gap)),
        "The admitted newer generation cannot immediately gap again"
    );
    io.press(true);
    in_flight.release();
    let frames = eventually(|| {
        let poll = inputs.poll(|_| Ok(())).unwrap();
        poll.frames
            .iter()
            .any(|frame| {
                frame.source == fast
                    && sample(frame, "button-event:0")
                        == Some(&SignalSample::scalar(PortValue::Boolean(true)))
            })
            .then_some(poll.frames)
    });
    assert!(
        frames
            .iter()
            .filter(|frame| frame.source == fast)
            .all(|frame| frame.sequence > accepted.sequence
                && frame.generation == accepted.generation)
    );
    inputs.stop();
}

#[test]
fn preparation_and_delivery_require_exact_native_port_scope_and_no_invoke_capability() {
    let (plan, io) = fixture(true, 1);
    let mut wrong = plan.clone();
    wrong.sources[0].preparation = None;
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    wrong = plan.clone();
    wrong.sources[0].route.as_mut().unwrap().provider_generation =
        json!([2, 2, 3, 4, 7]).to_string();
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    wrong = plan.clone();
    wrong.sources[0].contract = PortContract::Event {
        schema: ScalarSchema::Boolean {},
    };
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    wrong = plan.clone();
    wrong.sources.push(wrong.sources[0].clone());
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    assert!(inputs.start().is_err());
    assert!(inputs.poll(|_| Ok(())).is_err());
    assert!(inputs.baseline(&plan, |_| Ok(())).is_err());
    inputs.acquire(100).unwrap();
    assert!(inputs.acquire(100).is_err());
    eventually(|| {
        let ready = inputs.mailbox.lock().unwrap().baselines.len() == 1;
        ready.then_some(())
    });
    for field in ["access", "resource", "port", "hash", "generation"] {
        let mut wrong = plan.clone();
        let token = wrong.sources[0].token.as_mut().unwrap();
        match field {
            "access" => token.access = Access::Invoke,
            "resource" => token.resource_id = "other".into(),
            "port" => token.port_id = "other".into(),
            "hash" => token.contract_hash = "bad".into(),
            _ => token.provider_generation = "old".into(),
        };
        assert!(inputs.baseline(&wrong, |_| Ok(())).is_err(), "{field}");
    }
    assert!(inputs.baseline(&plan, |_| Err("revoked".into())).is_err());
    let frames = inputs.baseline(&plan, |_| Ok(())).unwrap().unwrap();
    assert_eq!(frames.len(), 1);
    inputs.start().unwrap();
    assert!(inputs.baseline(&plan, |_| Ok(())).is_err());
    inputs.stop();
    assert!(inputs.capture_receipts().is_empty());
}
#[test]
fn nonblocking_poll_and_stop_discard_late_device_reply() {
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.start().unwrap();
    let block = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel0".into(), block.clone()));
    block.entered();
    let start = Instant::now();
    let _ = inputs.poll(|_| Ok(())).unwrap();
    assert!(start.elapsed() < Duration::from_millis(50));
    let start = Instant::now();
    inputs.stop();
    assert!(start.elapsed() < Duration::from_millis(50));
    block.release();
    eventually(|| (IN_FLIGHT.load(Ordering::Acquire) == 0).then_some(()));
    assert!(inputs.poll(|_| Ok(())).unwrap().frames.is_empty());
    assert!(inputs.mailbox.lock().unwrap().baselines.is_empty());
}
#[test]
fn one_slow_device_does_not_prevent_other_native_baselines_and_wait_is_cancellable() {
    let (plan, io) = fixture(true, 2);
    let block = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel0".into(), block.clone()));
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    block.entered();
    eventually(|| (inputs.mailbox.lock().unwrap().baselines.len() == 1).then_some(()));
    assert!(inputs.baseline(&plan, |_| Ok(())).unwrap().is_none());
    assert!(inputs
        .wait_baseline(&plan, Duration::from_secs(3), |_| Ok(()), || true)
        .unwrap_err()
        .contains("cancelled"));
    assert!(inputs
        .wait_baseline(&plan, Duration::ZERO, |_| Ok(()), || false)
        .is_err());
    block.release();
    let frames = inputs
        .wait_baseline(&plan, Duration::from_secs(2), |_| Ok(()), || false)
        .unwrap();
    assert_eq!(frames.len(), 2);
    inputs.stop();
}
#[test]
fn start_discards_pre_activation_history_and_revocation_isolated_to_one_source() {
    let (plan, io) = fixture(true, 2);
    io.automatic_clock.store(true, Ordering::Release);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    io.press(true);
    io.press(false);
    inputs.start().unwrap();
    let mut initial = vec![];
    eventually(|| {
        initial.extend(inputs.poll(|_| Ok(())).unwrap().frames);
        (initial
            .iter()
            .filter(|f| f.reason == Reason::Baseline)
            .count()
            >= 2)
            .then_some(())
    });
    assert!(initial
        .iter()
        .flat_map(|f| &f.changes)
        .filter(|c| c.port_id == "button-event:0")
        .all(|c| c.sample == SignalSample::Unknown));
    let revoked = plan.sources[0].token.as_ref().unwrap().resource_id.clone();
    let result = inputs
        .poll(|token| {
            if token.resource_id == revoked {
                Err("removed".into())
            } else {
                Ok(())
            }
        })
        .unwrap();
    assert_eq!(
        result
            .frames
            .iter()
            .filter(|f| f.reason == Reason::Gap)
            .count(),
        1
    );
    io.press(true);
    let frames = eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Change)
            .then_some(result.frames)
    });
    assert!(frames.iter().all(|f| f.source != "logical0"));
    inputs.stop();
}
#[test]
fn queue_loss_precedes_fresh_generation_without_replaying_missing_occurrences() {
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.start().unwrap();
    eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline)
            .then_some(())
    });
    for n in 0..650 {
        io.press(n % 2 == 0);
    }
    eventually(|| (inputs.mailbox.lock().unwrap().loss_epoch > 0).then_some(()));
    eventually(|| {
        inputs
            .mailbox
            .lock()
            .unwrap()
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline && f.generation.0 > 1)
            .then_some(())
    });
    let result = inputs.poll(|_| Ok(())).unwrap();
    assert_eq!(result.frames[0].reason, Reason::Gap);
    assert!(result.frames.iter().skip(1).all(|f| f.generation.0 > 1));
    assert!(result
        .frames
        .windows(2)
        .all(|pair| pair[0].captured_us.0 <= pair[1].captured_us.0));
    assert!(result
        .frames
        .iter()
        .flat_map(|f| &f.changes)
        .filter(|c| c.port_id == "button-event:0")
        .all(|c| c.sample == SignalSample::Unknown));
    assert!(result.frames.len() <= MAX_QUEUE + 1);
    inputs.stop();
}
#[test]
fn empty_reader_and_stale_cached_baseline_are_bounded_and_idempotent() {
    let io = Arc::new(Fake::new());
    let plan = empty_plan();
    let mut empty = PanelInputs::prepare_io(&plan, io).unwrap();
    empty.acquire(0).unwrap();
    assert!(empty
        .baseline(&plan, |_| Ok(()))
        .unwrap()
        .unwrap()
        .is_empty());
    empty.start().unwrap();
    assert!(empty.poll(|_| Ok(())).unwrap().frames.is_empty());
    empty.stop();
    empty.stop();
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.stop();
    inputs.revoked.store(false, Ordering::Release);
    inputs.clock = Some((Instant::now(), 1_000_000));
    let frame = Frame {
        source: "logical0".into(),
        generation: Counter(1),
        sequence: Counter(1),
        captured_us: Counter(1),
        reason: Reason::Baseline,
        changes: vec![],
    };
    inputs
        .mailbox
        .lock()
        .unwrap()
        .baselines
        .insert("logical0".into(), frame);
    assert!(inputs.baseline(&plan, |_| Ok(())).unwrap().is_none());
    inputs.stop();
}

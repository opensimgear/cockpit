use super::*;
use osg_mapping::{
    desktop::{Action, ActionDefinition, Scheduler},
    program::SignalSample,
    signal::PortValue,
};

#[test]
fn late_key_hold_transfers_exactly_one_head_and_same_token_cancellation_revokes_it() {
    for revoked in [false, true] {
        let (mut channel, requests, events) = channel();
        let (arrival, resume) = block(&mut channel, "deliver");
        let output = Output::threaded(Coordinator::with_channel(Box::new(channel), || 10),
            Arc::new(|| {}), BTreeMap::new()).unwrap();
        let mut first = held("first", 1);
        first.producer.run.session = uuid::Uuid::new_v4().to_string();
        let token = first.producer.clone();
        let admission = Some(output.activate(token.run.clone(), Scope::DesktopWide).unwrap().admission);
        output.reconcile(&BTreeMap::from([
            ("first".into(), token.clone()), ("late".into(), token.clone()),
            ("following".into(), token.clone()),
        ]));
        output.dispatch_prepared(vec![], Some(("late".into(), token.clone())), 10, 1, admission, Default::default()).unwrap();
        wait_until(|| output.shared.lock().completion.is_some());
        assert!(output.take_completion().unwrap().error.is_none());

        let definitions = [("late", Key(5)), ("following", Key(6))].into_iter()
            .map(|(id, key)| ActionDefinition { id: id.into(), action: Action::KeyHold { key } }).collect();
        let mut scheduler = Scheduler::new(token.run.clone(), definitions).unwrap();
        for id in ["late", "following"] {
            scheduler.accept(id, token.clone(), SignalSample::scalar(PortValue::Boolean(false)), Counter(0), true).unwrap();
        }
        let baseline = scheduler.drain();
        output.dispatch_prepared(vec![first], None, 10, 1, admission, [Counter(1)].into()).unwrap();
        arrival.recv_timeout(Duration::from_secs(1)).unwrap();
        for id in ["late", "following"] {
            scheduler.accept(id, token.clone(), SignalSample::scalar(PortValue::Boolean(true)), Counter(10), false).unwrap();
        }
        let late = scheduler.pending_deliveries().next().unwrap().clone();
        // Do not transfer across a shadow change or skip a queued operation.
        output.enqueue_late_press(&mut scheduler, 10, 2, admission).unwrap();
        assert_eq!(scheduler.pending_deliveries().count(), 2);
        output.enqueue_late_press(&mut scheduler, 10, 1, admission).unwrap();
        output.enqueue_late_press(&mut scheduler, 10, 1, admission).unwrap();
        assert_eq!(scheduler.pending_deliveries().count(), 1);
        assert_eq!(scheduler.pending_deliveries().next().unwrap().action_id, "following");
        if revoked {
            scheduler.cancel_action("late", "test same-token cancellation").unwrap();
            assert!(output.current(&late));
        }
        assert!(output.take_completion().is_none());
        resume.send(()).unwrap();
        wait_until(|| output.shared.lock().completion.is_some());
        let completion = output.take_completion().unwrap();
        assert!(completion.error.is_none());
        assert_eq!(completion.outcomes.len(), if revoked { 1 } else { 2 });
        assert_eq!(completion.dropped.len(), usize::from(revoked));
        if revoked {
            assert_eq!(completion.dropped[0].0, late);
        } else {
            assert_eq!(completion.outcomes[1].0, late);
        }
        assert_eq!(events.lock().unwrap().len(), if revoked { 1 } else { 2 });
        let operations: Vec<_> = requests.lock().unwrap().iter().filter_map(|request| {
            match &request.command {
                Command::Deliver { delivery } => Some(delivery.operation_id),
                _ => None,
            }
        }).collect();
        let expected = if revoked { vec![Counter(1)] } else { vec![Counter(1), late.operation_id] };
        assert_eq!(operations, expected);
        let tail = scheduler.drain();
        assert_eq!(scheduler.trace().replay().unwrap().deliveries,
            baseline.into_iter().chain([late]).chain(tail).collect::<Vec<_>>());
        output.stop_all().unwrap();
    }
}

#[test]
fn fresh_baseline_revokes_claimed_press_without_cancelling_its_successor() {
    let (channel, _, _) = channel();
    let output = Output::inline(Coordinator::with_channel(Box::new(channel), || 10));
    let mut token = held("key", 1).producer;
    token.run.session = uuid::Uuid::new_v4().to_string();
    let admission = Some(output.activate(token.run.clone(), Scope::DesktopWide).unwrap().admission);
    output.reconcile(&BTreeMap::from([("key".into(), token.clone())]));
    let mut scheduler = Scheduler::new(token.run.clone(), vec![
        ActionDefinition { id: "key".into(), action: Action::KeyHold { key: Key(4) } },
    ]).unwrap();
    let sample = |pressed| SignalSample::scalar(PortValue::Boolean(pressed));
    scheduler.accept("key", token.clone(), sample(false), Counter(0), true).unwrap();
    scheduler.drain();
    scheduler.accept("key", token.clone(), sample(true), Counter(10), false).unwrap();
    {
        let mut state = output.shared.lock();
        state.executing_admission = admission;
        state.late_press = late_press::Slot::Open;
    }
    output.enqueue_late_press(&mut scheduler, 10, 0, admission).unwrap();
    let press = output.shared.take_late_press().unwrap();
    let operation = press.delivery.operation_id;
    let request = Request { sequence: Counter(5), deadline_us: None, admission,
        command: Command::Deliver { delivery: press.delivery } };
    assert!(output.shared.permitted(&request));

    // The same producer emits a fresh baseline while the second intent is
    // being committed, then emits a distinct valid press.
    scheduler.accept("key", token.clone(), sample(false), Counter(11), true).unwrap();
    scheduler.accept("key", token, sample(true), Counter(12), false).unwrap();
    let successor = scheduler.pending_deliveries().last().unwrap();
    assert!(late_press::single_key_press(successor));
    assert!(!output.shared.permitted(&request));
    let mut completion = Completion::new(0);
    output.shared.lock().late_press.retire(&mut completion);
    assert!(!completion.cancellation_current(operation));
    assert!(completion.cancellation_current(successor.operation_id));
}

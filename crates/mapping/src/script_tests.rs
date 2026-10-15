use crate::{engine::*, fixtures::*, profile::*, script::*};
fn scripted() -> Engine {
    let mut p = profile();
    p.scripts.push(definition());
    create(p, assignments(), Counter(1)).unwrap()
}
fn definition() -> Script {
    serde_json::from_value(serde_json::json!({"id":"logic","source":"fn on_event(ctx){}","inputs":[{"kind":"source","role":"left","control":"trigger"},{"kind":"source","role":"right","control":"trigger"}],"outputs":[{"id":"result","output":{"target":"main","control":{"kind":"button","index":1}}}]})).unwrap()
}
fn tx(pressed: bool) -> Transaction {
    serde_json::from_value(serde_json::json!({"state":{"held":pressed},"outputs":[{"output":"result","value":{"kind":"button","value":pressed}}],"timers":[]})).unwrap()
}
fn ready(e: &mut Engine) -> Invocation {
    e.accept(baseline("left", 1, false)).unwrap();
    assert!(e.script_requests().is_empty());
    e.accept(baseline("right", 1, false)).unwrap();
    let req = e.script_requests().pop().unwrap().1;
    assert_eq!(req.callback, "on_start");
    req
}
#[test]
fn complete_transactions_pulse_timing_and_source_ownership() {
    let mut e = scripted();
    let req = ready(&mut e);
    let mut pulse = tx(true);
    pulse.timers.push(Timer {
        id: "release".into(),
        after_us: 5000,
    });
    e.script_reply("logic", &req.ticket, pulse).unwrap();
    assert_eq!(e.contributions()["logic:result"], Value::Button(true));
    e.advance(Counter(5000)).unwrap();
    assert!(e.script_requests().is_empty());
    e.advance(Counter(5001)).unwrap();
    let next = e.script_requests().pop().unwrap().1;
    assert_eq!(next.callback, "on_timer");
    assert_eq!(next.event["timer"], "release");
    assert_eq!(next.state["held"], true);
    e.script_reply("logic", &next.ticket, tx(false)).unwrap();
    let edges = e
        .drain()
        .into_iter()
        .filter(|r| r.target == "main")
        .map(|r| (r.at_us.0, r.state.buttons[1]))
        .collect::<Vec<_>>();
    assert!(edges.contains(&(1, true)));
    assert!(edges.contains(&(5001, false)));
    for source in ["left", "right"] {
        let mut e = scripted();
        let req = ready(&mut e);
        e.script_reply("logic", &req.ticket, tx(true)).unwrap();
        e.accept(frame(
            "right",
            1,
            2,
            10,
            Reason::Change,
            vec![("trigger", Value::Button(true))],
        ))
        .unwrap();
        let pending = e.script_requests().pop().unwrap().1;
        e.accept(frame(
            source,
            1,
            if source == "right" { 3 } else { 2 },
            20,
            Reason::Disconnect,
            vec![],
        ))
        .unwrap();
        assert!(!e.contributions().contains_key("logic:result"));
        assert!(e.script_reply("logic", &pending.ticket, tx(true)).is_err());
        assert!(e.script_status()[0]["fault"].is_string());
        let mut baseline = baseline(source, 2, false);
        baseline.captured_us = Counter(21);
        e.accept(baseline).unwrap();
        assert!(e.script_requests().is_empty());
    }
}
#[test]
fn invalid_replies_do_not_partially_commit_and_do_not_release_other_writers() {
    let mut e = scripted();
    let req = ready(&mut e);
    e.script_reply("logic", &req.ticket, tx(true)).unwrap();
    e.accept(frame(
        "right",
        1,
        2,
        5,
        Reason::Change,
        vec![("trigger", Value::Button(true))],
    ))
    .unwrap();
    let req = e.script_requests().pop().unwrap().1;
    let mut invalid = tx(true);
    invalid.timers.push(Timer {
        id: "x".into(),
        after_us: 0,
    });
    assert!(e.script_reply("logic", &req.ticket, invalid).is_err());
    assert!(!e.contributions().contains_key("logic:result"));
    assert_eq!(e.contributions()["right-trigger"], Value::Button(true));
    assert!(e.script_requests().is_empty());
    e.stop();
    assert!(e.script_requests().is_empty());
    assert!(e.script_reply("logic", &req.ticket, tx(true)).is_err());
}
#[test]
fn queues_state_outputs_timers_and_logs_have_independent_budgets() {
    let mut e = scripted();
    let req = ready(&mut e);
    for n in 2..40 {
        e.accept(frame(
            "left",
            1,
            n,
            n,
            Reason::Change,
            vec![("trigger", Value::Button(n % 2 == 0))],
        ))
        .unwrap();
    }
    assert!(e.script_status()[0]["fault"]
        .as_str()
        .unwrap()
        .contains("queue"));
    assert!(e.script_reply("logic", &req.ticket, tx(true)).is_err());
    let p = profile();
    let script = definition();
    let mut t = tx(true);
    t.state = serde_json::json!({"huge":"x".repeat(16_384)});
    assert!(t.validate(&script, &p).is_err());
    t = tx(true);
    t.outputs.push(t.outputs[0].clone());
    assert!(t.validate(&script, &p).is_err());
    t = tx(true);
    t.outputs[0].output = "undeclared".into();
    assert!(t.validate(&script, &p).is_err());
    t = tx(true);
    t.timers = (0..65)
        .map(|n| Timer {
            id: format!("t{n}"),
            after_us: 1000,
        })
        .collect();
    assert!(t.validate(&script, &p).is_err());
    t = tx(true);
    t.logs = vec!["flood".into(); 5];
    assert!(t.validate(&script, &p).is_err());
    let mut e = scripted();
    let mut req = ready(&mut e);
    for n in 0..5 {
        let mut t = tx(true);
        t.logs = vec!["message".into(); 4];
        let result = e.script_reply("logic", &req.ticket, t);
        if n == 4 {
            assert!(result.is_err());
            break;
        }
        result.unwrap();
        e.accept(frame(
            "left",
            1,
            n + 2,
            n + 2,
            Reason::Change,
            vec![("trigger", Value::Button(false))],
        ))
        .unwrap();
        req = e.script_requests().pop().unwrap().1;
    }
}
#[test]
fn script_schema_rejects_undeclared_inputs_duplicate_owners_and_ambiguous_axes() {
    let mut p = profile();
    let mut s = definition();
    s.inputs.push(Input::Node {
        node: "left-trigger".into(),
    });
    p.scripts = vec![s];
    assert!(p.compile().is_err());
    p.scripts = vec![definition(), definition()];
    assert!(p.compile().is_err());
    p.scripts = vec![definition()];
    p.scripts[0].outputs[0].output.control = OutputControl::Axis { index: 0 };
    assert!(p.compile().is_err());
    p.arbitration.push(Arbitration {
        output: p.scripts[0].outputs[0].output.clone(),
        priority: vec!["logic:result".into(), "left-x".into()],
    });
    assert!(p.compile().is_ok());
    p.scripts[0].source = "x".repeat(16385);
    assert!(p.compile().is_err());
}
#[test]
fn recorded_callbacks_replay_identically_and_tampering_is_rejected() {
    let mut p = profile();
    p.scripts = vec![definition()];
    let mut runtime = crate::runtime::Runtime::default();
    let token = runtime.start(p, assignments()).unwrap();
    runtime
        .accept_for(&token, baseline("left", 1, false))
        .unwrap();
    runtime
        .accept_for(&token, baseline("right", 1, false))
        .unwrap();
    let req = runtime.script_requests().pop().unwrap().1;
    runtime
        .script_reply("logic", &req.ticket, tx(true))
        .unwrap();
    runtime.advance_for(&token, Counter(200)).unwrap();
    runtime.advance_for(&token, Counter(300)).unwrap();
    runtime.stop();
    let bytes = runtime.trace.export().unwrap();
    let mut trace = crate::trace::Trace::parse(&bytes).unwrap();
    assert_eq!(trace.replay().unwrap(), trace.replay().unwrap());
    trace.profile.scripts[0].source.push(' ');
    assert!(trace.replay().is_err());
    let mut capture = crate::trace::Capture::start(profile(), assignments());
    for _ in 0..4098 {
        capture.push(crate::trace::Step::Stop);
    }
    assert!(capture.export().is_err());
}

#[test]
fn replaced_timer_set_cancels_already_queued_timer_callbacks() {
    let mut e = scripted();
    let req = ready(&mut e);
    let mut pulse = tx(true);
    pulse.timers.push(Timer {
        id: "old".into(),
        after_us: 5000,
    });
    e.script_reply("logic", &req.ticket, pulse).unwrap();
    e.accept(frame(
        "left",
        1,
        2,
        100,
        Reason::Change,
        vec![("trigger", Value::Button(false))],
    ))
    .unwrap();
    let event = e.script_requests().pop().unwrap().1;
    e.advance(Counter(5001)).unwrap();
    assert_eq!(e.script_status()[0]["pending"], 1);
    e.script_reply("logic", &event.ticket, tx(false)).unwrap();
    assert!(e.script_requests().is_empty());
    assert_eq!(e.contributions()["logic:result"], Value::Button(false));
}

use super::{helper, NATIVE_HELPERS};
use osg_logic::typed_worker::TypedWorker;
use osg_mapping::{
    engine::{
        typed::{Change, Frame, ScriptInvocation, ScriptTransaction},
        Counter, Reason,
    },
    program::*,
    runtime::Runtime,
    signal::*,
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn program(body: &str) -> Program {
    let event = PortContract::Event {
        schema: ScalarSchema::Boolean {},
    };
    let lamp = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: Some(PortValue::Boolean(false)),
    };
    let parameters = vec![
        ActionParameter {
            id: "trigger".into(),
            name: "Trigger".into(),
            schema: ScalarSchema::Boolean {},
        },
        ActionParameter {
            id: "gain:0".into(),
            name: "Gain".into(),
            schema: ScalarSchema::Integer {
                min: 0,
                max: 10,
                unit: None,
            },
        },
    ];
    let output = PortContract::Invocation {
        semantics: ActionSemantics::OneShot,
        parameters: parameters.clone(),
    };
    Program{sources:vec![SourcePort{endpoint:endpoint("source","input"),contract:event.clone()}],nodes:vec![Node{id:"logic".into(),inputs:vec![InputBinding{port_id:"input".into(),from:endpoint("source","input"),contract:event,edge_id:"edge-in".into()}],outputs:vec![PortDescriptor{id:"lamp".into(),name:"Lamp".into(),direction:Direction::Output,contract:lamp.clone()},PortDescriptor{id:"call".into(),name:"Call".into(),direction:Direction::Output,contract:output.clone()}],operation:Operation::Script{source:format!("fn on_start(ctx) {{ transaction(#{{count:0}},#{{lamp:scalar(false)}},[]) }} fn on_event(ctx) {{ {body} }}"),parameters:BTreeMap::new()}}],
    sinks:vec![Sink{id:"lamp-sink".into(),endpoint:endpoint("device","lamp"),from:endpoint("logic","lamp"),contract:lamp,kind:SinkKind::Resource{adapter:ResourceAdapter::DeviceAction}},Sink{id:"call-sink".into(),endpoint:endpoint("device","call"),from:endpoint("logic","call"),contract:PortContract::Action{endpoint:"control".into(),parameters,semantics:ActionSemantics::OneShot,readable:false,display:None,ownership:None},kind:SinkKind::Resource{adapter:ResourceAdapter::DeviceAction}}],controllers:vec![]}
}
fn frame(sequence: u64, at: u64, reason: Reason, value: bool) -> Frame {
    Frame {
        source: "source".into(),
        generation: Counter(1),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: vec![Change {
            port_id: "input".into(),
            sample: SignalSample::scalar(PortValue::Boolean(value)),
        }],
    }
}
fn request(runtime: &mut Runtime) -> ScriptInvocation {
    let mut requests = runtime.typed_engine_mut().unwrap().script_requests();
    assert_eq!(requests.len(), 1);
    requests.remove(0)
}
fn result(
    worker: &mut TypedWorker,
) -> (
    osg_mapping::engine::typed::ScriptTicket,
    Result<ScriptTransaction, String>,
) {
    let start = Instant::now();
    loop {
        if let Some(result) = worker.poll() {
            return result;
        }
        assert!(start.elapsed() < Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn callback(worker: &mut TypedWorker, runtime: &mut Runtime) {
    let request = request(runtime);
    let node = request.node_id.clone();
    worker.submit(request).unwrap();
    let (ticket, reply) = result(worker);
    runtime
        .typed_engine_mut()
        .unwrap()
        .script_reply(&node, &ticket, reply.unwrap())
        .unwrap();
}
#[test]
fn actual_typed_process_commits_atomic_actions_and_cancels_loss_stop_old_callbacks() {
    let _helpers = NATIVE_HELPERS.lock().unwrap_or_else(|p| p.into_inner());
    let program = program(
        r#"transaction(#{count:ctx.state.count+1},#{lamp:scalar(true),"call":action([argument("trigger",true),argument("gain:0",3)])},[after("release",1000,#{lamp:scalar(false)})])"#,
    );
    let mut worker = TypedWorker::spawn(helper(), &program.nodes[0]).unwrap();
    assert!(worker.alive());
    assert!(worker.id().is_some());
    assert_eq!(worker.source_hash.len(), 64);
    let mut runtime = Runtime::default();
    let run = runtime.start_program(program.clone()).unwrap();
    runtime
        .accept_signal_for(&run, frame(1, 1, Reason::Baseline, false))
        .unwrap();
    callback(&mut worker, &mut runtime);
    assert!(!runtime.drain_effects().iter().any(|effect| matches!(
        effect.sample,
        SignalSample::Known(SignalValue::ActionPack(_))
    )));
    runtime
        .accept_signal_for(&run, frame(2, 10, Reason::Change, true))
        .unwrap();
    callback(&mut worker, &mut runtime);
    let effects = runtime.drain_effects();
    let calls = effects
        .iter()
        .filter_map(|effect| match &effect.sample {
            SignalSample::Known(SignalValue::ActionPack(pack)) => Some(pack),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments[0].id, "trigger");
    assert_eq!(calls[0].arguments[1].id, "gain:0");
    runtime.advance_for(&run, Counter(1010)).unwrap();
    let timer_effects = runtime.drain_effects();
    assert!(timer_effects
        .iter()
        .any(|effect| effect.sink_id == "lamp-sink"
            && effect.sample == SignalSample::scalar(PortValue::Boolean(false))));
    assert!(!timer_effects.iter().any(|effect| matches!(
        effect.sample,
        SignalSample::Known(SignalValue::ActionPack(_))
    )));
    runtime
        .accept_signal_for(&run, frame(3, 1020, Reason::Change, false))
        .unwrap();
    let late = request(&mut runtime);
    worker.submit(late).unwrap();
    assert!(worker.pending().is_some());
    runtime
        .accept_signal_for(&run, frame(4, 1030, Reason::Disconnect, false))
        .unwrap();
    let (ticket, reply) = result(&mut worker);
    assert!(runtime
        .typed_engine_mut()
        .unwrap()
        .script_reply("logic", &ticket, reply.unwrap())
        .is_err());
    runtime.advance_for(&run, Counter(2000)).unwrap();
    assert!(!runtime.drain_effects().iter().any(|effect| matches!(
        effect.sample,
        SignalSample::Known(SignalValue::ActionPack(_))
    )));
    runtime
        .accept_signal_for(&run, frame(5, 3000, Reason::Baseline, false))
        .unwrap();
    let late = request(&mut runtime);
    worker.submit(late).unwrap();
    runtime.stop();
    let (ticket, reply) = result(&mut worker);
    assert!(runtime
        .typed_engine_mut()
        .unwrap()
        .script_reply("logic", &ticket, reply.unwrap())
        .is_err());
    let next = runtime.start_program(program).unwrap();
    assert_ne!(next, run);
    assert!(runtime
        .typed_engine_mut()
        .unwrap()
        .script_reply("logic", &ticket, ScriptTransaction::default())
        .is_err());
    worker.terminate();
    assert!(!worker.alive());
    assert!(worker.poll().is_none());
}
#[test]
fn typed_helper_faults_do_not_retry_or_keep_a_worker_alive() {
    let _helpers = NATIVE_HELPERS.lock().unwrap_or_else(|p| p.into_inner());
    for body in ["loop {}", r#"transaction(#{},#{unknown:scalar(true)},[])"#] {
        let program = program(body);
        let mut worker = TypedWorker::spawn(helper(), &program.nodes[0]).unwrap();
        let mut runtime = Runtime::default();
        let run = runtime.start_program(program).unwrap();
        runtime
            .accept_signal_for(&run, frame(1, 1, Reason::Baseline, false))
            .unwrap();
        callback(&mut worker, &mut runtime);
        runtime
            .accept_signal_for(&run, frame(2, 10, Reason::Change, true))
            .unwrap();
        let request = request(&mut runtime);
        worker.submit(request.clone()).unwrap();
        assert!(worker.submit(request).is_err());
        let (_, reply) = result(&mut worker);
        assert!(reply.is_err());
        assert!(!worker.alive());
        assert!(worker.pending().is_none());
        runtime
            .typed_engine_mut()
            .unwrap()
            .script_fault("logic", "helper rejected callback");
        assert!(runtime
            .typed_engine_mut()
            .unwrap()
            .script_requests()
            .is_empty());
    }
    let mut node = program("transaction(#{},#{},[])").nodes.remove(0);
    node.operation = Operation::Script {
        source: "fn on_start(ctx){}".into(),
        parameters: BTreeMap::new(),
    };
    assert!(TypedWorker::spawn(helper(), &node).is_err());
    assert!(TypedWorker::spawn(std::path::Path::new("/missing-osg-logic"), &node).is_err());
}
#[test]
fn typed_process_rejects_injected_undeclared_input() {
    let _helpers = NATIVE_HELPERS.lock().unwrap_or_else(|p| p.into_inner());
    let program = program("transaction(#{},#{},[])");
    let mut worker = TypedWorker::spawn(helper(), &program.nodes[0]).unwrap();
    let mut runtime = Runtime::default();
    let run = runtime.start_program(program).unwrap();
    runtime
        .accept_signal_for(&run, frame(1, 1, Reason::Baseline, false))
        .unwrap();
    let mut request = request(&mut runtime);
    request
        .inputs
        .insert("injected".into(), SignalSample::Unknown);
    assert!(worker.submit(request).is_err());
    assert!(!worker.alive());
}

#[test]
fn typed_child_exits_on_eof_rejects_recompile_and_process_death_is_fatal() {
    let _helpers = NATIVE_HELPERS.lock().unwrap_or_else(|p| p.into_inner());
    use osg_logic::{typed::Definition, wire};
    use std::process::{Command, Stdio};
    let program = program("transaction(#{},#{},[])");
    let definition = Definition::from_node(&program.nodes[0]).unwrap();
    for recompile in [false, true] {
        let mut child = Command::new(helper())
            .arg(osg_logic::typed_worker::ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let request = serde_json::json!({"kind":"compile","definition":definition});
        wire::write(&request, &mut input).unwrap();
        assert_eq!(
            wire::read::<serde_json::Value>(&mut output).unwrap()["kind"],
            "compiled"
        );
        if recompile {
            wire::write(&request, &mut input).unwrap();
        }
        drop(input);
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.success(), !recompile);
                break;
            }
            if started.elapsed() > Duration::from_secs(2) {
                child.kill().unwrap();
                panic!("typed child did not exit")
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let mut worker = TypedWorker::spawn(helper(), &program.nodes[0]).unwrap();
    let pid = worker.id().unwrap();
    #[cfg(unix)]
    assert!(Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap()
        .success());
    #[cfg(windows)]
    assert!(Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .unwrap()
        .status
        .success());
    let mut runtime = Runtime::default();
    let run = runtime.start_program(program).unwrap();
    runtime
        .accept_signal_for(&run, frame(1, 1, Reason::Baseline, false))
        .unwrap();
    if worker.submit(request(&mut runtime)).is_ok() {
        assert!(result(&mut worker).1.is_err());
    }
    assert!(!worker.alive());
    assert!(worker.pending().is_none());
}

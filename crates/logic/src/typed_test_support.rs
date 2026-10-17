use crate::typed::{Definition, Invocation};
use osg_mapping::{
    engine::{
        typed::{ScriptInvocation, ScriptTicket, Token},
        Counter, RunToken,
    },
    program::*,
    signal::*,
};
use std::collections::BTreeMap;
pub(crate) fn boolean() -> PortContract {
    PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: Some(PortValue::Boolean(false)),
    }
}
pub(crate) fn event() -> PortContract {
    PortContract::Event {
        schema: ScalarSchema::Boolean {},
    }
}
pub(crate) fn descriptor(id: &str, direction: Direction, contract: PortContract) -> PortDescriptor {
    PortDescriptor {
        id: id.into(),
        name: id.into(),
        direction,
        contract,
    }
}
pub(crate) fn node(body: &str) -> Node {
    Node {
        id: "logic".into(),
        inputs: vec![InputBinding {
            port_id: "input".into(),
            from: Endpoint {
                node_id: "source".into(),
                port_id: "input".into(),
            },
            contract: event(),
            edge_id: "edge-in".into(),
        }],
        outputs: vec![
            descriptor("lamp", Direction::Output, boolean()),
            descriptor(
                "call",
                Direction::Output,
                PortContract::Invocation {
                    semantics: ActionSemantics::OneShot,
                    parameters: vec![
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
                    ],
                },
            ),
        ],
        operation: Operation::Script {
            source: format!(
                "fn on_start(ctx) {{ transaction(#{{}},#{{}},[]) }} fn on_event(ctx) {{ {body} }}"
            ),
            parameters: BTreeMap::new(),
        },
    }
}
pub(crate) fn request(event_trigger: bool) -> Invocation {
    Invocation {
        node_id: "logic".into(),
        ticket: ScriptTicket {
            producer: Token {
                run: RunToken {
                    session: "679b801a-dfa4-4806-9354-6e30525ebfed".into(),
                    generation: Counter(1),
                },
                node_id: "logic".into(),
                dependencies: vec![],
            },
            sequence: Counter(1),
        },
        at_us: Counter(10),
        callback: if event_trigger {
            "on_event"
        } else {
            "on_start"
        }
        .into(),
        event_trigger,
        inputs: BTreeMap::from([(
            "input".into(),
            SignalSample::scalar(PortValue::Boolean(true)),
        )]),
        parameters: BTreeMap::new(),
        state: BTreeMap::new(),
    }
}
pub(crate) fn invocation(value: Invocation) -> ScriptInvocation {
    ScriptInvocation {
        node_id: value.node_id,
        ticket: value.ticket,
        at_us: value.at_us,
        callback: value.callback,
        event_trigger: value.event_trigger,
        inputs: value.inputs,
        parameters: value.parameters,
        state: value.state,
    }
}
pub(crate) fn definition() -> Definition {
    Definition::from_node(&node("transaction(#{},#{},[]) ")).unwrap()
}

use crate::{engine::*, profile::*};
pub fn profile() -> Profile {
    Profile::parse(include_bytes!("../fixtures/direct-profile.json")).unwrap()
}
pub fn assignments() -> Vec<Assignment> {
    profile()
        .roles
        .into_iter()
        .map(|role| Assignment {
            source: Source {
                origin: Origin::Synthetic,
                id: role.id.clone(),
            },
            role: role.id,
            controls: role
                .controls
                .into_iter()
                .map(|control| ControlAssignment {
                    source_control: format!("native-{}", control.id),
                    control: control.id,
                })
                .collect(),
        })
        .collect()
}
pub fn frame(
    source: &str,
    generation: u64,
    sequence: u64,
    at: u64,
    reason: Reason,
    changes: Vec<(&str, Value)>,
) -> Frame {
    Frame {
        source: Source {
            origin: Origin::Synthetic,
            id: source.into(),
        },
        generation: Counter(generation),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: changes
            .into_iter()
            .map(|(control, value)| Change {
                control: format!("native-{control}"),
                value,
            })
            .collect(),
    }
}
pub fn baseline(source: &str, generation: u64, pressed: bool) -> Frame {
    let changes = if source == "left" {
        vec![
            ("trigger", Value::Button(pressed)),
            ("x", Value::Axis(10)),
            ("hat", Value::Hat(None)),
        ]
    } else {
        vec![("trigger", Value::Button(pressed))]
    };
    frame(source, generation, 1, 0, Reason::Baseline, changes)
}
pub fn engine() -> Engine {
    Engine::new(profile(), assignments(), Counter(1)).unwrap()
}

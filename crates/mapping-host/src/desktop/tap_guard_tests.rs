use super::*;

#[test]
fn native_tap_duration_map_uses_only_desktop_tap_and_click_down_times() {
    let mut program = source_program(Action::KeyTap { key: Key(4), down_ms: 7, up_ms: 1 }, false);
    let mut add_desktop = |id: &str, action: Action| {
        let mut sink = program.sinks[0].clone();
        sink.id = id.into();
        sink.contract = action.contract();
        sink.kind = SinkKind::Desktop { action };
        program.sinks.push(sink);
    };
    add_desktop("click", Action::MouseClick { button: osg_mapping::desktop::MouseButton::Left, down_ms: 60_000, up_ms: 1 });
    add_desktop("hold", Action::KeyHold { key: Key(5) });
    add_desktop("repeat", Action::Repeat { key: Key(6), delay_ms: 10, interval_ms: 8 });
    drop(add_desktop);
    let mut virtual_sink = program.sinks[0].clone();
    virtual_sink.id = "virtual-tap".into();
    virtual_sink.kind = SinkKind::Virtual {
        controller_id: "controller".into(),
        control: osg_mapping::profile::OutputControl::Button { index: 1 },
    };
    program.sinks.push(virtual_sink);

    assert_eq!(
        tap_durations(&program),
        BTreeMap::from([
            ("action".into(), Counter(7_000)),
            ("click".into(), Counter(60_000_000)),
        ])
    );
}

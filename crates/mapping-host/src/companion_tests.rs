use super::*;
use osg_mapping::{engine::ControlAssignment, profile::AxisRange};
use std::sync::atomic::{AtomicU64, Ordering};
fn hash() -> String {
    format!("sha256:{}", "a".repeat(64))
}
fn provider() -> (CompanionProvider, Arc<AtomicU64>) {
    let clock = Arc::new(AtomicU64::new(100));
    let captured = clock.clone();
    (
        CompanionProvider::new(Arc::new(move || captured.load(Ordering::Acquire))),
        clock,
    )
}
fn definition(id: &str) -> Definition {
    Definition {
        source_id: id.into(),
        name: format!("Panel {id}"),
        publication_hash: hash(),
        controls: vec![
            ControlDefinition {
                id: "trigger".into(),
                kind: Kind::Button,
                fallback: Value::Button(false),
            },
            ControlDefinition {
                id: "x".into(),
                kind: Kind::Axis {
                    range: AxisRange::Bipolar,
                },
                fallback: Value::Axis(123),
            },
            ControlDefinition {
                id: "hat".into(),
                kind: Kind::Hat,
                fallback: Value::Hat(None),
            },
        ],
    }
}
fn token(id: &str, generation: u64) -> InputToken {
    InputToken {
        source_id: id.into(),
        session_id: format!("session-{id}"),
        generation,
        publication_hash: hash(),
    }
}
fn baseline() -> Vec<Change> {
    definition("left")
        .controls
        .into_iter()
        .map(|control| Change {
            control: control.id,
            value: control.fallback,
        })
        .collect()
}
fn change(control: &str, value: Value) -> Change {
    Change {
        control: control.into(),
        value,
    }
}
fn fixture(
    companion_right: bool,
) -> (
    Profile,
    Vec<Assignment>,
    osg_controller_input::stream::Snapshot,
) {
    let mut profile =
        Profile::parse(include_bytes!("../../mapping/fixtures/direct-profile.json")).unwrap();
    profile.schema = 2;
    profile.roles[0].origin = Origin::Companion;
    if companion_right {
        profile.roles[1].origin = Origin::Companion;
    }
    let assignments = profile
        .roles
        .iter()
        .map(|role| Assignment {
            role: role.id.clone(),
            source: Source {
                origin: role.origin,
                id: role.id.clone(),
            },
            controls: role
                .controls
                .iter()
                .map(|control| ControlAssignment {
                    control: control.id.clone(),
                    source_control: control.id.clone(),
                })
                .collect(),
        })
        .collect();
    (
        profile,
        assignments,
        osg_controller_input::stream::Snapshot {
            session: "native-clock".into(),
            cursor: 0,
            captured_us: 100,
            sources: vec![],
        },
    )
}
fn right_definition() -> Definition {
    let mut value = definition("right");
    value.controls.truncate(1);
    value
}
fn right_baseline() -> Vec<Change> {
    vec![change("trigger", Value::Button(false))]
}
fn page(now: u64) -> osg_controller_input::stream::Page {
    osg_controller_input::stream::Page {
        session: "native-clock".into(),
        cursor: 0,
        captured_us: now,
        gap: false,
        events: vec![],
    }
}
#[test]
fn offline_descriptors_are_retained_and_no_baseline_is_fabricated() {
    let (provider, _) = provider();
    provider.register_definition(definition("left")).unwrap();
    let (profile, assignments, snapshot) = fixture(false);
    let prepared = provider.prepare(&profile, &assignments).unwrap();
    let mut router = crate::router::Router::default();
    router
        .start_with_sources(
            profile,
            assignments,
            snapshot,
            BTreeMap::new(),
            None,
            Some(prepared),
        )
        .unwrap();
    assert_eq!(router.status()["running"], true);
    assert_eq!(router.status()["ready"], false);
    assert_eq!(
        provider.list().unwrap()["companionSources"][0]["online"],
        false
    );
    assert_eq!(
        provider.inspect("left").unwrap()["companionControls"][0]["value"],
        serde_json::Value::Null
    );
    assert!(provider.inspect("missing").is_err());
    assert_eq!(provider.state.lock().unwrap().subscriptions.len(), 1);
    router.stop();
    assert!(provider.state.lock().unwrap().subscriptions.is_empty());
    assert_eq!(
        provider.list().unwrap()["companionSources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn ordered_repeated_transitions_use_positive_native_sequences_and_host_clock() {
    let (provider, clock) = provider();
    provider.register_definition(definition("left")).unwrap();
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    let (profile, assignments, snapshot) = fixture(false);
    let prepared = provider.prepare(&profile, &assignments).unwrap();
    let mut router = crate::router::Router::default();
    router
        .start_with_sources(
            profile,
            assignments,
            snapshot,
            BTreeMap::new(),
            None,
            Some(prepared),
        )
        .unwrap();
    assert_eq!(router.status()["ready"], true);
    let before = router.runtime.recording.cursor();
    clock.store(200, Ordering::Release);
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[
                change("trigger", Value::Button(true)),
                change("trigger", Value::Button(false)),
                change("trigger", Value::Button(true)),
            ],
        )
        .unwrap();
    router.pump(page(200)).unwrap();
    let records = router.runtime.recording.page(before, 32).unwrap().records;
    let buttons = records
        .iter()
        .filter(|record| record.report.target == "main")
        .map(|record| record.report.state.buttons[0])
        .collect::<Vec<_>>();
    assert_eq!(buttons, vec![true, false, true]);
    assert!(records.iter().all(|record| record.report.at_us.0 == 200));
    let trace = &router.runtime.trace.trace.as_ref().unwrap().steps;
    let frames = trace
        .iter()
        .filter_map(|step| {
            if let osg_mapping::trace::Step::Input { frame } = step {
                Some(frame)
            } else {
                None
            }
        })
        .filter(|frame| frame.source.origin == Origin::Companion)
        .collect::<Vec<_>>();
    assert_eq!(frames[0].sequence, Counter(1));
    assert_eq!(frames[1].sequence, Counter(2));
    assert_eq!(frames.last().unwrap().sequence, Counter(4));
    assert!(frames.iter().all(|frame| frame
        .changes
        .iter()
        .map(|change| &change.control)
        .collect::<BTreeSet<_>>()
        .len()
        == frame.changes.len()));
}
#[test]
fn peer_loss_and_queue_overflow_release_only_the_affected_source() {
    let (provider, clock) = provider();
    provider.register_definition(definition("left")).unwrap();
    provider.register_definition(right_definition()).unwrap();
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    provider
        .baseline_native(&token("right", 1), 0, &right_baseline())
        .unwrap();
    let (profile, assignments, snapshot) = fixture(true);
    let prepared = provider.prepare(&profile, &assignments).unwrap();
    let mut router = crate::router::Router::default();
    router
        .start_with_sources(
            profile,
            assignments,
            snapshot,
            BTreeMap::new(),
            None,
            Some(prepared),
        )
        .unwrap();
    provider
        .changes_native(
            &token("right", 1),
            1,
            &[change("trigger", Value::Button(true))],
        )
        .unwrap();
    clock.store(200, Ordering::Release);
    router.pump(page(200)).unwrap();
    let events = (0..64)
        .map(|index| change("trigger", Value::Button(index % 2 == 0)))
        .collect::<Vec<_>>();
    for sequence in 1..=8 {
        provider
            .changes_native(&token("left", 1), sequence, &events)
            .unwrap();
    }
    assert!(provider
        .changes_native(
            &token("left", 1),
            9,
            &[change("trigger", Value::Button(true))]
        )
        .is_err());
    clock.store(300, Ordering::Release);
    router.pump(page(300)).unwrap();
    assert_eq!(router.status()["running"], true);
    assert_eq!(router.status()["ready"], false);
    let outputs = router.runtime.engine().unwrap().inspect(0)["outputs"].clone();
    assert_eq!(outputs["main"]["buttons"][0], true);
    assert_eq!(outputs["main"]["axes"][0], -123);
    provider
        .baseline_native(&token("left", 2), 0, &baseline())
        .unwrap();
    clock.store(400, Ordering::Release);
    router.pump(page(400)).unwrap();
    assert_eq!(router.status()["ready"], true);
    provider.loss_native(&token("left", 1)).unwrap_err();
    assert_eq!(
        provider.list().unwrap()["companionSources"][0]["online"],
        true
    );
    provider.loss_native(&token("left", 2)).unwrap();
    clock.store(500, Ordering::Release);
    router.pump(page(500)).unwrap();
    assert_eq!(
        router.runtime.engine().unwrap().inspect(0)["outputs"]["main"]["buttons"][0],
        true
    );
}
#[test]
fn gaps_stale_generations_and_descriptor_changes_are_isolated_and_atomic() {
    let (provider, _) = provider();
    provider.register_definition(definition("left")).unwrap();
    let (profile, assignments, _) = fixture(false);
    let mut feed = provider.prepare(&profile, &assignments).unwrap().feed;
    assert!(feed.initial().unwrap().is_empty());
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    let baseline_frame = feed.drain();
    assert_eq!(baseline_frame[0].sequence, Counter(1));
    assert!(provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .is_err());
    assert!(provider
        .changes_native(
            &token("left", 1),
            2,
            &[change("trigger", Value::Button(true))]
        )
        .is_err());
    assert_eq!(
        feed.drain()[0].reason,
        osg_mapping::engine::Reason::Disconnect
    );
    assert!(provider
        .changes_native(
            &token("left", 1),
            1,
            &[change("trigger", Value::Button(true))]
        )
        .is_err());
    provider
        .baseline_native(&token("left", 2), 0, &baseline())
        .unwrap();
    let mut changed = definition("left");
    changed.controls[1].fallback = Value::Axis(1000);
    assert!(provider.register_definition(changed).is_err());
    assert_eq!(
        provider.inspect("left").unwrap()["source"]["publicationHash"],
        hash()
    );
    let mut styled = definition("left");
    styled.publication_hash = format!("sha256:{}", "b".repeat(64));
    provider.register_definition(styled).unwrap();
    assert_eq!(
        feed.drain()[0].reason,
        osg_mapping::engine::Reason::Disconnect
    );
    assert!(provider
        .baseline_native(&token("left", 3), 0, &baseline())
        .is_err());
    let mut next = token("left", 3);
    next.publication_hash = format!("sha256:{}", "b".repeat(64));
    provider.baseline_native(&next, 0, &baseline()).unwrap();
    let frames = feed.drain();
    assert!(
        frames.is_empty(),
        "A changed immutable revision must not revive an old subscription"
    );
    drop(feed);
    let mut changed = definition("left");
    changed.controls[1].fallback = Value::Axis(1000);
    provider.register_definition(changed).unwrap();
}
#[test]
fn malformed_definitions_baselines_and_input_never_partially_update_values() {
    let (provider, _) = provider();
    for fault in 0..8 {
        let mut value = definition("left");
        match fault {
            0 => value.source_id = "../path".into(),
            1 => value.name = "\n".into(),
            2 => value.publication_hash = "bad".into(),
            3 => value.controls.clear(),
            4 => value.controls.push(value.controls[0].clone()),
            5 => value.controls[0].fallback = Value::Button(true),
            6 => value.controls[2].fallback = Value::Hat(Some(0)),
            _ => value.controls[1].fallback = Value::Axis(40000),
        }
        assert!(provider.register_definition(value).is_err());
    }
    provider.register_definition(definition("left")).unwrap();
    provider.register_definition(definition("left")).unwrap();
    assert!(provider
        .baseline_native(&token("left", 1), 1, &baseline())
        .is_err());
    assert!(provider
        .baseline_native(&token("left", 1), 0, &baseline()[..1])
        .is_err());
    let mut duplicate = baseline();
    duplicate[2] = duplicate[0].clone();
    assert!(provider
        .baseline_native(&token("left", 1), 0, &duplicate)
        .is_err());
    let mut held = baseline();
    held[0].value = Value::Button(true);
    assert!(provider
        .baseline_native(&token("left", 1), 0, &held)
        .is_err());
    let mut bad = token("left", 0);
    assert!(provider.baseline_native(&bad, 0, &baseline()).is_err());
    bad.generation = 1;
    bad.publication_hash = "bad".into();
    assert!(provider.baseline_native(&bad, 0, &baseline()).is_err());
    assert!(provider
        .baseline_native(&token("missing", 1), 0, &baseline())
        .is_err());
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    assert!(provider
        .changes_native(
            &token("left", 1),
            1,
            &[change("unknown", Value::Button(true))]
        )
        .is_err());
    assert_eq!(
        provider.inspect("left").unwrap()["companionControls"][0]["value"],
        serde_json::Value::Null
    );
    assert!(provider.release("missing").is_err());
    provider.release("left").unwrap();
}
#[test]
fn source_sink_converts_normalized_values_and_rejects_stale_or_wrong_kinds() {
    use osg_companion_service::{
        AxisMode, InputValue, SourceChange, SourceControl, SourceDefinition, SourceKind,
        SourceSink, SourceToken,
    };
    let (provider, _) = provider();
    provider
        .register(SourceDefinition {
            source_id: "left".into(),
            name: "Panel".into(),
            publication_hash: hash(),
            controls: vec![
                SourceControl {
                    id: "x".into(),
                    kind: SourceKind::Axis {
                        axis_mode: AxisMode::Bipolar,
                        fallback: 0.0,
                    },
                },
                SourceControl {
                    id: "pedal".into(),
                    kind: SourceKind::Axis {
                        axis_mode: AxisMode::Unipolar,
                        fallback: 0.25,
                    },
                },
                SourceControl {
                    id: "trigger".into(),
                    kind: SourceKind::Button,
                },
                SourceControl {
                    id: "hat".into(),
                    kind: SourceKind::Hat,
                },
            ],
        })
        .unwrap();
    let token = SourceToken {
        source_id: "left".into(),
        session_id: "session".into(),
        generation: 1,
        publication_hash: hash(),
    };
    let values = vec![
        SourceChange {
            control_id: "x".into(),
            value: InputValue::Number(0.0),
        },
        SourceChange {
            control_id: "pedal".into(),
            value: InputValue::Number(0.25),
        },
        SourceChange {
            control_id: "trigger".into(),
            value: InputValue::Button(false),
        },
        SourceChange {
            control_id: "hat".into(),
            value: InputValue::Number(-1.0),
        },
    ];
    provider.baseline(&token, 0, &values).unwrap();
    provider
        .changes(
            &token,
            1,
            &[
                SourceChange {
                    control_id: "x".into(),
                    value: InputValue::Number(-1.0),
                },
                SourceChange {
                    control_id: "hat".into(),
                    value: InputValue::Number(7.0),
                },
            ],
        )
        .unwrap();
    let inspected = provider.inspect("left").unwrap();
    assert_eq!(
        inspected["companionControls"][0]["value"]["normalized"],
        -32768
    );
    assert_eq!(
        inspected["companionControls"][1]["value"]["normalized"],
        8192
    );
    assert_eq!(inspected["companionControls"][3]["value"]["normalized"], 7);
    let mut old = token.clone();
    old.generation = 0;
    assert!(provider
        .changes(
            &old,
            2,
            &[SourceChange {
                control_id: "x".into(),
                value: InputValue::Button(true)
            }]
        )
        .is_err());
    assert_eq!(
        provider.list().unwrap()["companionSources"][0]["online"],
        true
    );
    assert!(provider
        .changes(
            &token,
            2,
            &[SourceChange {
                control_id: "x".into(),
                value: InputValue::Number(f64::NAN)
            }]
        )
        .is_err());
    assert_eq!(
        provider.list().unwrap()["companionSources"][0]["online"],
        false
    );
    assert!(provider.loss(&token).is_err());
}
#[test]
fn subscriptions_filter_unassigned_controls_without_engine_sequence_gaps() {
    let (provider, _) = provider();
    provider.register_definition(definition("left")).unwrap();
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    let (mut profile, mut assignments, _) = fixture(false);
    profile.roles[0].controls.truncate(1);
    profile
        .nodes
        .retain(|node| !["left-x", "left-hat"].contains(&node.id.as_str()));
    assignments[0].controls.truncate(1);
    let mut prepared = provider.prepare(&profile, &assignments).unwrap();
    let initial = prepared.feed.initial().unwrap();
    assert_eq!(initial[0].changes.len(), 1);
    provider
        .changes_native(&token("left", 1), 1, &[change("x", Value::Axis(200))])
        .unwrap();
    assert!(prepared.feed.drain().is_empty());
    provider
        .changes_native(
            &token("left", 1),
            2,
            &[change("trigger", Value::Button(true))],
        )
        .unwrap();
    assert_eq!(prepared.feed.drain()[0].sequence, Counter(2));
    let mut subscriptions = Vec::new();
    for _ in 1..MAX_SUBSCRIPTIONS {
        subscriptions.push(provider.prepare(&profile, &assignments).unwrap());
    }
    assert!(provider.prepare(&profile, &assignments).is_err());
    drop(subscriptions);
    assert!(provider.prepare(&profile, &assignments).is_ok());
}

#[test]
fn bounded_per_instance_fair_drain_does_not_allow_a_hot_peer_to_starve_others() {
    let (provider, _) = provider();
    let (mut profile, _, _) = fixture(false);
    profile.roles.clear();
    profile.nodes.clear();
    profile.targets.truncate(1);
    profile.targets[0].layout = profile::Layout {
        buttons: 8,
        axes: vec![],
        hats: 0,
    };
    let mut assignments = vec![];
    for index in 0..8 {
        let id = format!("instance-{index}");
        let mut definition = definition(&id);
        definition.controls.truncate(1);
        provider.register_definition(definition).unwrap();
        provider
            .baseline_native(&token(&id, 1), 0, &right_baseline())
            .unwrap();
        profile.roles.push(profile::Role {
            id: id.clone(),
            name: id.clone(),
            origin: Origin::Companion,
            controls: vec![profile::Control {
                id: "trigger".into(),
                kind: Kind::Button,
            }],
        });
        profile.nodes.push(profile::Node {
            id: format!("node-{index}"),
            inputs: vec![profile::Input::Source {
                role: id.clone(),
                control: "trigger".into(),
            }],
            output: Some(profile::Output {
                target: "main".into(),
                control: profile::OutputControl::Button { index },
            }),
            operation: profile::Operation::Direct {
                activation: profile::Activation::Momentary,
            },
        });
        assignments.push(Assignment {
            role: id.clone(),
            source: source(&id),
            controls: vec![ControlAssignment {
                control: "trigger".into(),
                source_control: "trigger".into(),
            }],
        });
    }
    assert!(profile.compile().is_ok());
    let mut prepared = provider.prepare(&profile, &assignments).unwrap();
    assert_eq!(prepared.feed.initial().unwrap().len(), 8);
    let changes = (0..64)
        .map(|index| change("trigger", Value::Button(index % 2 == 0)))
        .collect::<Vec<_>>();
    for index in 0..8 {
        let id = format!("instance-{index}");
        provider
            .changes_native(&token(&id, 1), 1, &changes)
            .unwrap();
        provider
            .changes_native(&token(&id, 1), 2, &changes)
            .unwrap();
    }
    let first = prepared.feed.drain();
    let second = prepared.feed.drain();
    assert_eq!(first.len(), 512);
    assert_eq!(second.len(), 512);
    assert_eq!(
        first
            .iter()
            .map(|frame| &frame.source.id)
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
    assert_ne!(first[0].source.id, second[0].source.id);
}
#[test]
fn registry_and_counter_exhaustion_fail_closed_and_stale_batches_do_not_release_new_owners() {
    let (provider, _) = provider();
    provider.register_definition(definition("left")).unwrap();
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[change("trigger", Value::Button(true))],
        )
        .unwrap();
    assert!(provider
        .changes_native(
            &token("left", 1),
            1,
            &[change("trigger", Value::Button(false))]
        )
        .is_err());
    assert_eq!(
        provider.inspect("left").unwrap()["companionControls"][0]["value"]["normalized"],
        1
    );
    provider
        .baseline_native(&token("left", 2), 0, &baseline())
        .unwrap();
    assert!(provider
        .changes_native(
            &token("left", 1),
            2,
            &[change("trigger", Value::Button(true))]
        )
        .is_err());
    assert_eq!(
        provider.list().unwrap()["companionSources"][0]["online"],
        true
    );
    {
        let mut state = provider.state.lock().unwrap();
        state.instances.get_mut("left").unwrap().generation = u64::MAX;
    }
    assert!(provider
        .baseline_native(&token("left", 3), 0, &baseline())
        .is_err());
    provider.release("left").unwrap();
    for index in 1..MAX_REGISTERED {
        provider
            .register_definition(definition(&format!("panel-{index}")))
            .unwrap();
    }
    assert!(provider
        .register_definition(definition("overflow"))
        .is_err());
}
#[test]
fn service_clock_consumes_companion_events_without_frontend_polling_or_optional_drivers() {
    let root = tempfile::tempdir().unwrap();
    let (profile, assignments, _) = fixture(false);
    let store = osg_mapping::store::Store::new(root.path().into());
    store.save(&profile).unwrap();
    store.save_assignments(&profile.id, &assignments).unwrap();
    let capture = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let input = capture.clone();
    let provider = Arc::new(CompanionProvider::new(Arc::new(move || {
        input.stream_page("", 0).captured_us
    })));
    provider.register_definition(definition("left")).unwrap();
    let service = crate::service::Service::new(
        capture,
        Some(root.path().into()),
        Arc::new(|| Err("No virtual drivers in fixture".into())),
    );
    assert_eq!(
        service
            .request(&json!({"command":"mapping_companion_sources"}))
            .unwrap()["companionSources"],
        json!([])
    );
    assert!(service
        .request(&json!({"command":"mapping_companion_inspect","sourceId":"left"}))
        .is_err());
    service.set_companion_provider(provider.clone());
    assert_eq!(
        service
            .request(&json!({"command":"mapping_companion_sources"}))
            .unwrap()["companionSources"][0]["origin"],
        "companion"
    );
    assert_eq!(
        service
            .request(&json!({"command":"mapping_companion_inspect","sourceId":"left"}))
            .unwrap()["companionControls"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    service
        .request(&json!({"command":"mapping_preview_start","profileId":profile.id}))
        .unwrap();
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["ready"],
        false
    );
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[
                change("trigger", Value::Button(true)),
                change("trigger", Value::Button(false)),
            ],
        )
        .unwrap();
    // The worker advances independently; no UI request transports these events.
    std::thread::sleep(std::time::Duration::from_millis(35));
    let records = service
        .request(&json!({"command":"mapping_recording","after":"0"}))
        .unwrap();
    let buttons = records["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["report"]["target"] == "main")
        .map(|record| record["report"]["state"]["buttons"][0].as_bool().unwrap())
        .collect::<Vec<_>>();
    assert!(buttons.windows(2).any(|pair| pair == [true, false]));
    provider.loss_native(&token("left", 1)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(15));
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        true
    );
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["ready"],
        false
    );
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    service.shutdown();
    assert!(provider.state.lock().unwrap().subscriptions.is_empty());
}

#[test]
fn captured_game_start_uses_companion_feed_without_relabeling_or_live_output() {
    let root = tempfile::tempdir().unwrap();
    let (profile, assignments, _) = fixture(false);
    let capture = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let clock = capture.clone();
    let provider = Arc::new(CompanionProvider::new(Arc::new(move || {
        clock.stream_page("", 0).captured_us
    })));
    provider.register_definition(definition("left")).unwrap();
    let service = crate::service::Service::new(
        capture,
        Some(root.path().into()),
        Arc::new(|| Err("No live output fixture".into())),
    );
    service.set_companion_provider(provider.clone());
    let desired =
        json!({"profile":profile,"assignments":assignments,"targets":[],"visibility":null});
    let journal = service.game_snapshot(&desired).unwrap();
    service.game_take(&journal).unwrap();
    service.game_arm(&journal).unwrap();
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["ready"],
        false
    );
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[change("trigger", Value::Button(true))],
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(25));
    let status = service
        .request(&json!({"command":"mapping_status","inspect":true}))
        .unwrap();
    assert_eq!(status["running"], true);
    assert_eq!(status["mode"], "recording");
    assert_eq!(status["inspector"]["outputs"]["main"]["buttons"][0], true);
    service.game_stop(&journal).unwrap();
    service.shutdown();
    assert!(provider.state.lock().unwrap().subscriptions.is_empty());
}
#[test]
fn a_companion_origin_cannot_enter_through_synthetic_or_hid_start_paths() {
    let (provider, _) = provider();
    provider.register_definition(definition("left")).unwrap();
    let (profile, assignments, snapshot) = fixture(false);
    let mut router = crate::router::Router::default();
    assert!(router
        .start(profile.clone(), assignments.clone(), fixture(false).2)
        .is_err());
    let mut wrong = assignments.clone();
    wrong[0].source.origin = Origin::Synthetic;
    let prepared = provider.prepare(&profile, &assignments).unwrap();
    assert!(router
        .start_with_sources(
            profile,
            wrong,
            snapshot,
            BTreeMap::new(),
            None,
            Some(prepared)
        )
        .is_err());
    assert!(provider.state.lock().unwrap().subscriptions.is_empty());
}

#[test]
fn native_clock_expires_a_lost_peer_even_if_hub_io_and_frontend_are_stalled() {
    let (provider, clock) = provider();
    provider.register_definition(definition("left")).unwrap();
    provider.register_definition(right_definition()).unwrap();
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    provider
        .baseline_native(&token("right", 1), 0, &right_baseline())
        .unwrap();
    let (profile, assignments, snapshot) = fixture(true);
    let prepared = provider.prepare(&profile, &assignments).unwrap();
    let mut router = crate::router::Router::default();
    router
        .start_with_sources(
            profile,
            assignments,
            snapshot,
            BTreeMap::new(),
            None,
            Some(prepared),
        )
        .unwrap();
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[change("trigger", Value::Button(true))],
        )
        .unwrap();
    provider
        .changes_native(
            &token("right", 1),
            1,
            &[change("trigger", Value::Button(true))],
        )
        .unwrap();
    clock.store(101, Ordering::Release);
    router.pump(page(101)).unwrap();
    clock.store(100 + LEASE_US - 1, Ordering::Release);
    provider.heartbeat_native(&token("right", 1)).unwrap();
    clock.store(100 + LEASE_US, Ordering::Release);
    router.pump(page(100 + LEASE_US)).unwrap();
    assert_eq!(
        provider.list().unwrap()["companionSources"][0]["online"],
        false
    );
    assert_eq!(
        provider.list().unwrap()["companionSources"][1]["online"],
        true
    );
    assert_eq!(router.status()["running"], true);
    assert_eq!(
        router.runtime.engine().unwrap().inspect(0)["outputs"]["main"]["buttons"][0],
        true
    );
    assert!(provider.heartbeat_native(&token("left", 1)).is_err());
    assert!(provider
        .changes_native(
            &token("left", 1),
            2,
            &[change("trigger", Value::Button(true))]
        )
        .is_err());
    provider
        .baseline_native(&token("left", 2), 0, &baseline())
        .unwrap();
    router.pump(page(100 + LEASE_US)).unwrap();
    assert_eq!(router.status()["ready"], true);
}
#[test]
fn healthy_held_input_renews_without_fabricated_edges_and_late_heartbeats_cannot_rearm() {
    use osg_companion_service::{SourceSink, SourceToken};
    let (provider, clock) = provider();
    provider.register_definition(definition("left")).unwrap();
    provider
        .baseline_native(&token("left", 1), 0, &baseline())
        .unwrap();
    let (profile, assignments, _) = fixture(false);
    let mut feed = provider.prepare(&profile, &assignments).unwrap().feed;
    feed.initial().unwrap();
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[change("trigger", Value::Button(true))],
        )
        .unwrap();
    feed.drain();
    let token = SourceToken {
        source_id: "left".into(),
        session_id: "session-left".into(),
        generation: 1,
        publication_hash: hash(),
    };
    for index in 1..=8 {
        clock.store(100 + index * (LEASE_US - 1), Ordering::Release);
        provider.heartbeat(&token).unwrap();
        assert!(feed.drain().is_empty());
    }
    clock.store(100 + 8 * (LEASE_US - 1) + LEASE_US, Ordering::Release);
    assert!(provider.heartbeat(&token).is_err());
    assert_eq!(
        feed.drain()[0].reason,
        osg_mapping::engine::Reason::Disconnect
    );
}

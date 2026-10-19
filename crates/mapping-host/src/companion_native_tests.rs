use super::*;
use osg_mapping::engine::Reason;

fn definition(id: &str) -> Definition {
    Definition {
        source_id: id.into(),
        name: "Native companion".into(),
        publication_hash: format!("sha256:{}", "a".repeat(64)),
        controls: vec![ControlDefinition {
            id: "button".into(),
            kind: Kind::Button,
            fallback: Value::Button(false),
        }],
    }
}
fn selection(id: &str) -> NativeSelection {
    NativeSelection {
        definition: definition(id),
        controls: vec!["button".into()],
    }
}
fn token(id: &str, generation: u64) -> InputToken {
    InputToken {
        source_id: id.into(),
        session_id: format!("session-{id}"),
        generation,
        publication_hash: definition(id).publication_hash,
    }
}

#[test]
fn source_admission_wakes_only_after_committed_state_and_provider_unlock() {
    use osg_companion_service::{InputValue, SourceChange, SourceSink, SourceToken};
    use std::sync::{OnceLock, Weak};
    let state = Arc::new(OnceLock::<Weak<Mutex<State>>>::new());
    let observed = Arc::new(Mutex::new(vec![]));
    let callback_state = state.clone();
    let callback_observed = observed.clone();
    let provider = CompanionProvider::with_wake(Arc::new(|| 100), Arc::new(move || {
        let state = callback_state.get().unwrap().upgrade().unwrap();
        let state = state.try_lock().expect("wake must never run while the provider mutex is held");
        let instance = &state.instances["left"];
        let queue = state.subscriptions.values().next().unwrap().sources.get("left").unwrap();
        callback_observed.lock().unwrap().push((instance.wire_sequence, instance.owner.is_some(),
            queue.frames.len(), queue.loss.is_some()));
    }));
    state.set(Arc::downgrade(&provider.state)).unwrap();
    provider.register_definition(definition("left")).unwrap();
    let mut feed = provider.subscribe_native(&[selection("left")]).unwrap();
    let native = token("left", 1);
    let token = SourceToken { source_id: native.source_id, session_id: native.session_id,
        generation: native.generation, publication_hash: native.publication_hash };
    provider.baseline(&token, 0, &[SourceChange { control_id: "button".into(), value: InputValue::Button(false) }]).unwrap();
    feed.baseline().unwrap();
    provider.changes(&token, 1, &[SourceChange { control_id: "button".into(), value: InputValue::Button(true) }]).unwrap();
    provider.heartbeat(&token).unwrap();
    provider.loss(&token).unwrap();
    assert_eq!(*observed.lock().unwrap(), [(0, true, 0, false), (1, true, 1, false),
        (1, true, 1, false), (1, false, 0, true)]);
    assert_eq!(feed.poll().unwrap()[0].reason, Reason::Disconnect);
}
fn baseline(provider: &CompanionProvider, id: &str, generation: u64) {
    provider
        .baseline_native(
            &token(id, generation),
            0,
            &[Change {
                control: "button".into(),
                value: Value::Button(false),
            }],
        )
        .unwrap();
}

#[test]
fn exact_native_subscription_requires_real_baseline_and_discards_prestart_transitions() {
    let provider = CompanionProvider::new(Arc::new(|| 100));
    provider.register_definition(definition("left")).unwrap();
    let mut feed = provider.subscribe_native(&[selection("left")]).unwrap();
    assert!(feed.baseline().is_err());
    baseline(&provider, "left", 1);
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[
                Change {
                    control: "button".into(),
                    value: Value::Button(true),
                },
                Change {
                    control: "button".into(),
                    value: Value::Button(false),
                },
            ],
        )
        .unwrap();
    let fresh = feed.baseline().unwrap();
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].reason, Reason::Baseline);
    assert_eq!(fresh[0].changes[0].value, Value::Button(false));
    assert!(feed.poll().unwrap().is_empty());
    provider
        .changes_native(
            &token("left", 1),
            2,
            &[Change {
                control: "button".into(),
                value: Value::Button(true),
            }],
        )
        .unwrap();
    assert!(feed.validate_baseline().is_err());
    assert!(feed.baseline().is_err());
}

#[test]
fn loss_and_reconnect_are_native_generation_fenced_and_peer_isolated() {
    let provider = CompanionProvider::new(Arc::new(|| 100));
    for id in ["left", "right"] {
        provider.register_definition(definition(id)).unwrap();
        baseline(&provider, id, 1);
    }
    let mut left = provider.subscribe_native(&[selection("left")]).unwrap();
    let mut right = provider.subscribe_native(&[selection("right")]).unwrap();
    let old = left.baseline().unwrap()[0].generation;
    right.baseline().unwrap();
    provider.loss_native(&token("left", 1)).unwrap();
    assert_eq!(left.poll().unwrap()[0].reason, Reason::Disconnect);
    assert!(right.validate_baseline().is_ok());
    assert!(right.poll().unwrap().is_empty());
    assert!(left.validate_baseline().is_err());
    baseline(&provider, "left", 2);
    let fresh = left.poll().unwrap();
    assert_eq!(fresh[0].reason, Reason::Baseline);
    assert!(fresh[0].generation > old);
    assert!(provider
        .changes_native(
            &token("left", 1),
            1,
            &[Change {
                control: "button".into(),
                value: Value::Button(true)
            }]
        )
        .is_err());
    assert!(right.validate_baseline().is_ok());
}

#[test]
fn diagnostic_envelopes_keep_their_exact_subscription_domain_after_queue_changes() {
    let provider = CompanionProvider::new(Arc::new(|| 100));
    provider.register_definition(definition("left")).unwrap();
    baseline(&provider, "left", 1);
    let mut left = provider.subscribe_native(&[selection("left")]).unwrap();
    let mut right = provider.subscribe_native(&[selection("left")]).unwrap();
    left.baseline().unwrap();
    right.baseline().unwrap();
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[Change {
                control: "button".into(),
                value: Value::Button(true),
            }],
        )
        .unwrap();
    let left_subscription = left.diagnostic_identity().1;
    let right_subscription = right.diagnostic_identity().1;
    assert_ne!(left_subscription, right_subscription);
    // Inject only diagnostic metadata into real queued frames. Source authority,
    // sequencing and values still come from the actual provider path.
    {
        let mut state = provider.state.lock().unwrap();
        for (subscription, domain, frame_id) in [
            (left_subscription, 101, 501),
            (right_subscription, 202, 502),
        ] {
            let queue = state
                .subscriptions
                .get_mut(&subscription)
                .unwrap()
                .sources
                .get_mut("left")
                .unwrap();
            queue.frames.front_mut().unwrap().diagnostic = Some(NativeFrameDiagnostic {
                frame_id,
                provider: 7,
                subscription,
                queue_domain: domain,
                physical_source: 13,
                kind: 0,
            });
            // A current queue-domain lookup would now produce the wrong join.
            queue.audit_domain = 999;
        }
    }
    let a = left.poll_audited().unwrap();
    let b = right.poll_audited().unwrap();
    assert_eq!(a[0].frame.sequence, b[0].frame.sequence);
    assert_eq!(a[0].frame.generation, b[0].frame.generation);
    assert_eq!(a[0].diagnostic.unwrap().queue_domain, 101);
    assert_eq!(b[0].diagnostic.unwrap().queue_domain, 202);
    assert_eq!(a[0].diagnostic.unwrap().subscription, left_subscription);
    assert_eq!(b[0].diagnostic.unwrap().subscription, right_subscription);
    assert_eq!(a[0].diagnostic.unwrap().frame_id, 501);
    assert_eq!(b[0].diagnostic.unwrap().frame_id, 502);
    assert_eq!(a[0].frame.changes[0].value, Value::Button(true));
    assert_eq!(b[0].frame.changes[0].value, Value::Button(true));
}

#[test]
fn diagnostic_frames_preserve_the_existing_portable_source_serialization() {
    let provider = CompanionProvider::new(Arc::new(|| 100));
    provider.register_definition(definition("left")).unwrap();
    baseline(&provider, "left", 1);
    let mut feed = provider.subscribe_native(&[selection("left")]).unwrap();
    let envelope = feed.baseline_audited().unwrap().remove(0);
    let serialized = serde_json::to_value(&envelope.frame).unwrap();
    let plain: Vec<Frame> = feed.baseline().unwrap();
    assert_eq!(serialized, serde_json::to_value(&plain[0]).unwrap());
    assert!(serialized.get("diagnostic").is_none());
    assert_eq!(plain[0].sequence, Counter(1));
    assert_eq!(plain[0].captured_us, Counter(100));
}

#[test]
fn native_subscriptions_are_bounded_and_exact_without_authoring_a_shadow_profile() {
    let provider = CompanionProvider::new(Arc::new(|| 100));
    provider.register_definition(definition("left")).unwrap();
    assert!(provider.subscribe_native(&[]).is_err());
    assert!(provider
        .subscribe_native(&vec![selection("left"); 9])
        .is_err());
    assert!(provider.subscribe_native(&[selection("missing")]).is_err());
    assert!(provider
        .subscribe_native(&[selection("left"), selection("left")])
        .is_err());
    for controls in [
        vec![],
        vec!["unknown".into()],
        vec!["button".into(); 2],
        vec!["button".into(); 129],
    ] {
        assert!(provider
            .subscribe_native(&[NativeSelection {
                controls,
                ..selection("left")
            }])
            .is_err());
    }
    let mut changed = selection("left");
    changed.definition.name = "Changed".into();
    assert!(provider.subscribe_native(&[changed]).is_err());
    let feeds = (0..MAX_SUBSCRIPTIONS)
        .map(|_| provider.subscribe_native(&[selection("left")]).unwrap())
        .collect::<Vec<_>>();
    assert!(provider.subscribe_native(&[selection("left")]).is_err());
    drop(feeds);
    assert!(provider.subscribe_native(&[selection("left")]).is_ok());
}

#[test]
fn published_axis_conversion_matches_actual_registration_and_rejects_invalid_fallback() {
    use osg_companion_service::{
        AxisMode, SourceControl, SourceDefinition, SourceKind, SourceSink,
    };
    let source = SourceDefinition {
        source_id: "source".into(),
        name: "Axes".into(),
        publication_hash: definition("source").publication_hash,
        controls: vec![
            SourceControl {
                id: "bipolar".into(),
                kind: SourceKind::Axis {
                    axis_mode: AxisMode::Bipolar,
                    fallback: -1.0,
                },
            },
            SourceControl {
                id: "unipolar".into(),
                kind: SourceKind::Axis {
                    axis_mode: AxisMode::Unipolar,
                    fallback: 1.0,
                },
            },
            SourceControl {
                id: "hat".into(),
                kind: SourceKind::Hat,
            },
            SourceControl {
                id: "button".into(),
                kind: SourceKind::Button,
            },
        ],
    };
    let converted = native_definition(source.clone()).unwrap();
    assert_eq!(converted.controls[0].fallback, Value::Axis(-32768));
    assert_eq!(converted.controls[1].fallback, Value::Axis(32767));
    let provider = CompanionProvider::new(Arc::new(|| 100));
    provider.register(source.clone()).unwrap();
    assert_eq!(provider.resource_inventory()[0].definition, converted);
    let mut invalid = source.clone();
    invalid.controls[0].kind = SourceKind::Axis {
        axis_mode: AxisMode::Bipolar,
        fallback: f64::NAN,
    };
    assert!(native_definition(invalid).is_err());
    let mut invalid = source;
    invalid.controls[0].id.clear();
    assert!(native_definition(invalid).is_err());
}

#[test]
fn unchanged_state_renews_only_from_actual_native_heartbeat_after_ordered_changes() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let clock = Arc::new(AtomicU64::new(100));
    let captured = clock.clone();
    let provider = CompanionProvider::new(Arc::new(move || captured.load(Ordering::Acquire)));
    provider.register_definition(definition("left")).unwrap();
    baseline(&provider, "left", 1);
    let mut feed = provider.subscribe_native(&[selection("left")]).unwrap();
    feed.baseline().unwrap();
    assert!(feed.poll().unwrap().is_empty());
    clock.store(200, Ordering::Release);
    assert!(
        feed.poll().unwrap().is_empty(),
        "Polling cannot invent a fresh state sample"
    );
    provider.heartbeat_native(&token("left", 1)).unwrap();
    let renewal = feed.poll().unwrap();
    assert_eq!(renewal.len(), 1);
    assert_eq!(renewal[0].reason, Reason::Change);
    assert_eq!(renewal[0].captured_us.0, 200);
    assert_eq!(renewal[0].changes[0].value, Value::Button(false));
    assert!(feed.poll().unwrap().is_empty());
    clock.store(300, Ordering::Release);
    provider
        .changes_native(
            &token("left", 1),
            1,
            &[
                Change {
                    control: "button".into(),
                    value: Value::Button(true),
                },
                Change {
                    control: "button".into(),
                    value: Value::Button(false),
                },
            ],
        )
        .unwrap();
    let frames = feed.poll().unwrap();
    assert_eq!(frames[0].changes[0].value, Value::Button(true));
    assert_eq!(frames[1].changes[0].value, Value::Button(false));
    assert_eq!(frames.last().unwrap().captured_us.0, 300);
    assert!(frames
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    clock.store(300 + LEASE_US, Ordering::Release);
    let loss = feed.poll().unwrap();
    assert_eq!(loss.len(), 1);
    assert_eq!(loss[0].reason, Reason::Disconnect);
    assert!(feed.baseline().is_err());
}

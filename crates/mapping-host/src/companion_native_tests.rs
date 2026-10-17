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

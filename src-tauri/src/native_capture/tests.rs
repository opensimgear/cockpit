use super::*;
use serde_json::json;

fn sampled(input: &str, sequence: u64, at: Instant) -> Arc<Contribution> {
    Arc::new(Contribution {
        input: InputId::Channel(input.into()),
        capture: Stamp {
            producer_id: "native".into(),
            generation: 1,
            sequence,
            byte_offset: None,
            captured_at: at,
        },
        validity: Validity::Sampled {
            valid_until: at + Duration::from_millis(100),
        },
    })
}
fn latched(captured: Instant, observed: Instant, sequence: u64) -> Arc<Contribution> {
    Arc::new(Contribution {
        input: InputId::Byte(10),
        capture: Stamp {
            producer_id: "wire".into(),
            generation: 1,
            sequence: 1,
            byte_offset: Some(5),
            captured_at: captured,
        },
        validity: Validity::Latched(Arc::new(Lease {
            domain_id: "decoder".into(),
            generation: 2,
            observation: Stamp {
                producer_id: "wire".into(),
                generation: 1,
                sequence,
                byte_offset: Some(9),
                captured_at: observed,
            },
            valid_until: observed + Duration::from_millis(100),
        })),
    })
}

#[test]
fn projection_and_scaling_keep_original_values_and_shared_evidence() {
    let at = Instant::now();
    let source = CapturedValue::native(json!(0), vec![sampled("x", 3, at)], at).unwrap();
    let scaled = source.with_value(json!(false)).unwrap();
    assert_eq!(scaled.value(), &json!(false));
    assert!(Arc::ptr_eq(
        source.contributors.as_ref().unwrap(),
        scaled.contributors.as_ref().unwrap()
    ));
    assert_eq!(scaled.contributors().unwrap()[0].capture.sequence, 3);
    assert!(source.is_current(at + Duration::from_millis(99)));
    assert!(!source.is_current(at + Duration::from_millis(100)));
    assert!(!source.is_current(at - Duration::from_nanos(1)));
    assert!(source.with_value(json!([])).is_none());
}

#[test]
fn mixed_age_union_uses_every_dependency_without_relabeling_or_conflicts() {
    let old = Instant::now();
    let now = old + Duration::from_millis(50);
    let x = CapturedValue::native(json!(3), vec![sampled("x", 10, old)], now).unwrap();
    let y = CapturedValue::native(json!(4), vec![sampled("y", 4, now)], now).unwrap();
    let magnitude = CapturedValue::combine(json!(5), &[&x, &y, &x], now).unwrap();
    assert_eq!(magnitude.contributors().unwrap().len(), 2);
    assert_eq!(
        magnitude.contributors().unwrap()[0].capture.captured_at,
        old
    );
    assert_eq!(magnitude.contributors().unwrap()[1].capture.sequence, 4);
    assert!(!magnitude.is_current(old + Duration::from_millis(100)));
    let conflicting = CapturedValue::native(json!(3), vec![sampled("x", 11, now)], now).unwrap();
    assert!(CapturedValue::combine(json!(6), &[&x, &conflicting], now).is_none());
    assert!(
        CapturedValue::combine(json!(6), &[&x, &CapturedValue::fixture(json!(3))], now).is_none()
    );
    assert!(
        CapturedValue::combine(json!(6), &[&CapturedValue::fixture(json!(3))], now)
            .unwrap()
            .contributors()
            .is_none()
    );
    assert!(CapturedValue::combine(json!(0), &[], now).is_none());
}

#[test]
fn old_latched_capture_keeps_its_identity_when_a_real_native_lease_advances() {
    let old = Instant::now();
    let now = old + Duration::from_secs(3600);
    let previous = CapturedValue::native(json!(9), vec![latched(old, now, 2)], now).unwrap();
    let later = now + Duration::from_millis(50);
    let renewed = CapturedValue::native(json!(9), vec![latched(old, later, 3)], later).unwrap();
    let combined = CapturedValue::combine(json!(18), &[&previous, &renewed], later).unwrap();
    let contribution = &combined.contributors().unwrap()[0];
    assert_eq!(contribution.capture.captured_at, old);
    assert_eq!(contribution.capture.sequence, 1);
    let Validity::Latched(lease) = &contribution.validity else {
        panic!()
    };
    assert_eq!(lease.observation.sequence, 3);
    assert!(combined.is_current(now + Duration::from_millis(125)));
    assert!(!combined.is_current(later + Duration::from_millis(100)));
    assert_eq!(
        previous.contributors().unwrap()[0].capture,
        contribution.capture
    );
    let reversed = CapturedValue::combine(json!(18), &[&renewed, &previous], later).unwrap();
    assert_eq!(combined, reversed);
    let mut contradiction = (*latched(old, now, 2)).clone();
    let Validity::Latched(lease) = &mut contradiction.validity else {
        panic!()
    };
    Arc::make_mut(lease).observation.captured_at += Duration::from_millis(1);
    let contradiction =
        CapturedValue::native(json!(9), vec![Arc::new(contradiction)], later).unwrap();
    assert!(CapturedValue::combine(json!(18), &[&previous, &contradiction], later).is_none());
    assert!(CapturedValue::combine(json!(18), &[&contradiction, &previous], later).is_none());
    assert_eq!(renewed.authority_age_ms(later), Some(0));
    assert_eq!(previous.authority_age_ms(later), Some(50));
}

#[test]
fn malformed_and_future_proof_cannot_become_native_values() {
    let now = Instant::now();
    let bad = |change: fn(&mut Contribution)| {
        let mut c = (*sampled("x", 1, now)).clone();
        change(&mut c);
        assert!(CapturedValue::native(json!(1), vec![Arc::new(c)], now).is_none());
    };
    for change in [
        (|c: &mut Contribution| c.capture.sequence = 0) as fn(&mut Contribution),
        |c| c.capture.generation = 0,
        |c| c.capture.producer_id = "".into(),
        |c| c.input = InputId::Channel("\n".into()),
        |c| c.input = InputId::Byte(10),
        |c| c.capture.byte_offset = Some(1),
        |c| c.capture.captured_at += Duration::from_millis(1),
        |c| {
            c.validity = Validity::Sampled {
                valid_until: c.capture.captured_at,
            }
        },
        |c| {
            c.validity = Validity::Sampled {
                valid_until: c.capture.captured_at + Duration::from_secs(12),
            }
        },
    ] {
        bad(change);
    }
    assert!(CapturedValue::native(json!(null), vec![sampled("x", 1, now)], now).is_none());
    assert!(CapturedValue::native(
        json!("x".repeat(MAX_TEXT + 1)),
        vec![sampled("x", 1, now)],
        now
    )
    .is_none());
    assert!(CapturedValue::native(json!(1), vec![], now).is_none());
    assert!(CapturedValue::native(
        json!(1),
        vec![sampled("x", 1, now), sampled("x", 1, now)],
        now
    )
    .is_none());
    let mut c = (*latched(now, now, 2)).clone();
    let Validity::Latched(lease) = &mut c.validity else {
        panic!()
    };
    Arc::make_mut(lease).observation.producer_id = "foreign".into();
    assert!(CapturedValue::native(json!(1), vec![Arc::new(c)], now).is_none());
}

#[test]
fn reference_and_heap_budgets_count_shared_storage_in_full() {
    let now = Instant::now();
    let inputs = (0..MAX_CONTRIBUTORS)
        .map(|i| sampled(&format!("x{i}"), i as u64 + 1, now))
        .collect::<Vec<_>>();
    let value = CapturedValue::native(json!("test"), inputs.clone(), now).unwrap();
    assert!(value.retained_bytes() > MAX_CONTRIBUTORS * std::mem::size_of::<Contribution>());
    assert_eq!(value.retained_bytes(), value.clone().retained_bytes());
    let mut too_many = inputs;
    too_many.push(sampled("extra", 2000, now));
    assert!(CapturedValue::native(json!(1), too_many, now).is_none());
    let mut budget = ProjectionBudget::default();
    for _ in 0..MAX_PROJECTION_REFERENCES / MAX_CONTRIBUTORS {
        assert!(budget.admit(&value));
    }
    assert!(!budget.admit(&value));
    assert!(budget.admit(&CapturedValue::fixture(json!(0))));
    let mut text = String::with_capacity(128);
    text.push('x');
    assert!(CapturedValue::fixture(Value::String(text)).retained_bytes() >= 128);
    assert!(
        CapturedValue::fixture(json!({"a":["abc",0]})).retained_bytes()
            > std::mem::size_of::<CapturedValue>()
    );
}

use super::*;
use crate::native_capture::{Contribution, InputId, Stamp, Validity};
use std::{sync::Arc, time::Duration};
fn value(id: &str, n: f64, at: Instant) -> CapturedValue {
    CapturedValue::native(
        json!(n),
        vec![Arc::new(Contribution {
            input: InputId::Channel(id.into()),
            capture: Stamp {
                producer_id: "source".into(),
                generation: 1,
                sequence: 1,
                byte_offset: None,
                captured_at: at,
            },
            validity: Validity::Sampled {
                valid_until: at + Duration::from_millis(100),
            },
        })],
        at,
    )
    .unwrap()
}
#[test]
fn canonical_scale_and_magnitude_preserve_actual_mixed_age_dependencies() {
    let old = Instant::now();
    let now = old + Duration::from_millis(50);
    let native = CapturedValues::from([
        ("speed_kmh".into(), value("physics.speedKmh", 72., old)),
        ("physics.accG.0".into(), value("physics.accG.0", 0., old)),
        ("physics.accG.1".into(), value("physics.accG.1", 3., now)),
        ("physics.accG.2".into(), value("physics.accG.2", 4., now)),
    ]);
    let canonical = captured_values("assetto-corsa", &native, now);
    assert_eq!(canonical["vehicle.speed_mps"].value(), &json!(20.));
    assert_eq!(
        canonical["vehicle.speed_mps"].contributors(),
        native["speed_kmh"].contributors()
    );
    assert_eq!(canonical["vehicle.g_force.magnitude"].value(), &json!(5.));
    assert_eq!(
        canonical["vehicle.g_force.magnitude"]
            .contributors()
            .unwrap()
            .len(),
        3
    );
    assert!(!canonical["vehicle.g_force.magnitude"].is_current(old + Duration::from_millis(100)));
    assert!(
        captured_values("assetto-corsa", &native, old + Duration::from_millis(100))
            .get("vehicle.g_force.magnitude")
            .is_none()
    );
}
#[test]
fn selected_rows_drop_missing_invalid_or_conflicting_derived_values() {
    let now = Instant::now();
    let rows = vec![
        json!({"id":"s","channel":"vehicle.speed_mps"}),
        json!({"id":"g","channel":"vehicle.g_force.magnitude"}),
    ];
    let mut native = CapturedValues::from([
        ("s".into(), value("speed_kmh", 72., now)),
        ("g/g0".into(), value("x", 0., now)),
        ("g/g1".into(), value("y", 3., now)),
        ("g/g2".into(), value("z", 4., now)),
    ]);
    captured_project_rows("assetto-corsa", &rows, &mut native, now);
    assert_eq!(native["s"].value(), &json!(20.));
    assert_eq!(native["g"].value(), &json!(5.));
    native.remove("g/g2");
    captured_project_rows("assetto-corsa", &rows[1..], &mut native, now);
    assert!(!native.contains_key("g"));
    native.insert("s".into(), CapturedValue::fixture(json!("invalid")));
    captured_project_rows("assetto-corsa", &rows[..1], &mut native, now);
    assert!(!native.contains_key("s"));
    native.insert("g/g2".into(), CapturedValue::fixture(json!(4)));
    captured_project_rows("assetto-corsa", &rows[1..], &mut native, now);
    assert!(!native.contains_key("g"));
}

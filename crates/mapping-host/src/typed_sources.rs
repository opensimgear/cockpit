//! Scalar decoding for the contracts advertised by the unified inventory.
//!
//! This layer owns no capture subscription, clock or authority. Its callers must
//! supply a current provider sample; missing/stale state stays in the typed
//! engine's validity envelope instead of being passed here as a fabricated value.
use osg_controller_input::registry::{Control, Kind, Sample};
use osg_mapping::signal::PortValue;

/// Decode a host value in its advertised domain. Axes retain the native HID
/// range. A profile must contain an explicit range-mapping node to normalize it.
/// Buttons and hats use the provider's canonical boolean/-1..7 wire contract.
pub fn controller_value(control: &Control, sample: &Sample) -> Result<PortValue, String> {
    if control.bits == 0
        || control.bits > 32
        || control.min > control.max
        || control.min.unsigned_abs() > 9_007_199_254_740_991
        || control.max.unsigned_abs() > 9_007_199_254_740_991
    {
        return Err("The captured control has invalid native bounds.".into());
    }
    let in_range = (control.min..=control.max).contains(&sample.native);
    match control.kind {
        Kind::Button if in_range => Ok(PortValue::Boolean(sample.native != 0)),
        Kind::Axis
            if in_range
                && control.max > control.min
                && sample.native.unsigned_abs() <= 9_007_199_254_740_991 =>
        {
            Ok(PortValue::Number(sample.native as f64))
        }
        Kind::Hat if !in_range && control.has_null => Ok(PortValue::Number(-1.0)),
        Kind::Hat if in_range && [4, 8].contains(&(control.max - control.min + 1)) => {
            Ok(PortValue::Number(
                ((sample.native - control.min) * (8 / (control.max - control.min + 1))) as f64,
            ))
        }
        _ => Err("The captured value does not match its advertised physical port.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(kind: Kind, min: i64, max: i64) -> Control {
        let mut control = Control::new(1, 0x30, 0, 0, 0);
        control.kind = kind;
        control.min = min;
        control.max = max;
        control.bits = 16;
        control
    }

    #[test]
    fn every_native_axis_value_remains_raw_until_an_explicit_graph_conversion() {
        let axis = control(Kind::Axis, 0, 4095);
        for raw in 0..=4095 {
            let sample = axis.decode(raw).unwrap();
            assert_eq!(
                controller_value(&axis, &sample).unwrap(),
                PortValue::Number(raw as f64)
            );
        }
        let signed = control(Kind::Axis, -2048, 2047);
        let sample = signed.decode(0xffff).unwrap();
        assert_eq!(sample.native, -1);
        assert_ne!(sample.normalized, Some(-1));
        assert_eq!(
            controller_value(&signed, &sample).unwrap(),
            PortValue::Number(-1.0)
        );
    }

    #[test]
    fn provider_hat_and_button_decoding_matches_the_advertised_canonical_contract() {
        let button = control(Kind::Button, 0, 1);
        for (raw, expected) in [(0, false), (1, true)] {
            assert_eq!(
                controller_value(&button, &button.decode(raw).unwrap()).unwrap(),
                PortValue::Boolean(expected)
            );
        }
        for count in [4, 8] {
            let mut hat = control(Kind::Hat, 1, count);
            hat.has_null = true;
            assert_eq!(
                controller_value(&hat, &hat.decode(0).unwrap()).unwrap(),
                PortValue::Number(-1.0)
            );
            for raw in 1..=count as u32 {
                assert_eq!(
                    controller_value(&hat, &hat.decode(raw).unwrap()).unwrap(),
                    PortValue::Number(((raw - 1) * (8 / count as u32)) as f64)
                );
            }
        }
    }

    #[test]
    fn invalid_or_diagnostic_capture_never_becomes_a_known_zero_or_release() {
        let mut axis = control(Kind::Axis, 0, 100);
        for sample in [
            Sample {
                native: -1,
                normalized: Some(0),
            },
            Sample {
                native: 101,
                normalized: Some(0),
            },
        ] {
            assert!(controller_value(&axis, &sample).is_err());
        }
        axis.bits = 0;
        assert!(controller_value(
            &axis,
            &Sample {
                native: 0,
                normalized: Some(0)
            }
        )
        .is_err());
        let diagnostic = control(Kind::Diagnostic, 0, 100);
        assert!(controller_value(
            &diagnostic,
            &Sample {
                native: 0,
                normalized: None
            }
        )
        .is_err());
        let hat = control(Kind::Hat, 0, 5);
        assert!(controller_value(
            &hat,
            &Sample {
                native: 0,
                normalized: Some(0)
            }
        )
        .is_err());
        let impossible = control(Kind::Hat, i64::MIN, i64::MAX);
        assert!(controller_value(
            &impossible,
            &Sample {
                native: 0,
                normalized: Some(0)
            }
        )
        .is_err());
    }
}

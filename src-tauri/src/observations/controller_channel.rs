//! Compact repeat snapshots only on the native WebView transport.
use super::Delivery;
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct ControllerChannel {
    baseline: Option<(String, Value)>,
}

impl ControllerChannel {
    pub(super) fn encode(&mut self, mut delivery: Delivery) -> Delivery {
        let frame = &mut delivery.frame;
        if frame.source != "controller-input" || frame.kind != "snapshot" {
            self.baseline = None;
            return delivery;
        }
        let Some(payload) = frame.payload.as_ref() else {
            self.baseline = None;
            return delivery;
        };
        if let Some((generation, baseline)) = &self.baseline {
            if generation == &frame.generation {
                if let Some(compact) = compact(baseline, payload) {
                    frame.payload = Some(compact);
                    return delivery;
                }
            }
        }
        self.baseline = Some((frame.generation.clone(), payload.clone()));
        delivery
    }
}

fn compact(baseline: &Value, payload: &Value) -> Option<Value> {
    let current = payload.as_object()?;
    let previous = baseline.as_object()?;
    if current.len() != previous.len() || current.iter().any(|(key, value)| {
        !matches!(key.as_str(), "source" | "controls" | "cursor" | "capturedUs")
            && previous.get(key) != Some(value)
    }) {
        return None;
    }
    let controls = current.get("controls")?.as_array()?;
    let descriptors = previous.get("controls")?.as_array()?;
    if controls.len() != descriptors.len() || controls.len() > 256 {
        return None;
    }
    let values = controls.iter().zip(descriptors).map(|(row, descriptor)| {
        if row.as_object()?.len() != 2 || row.get("capability")? != descriptor.get("capability")? {
            return None;
        }
        let sample = row.get("value")?;
        if sample.is_null() {
            return Some(Value::Null);
        }
        if sample.as_object()?.len() != 2 {
            return None;
        }
        Some(json!([sample.get("native")?, sample.get("normalized")?]))
    }).collect::<Option<Vec<_>>>()?;
    Some(json!({"controllerSamples": values, "source": current.get("source")?,
        "cursor": current.get("cursor")?, "capturedUs": current.get("capturedUs")?}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(kind: &str, generation: &str, descriptor: &str, native: u32) -> Delivery {
        Delivery {
            subscription_id: "reader".into(),
            delivery: "1".into(),
            frame: osg_streams::WireFrame {
                kind: kind.into(),
                topic: "controllers.input.test".into(),
                source: "controller-input".into(),
                session: "stream".into(),
                generation: generation.into(),
                revision: "1".into(),
                sequence: None,
                captured_us: "17".into(),
                clock_id: "controllers:registry".into(),
                continuity: "baseline".into(),
                reason: None,
                payload: Some(json!({"session":"registry","cursor":"9","capturedUs":"17",
                    "source":{"sequence":"9"},"controls":[{"capability":{"id":descriptor},
                    "value":{"native":native,"normalized":null}}]})),
            },
        }
    }

    #[test]
    fn compact_updates_are_absolute_and_full_baselines_follow_boundaries() {
        let mut channel = ControllerChannel::default();
        let encode = |channel: &mut ControllerChannel, kind, generation, descriptor, native| {
            channel.encode(report(kind, generation, descriptor, native)).frame.payload.unwrap()
        };
        assert!(encode(&mut channel, "snapshot", "1", "axis", 0).get("controls").is_some());
        let repeat = encode(&mut channel, "snapshot", "1", "axis", 32768);
        assert_eq!(repeat["controllerSamples"], json!([[32768, null]]));
        assert_eq!(repeat["capturedUs"], "17");
        assert!(encode(&mut channel, "snapshot", "1", "other", 5).get("controls").is_some());
        assert!(encode(&mut channel, "snapshot", "2", "other", 6).get("controls").is_some());
        assert!(encode(&mut channel, "loss", "2", "other", 0).get("controllerSamples").is_none());
        assert!(encode(&mut channel, "snapshot", "2", "other", 7).get("controls").is_some());
    }
}

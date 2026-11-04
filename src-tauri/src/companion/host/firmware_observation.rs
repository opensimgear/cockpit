use super::Runtime;
use crate::observations;
use osg_streams::TopicSpec;
use serde_json::Value;
use std::sync::Arc;

const TOPIC: &str = "companion.firmware";

impl Runtime {
    pub(crate) fn observe_firmware(&self, runtime: &observations::Runtime) -> Result<(), String> {
        let publisher = runtime.broker().register(
            TopicSpec {
                topic: TOPIC.into(),
                source: "companion-serial".into(),
                audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
            },
            Value::Null, runtime.captured_us(), TOPIC.into(),
        ).map_err(|error| error.to_string())?;
        let runtime = runtime.clone();
        self.firmware_observation.set(Arc::new(move |state| {
            let _ = publisher.publish_latest_state(
                publisher.generation(), state.clone(), runtime.captured_us(), TOPIC,
            );
        })).map_err(|_| "Companion firmware observation is already installed.".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use osg_companion_serial::{FirmwareTarget, OwnedFirmwareSegment};
    use std::time::{Duration, Instant};

    #[test]
    fn firmware_failure_is_retained_for_passive_readers() {
        let runtime = Runtime::new(None, Arc::new(super::super::CompanionProvider::new(Arc::new(|| 1))));
        let services = runtime.services().unwrap();
        let observations = observations::Runtime::new();
        runtime.observe_firmware(&observations).unwrap();
        let broker = observations.broker();
        let mut reader = broker.subscribe("firmware-test", "mcp", TOPIC).unwrap();
        assert_eq!(reader.try_recv().unwrap().unwrap().payload, Some(Value::Null));
        let port = format!("missing-firmware-fixture-{}", uuid::Uuid::new_v4());
        let firmware = [(0, 0xe9), (0x8000, 0), (0x10000, 0xe9)]
            .map(|(offset, byte)| OwnedFirmwareSegment { offset, bytes: vec![byte] });
        services.usb.flash_owned_options_preserving(
            &port, firmware.into(), FirmwareTarget::Esp32s3, false, false,
        ).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(state) = reader.try_recv().unwrap().and_then(|frame| frame.payload) {
                if state["flashError"].is_string() {
                    assert_eq!(state["flashing"], false);
                    assert!(state["flashId"].is_string());
                    let mut late = broker.subscribe("firmware-late", "ui", TOPIC).unwrap();
                    assert_eq!(late.try_recv().unwrap().unwrap().payload, Some(state));
                    break;
                }
            }
            assert!(Instant::now() < deadline, "Firmware worker did not publish its terminal state");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

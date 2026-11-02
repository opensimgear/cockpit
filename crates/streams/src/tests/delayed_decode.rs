use super::*;
use serde::{de::Error as _, ser::Serializer, Deserialize, Serialize};
use std::sync::{Arc, Barrier, Mutex, OnceLock};

type DecodeGates = (Arc<Barrier>, Arc<Barrier>);
static DECODE_GATES: OnceLock<Mutex<Option<DecodeGates>>> = OnceLock::new();

fn gates() -> &'static Mutex<Option<DecodeGates>> {
    DECODE_GATES.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Copy)]
enum Boundary {
    ReplaceGeneration,
    Resynchronize,
    Close,
    Overflow,
}

fn delayed_decode_during(boundary: Boundary) {
    let started = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    *gates().lock().unwrap() = Some((started.clone(), resume.clone()));

    let journal: Stream<u64, FailsOnDecode> =
        Stream::with_limits(1, 0, 1, "test-clock", super::limits()).unwrap();
    let mut subscription = journal.subscribe().unwrap();
    subscription.try_recv().unwrap();
    journal
        .publish(1, 1, FailsOnDecode, 2, "test-clock")
        .unwrap();

    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let result = subscription.try_recv();
        send.send((result, subscription)).unwrap();
    });
    started.wait();
    match boundary {
        Boundary::ReplaceGeneration => {
            journal.replace_generation(2, 10, 3, "test-clock").unwrap();
        }
        Boundary::Resynchronize => {
            journal.resynchronize(1, 10, 3, "test-clock").unwrap();
        }
        Boundary::Close => journal.close(),
        Boundary::Overflow => {
            for value in 2..=4 {
                journal
                    .publish(1, value, FailsOnDecode, value + 2, "test-clock")
                    .unwrap();
            }
        }
    }
    resume.wait();

    let (result, mut subscription) = receive.recv().unwrap();
    assert!(matches!(result, Err(StreamError::Serialization(_))));
    reader.join().unwrap();
    let loss = subscription.try_recv().unwrap().unwrap();
    let Delivery::Loss(loss) = loss else {
        panic!("decode completion must preserve the stronger continuity loss");
    };
    match boundary {
        Boundary::ReplaceGeneration => {
            assert_eq!(loss.reason, LossReason::GenerationReplaced);
            assert!(matches!(
                subscription.try_recv().unwrap(),
                Some(Delivery::Snapshot(Snapshot {
                    value: 10,
                    generation: 2,
                    ..
                }))
            ));
        }
        Boundary::Resynchronize => {
            assert_eq!(loss.reason, LossReason::BaselineReset);
            assert!(matches!(
                subscription.try_recv().unwrap(),
                Some(Delivery::Snapshot(Snapshot {
                    value: 10,
                    generation: 1,
                    ..
                }))
            ));
        }
        Boundary::Close => assert_eq!(loss.reason, LossReason::Closed),
        Boundary::Overflow => assert_eq!(loss.reason, LossReason::Overflow),
    }
    assert_eq!(subscription.try_recv().unwrap(), None);
    *gates().lock().unwrap() = None;
}

#[test]
fn delayed_decode_failure_cannot_erase_new_generation_or_resync_baseline() {
    delayed_decode_during(Boundary::ReplaceGeneration);
    delayed_decode_during(Boundary::Resynchronize);
    delayed_decode_during(Boundary::Close);
    delayed_decode_during(Boundary::Overflow);
}

#[derive(Debug, PartialEq, Eq)]
struct FailsOnDecode;

impl Serialize for FailsOnDecode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("valid-json-but-invalid-domain-value")
    }
}

impl<'de> Deserialize<'de> for FailsOnDecode {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let current = gates().lock().unwrap().clone().unwrap();
        current.0.wait();
        current.1.wait();
        Err(D::Error::custom("deliberate decode failure"))
    }
}

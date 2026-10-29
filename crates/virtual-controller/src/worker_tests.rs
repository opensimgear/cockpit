use super::*;
use crate::{
    feeder::Message,
    vjoy::{Binding, Driver, Position, Slot, Status},
};
use osg_mapping::engine::{Counter, RunToken};
use std::sync::{Arc, Mutex};
#[derive(Clone, Default)]
struct DriverFixture(Arc<Mutex<Vec<String>>>);
impl Driver for DriverFixture {
    fn check(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn inspect(&mut self, id: u8) -> Result<Slot, String> {
        Ok(Slot {
            id,
            status: Status::Free,
            owner_pid: None,
            buttons: 1,
            axes: vec![],
            continuous_hats: 0,
            discrete_hats: 0,
            force_feedback: false,
        })
    }
    fn acquire(&mut self, _: u8) -> Result<(), String> {
        self.0.lock().unwrap().push("acquire".into());
        Ok(())
    }
    fn submit(&mut self, _: u8, _: &Position) -> Result<(), String> {
        self.0.lock().unwrap().push("submit".into());
        Ok(())
    }
    fn release(&mut self, _: u8) {
        self.0.lock().unwrap().push("release".into());
    }
}
#[derive(Clone, Default)]
struct SharedOutput(Arc<Mutex<Vec<u8>>>);
impl Write for SharedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn messages() -> Vec<u8> {
    let token = RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    };
    let messages = [
        Message::Start {
            token: token.clone(),
            bindings: vec![Binding {
                slot: 1,
                target: "main".into(),
                axes: vec![],
                layout: osg_mapping::profile::Layout {
                    buttons: 1,
                    axes: vec![],
                    hats: 0,
                },
            }],
        },
        Message::Renew {
            token: token.clone(),
            sequence: Counter(1),
        },
        Message::Stop { token },
    ];
    messages
        .iter()
        .flat_map(|m| {
            let mut b = serde_json::to_vec(m).unwrap();
            b.push(b'\n');
            b
        })
        .collect()
}
#[test]
fn worker_uses_production_feeder_and_releases_on_eof_and_malformed_input() {
    let driver = DriverFixture::default();
    let output = SharedOutput::default();
    serve(
        driver.clone(),
        io::Cursor::new(messages()),
        output.clone(),
        "fixture",
    )
    .unwrap();
    let replies: Vec<serde_json::Value> = String::from_utf8(output.0.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[0]["backend"], "fixture");
    assert_eq!(replies[3]["active"], false);
    assert_eq!(driver.0.lock().unwrap().last().unwrap(), "release");
    let driver = DriverFixture::default();
    let data = messages();
    let first = data.iter().position(|b| *b == b'\n').unwrap() + 1;
    serve(
        driver.clone(),
        io::Cursor::new(data[..first].to_vec()),
        Vec::new(),
        "fixture",
    )
    .unwrap();
    assert_eq!(driver.0.lock().unwrap().last().unwrap(), "release");
    assert!(serve(
        DriverFixture::default(),
        io::Cursor::new(b"{bad}\n".to_vec()),
        Vec::new(),
        "fixture"
    )
    .is_err());
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(serve(
        DriverFixture::default(),
        io::Cursor::new(messages()),
        Broken,
        "fixture"
    )
    .is_err());
}
#[test]
fn slow_native_acquisition_does_not_consume_the_first_report_lease() {
    struct SlowAcquisition(DriverFixture);
    impl Driver for SlowAcquisition {
        fn check(&mut self) -> Result<(), String> {
            self.0.check()
        }
        fn inspect(&mut self, id: u8) -> Result<Slot, String> {
            self.0.inspect(id)
        }
        fn acquire(&mut self, id: u8) -> Result<(), String> {
            thread::sleep(Duration::from_millis(300));
            self.0.acquire(id)
        }
        fn submit(&mut self, id: u8, position: &Position) -> Result<(), String> {
            self.0.submit(id, position)
        }
        fn release(&mut self, id: u8) {
            self.0.release(id);
        }
    }
    let driver = DriverFixture::default();
    let output = SharedOutput::default();
    serve(
        SlowAcquisition(driver.clone()),
        io::Cursor::new(messages()),
        output.clone(),
        "fixture",
    )
    .unwrap();
    let responses: Vec<serde_json::Value> = output
        .0
        .lock()
        .unwrap()
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(|b| serde_json::from_slice(b).unwrap())
        .collect();
    assert_eq!(responses.len(), 4);
    assert!(responses.iter().all(|r| r["ok"] == true));
    assert_eq!(responses[3]["active"], false);
    assert_eq!(driver.0.lock().unwrap().last().unwrap(), "release");
}

#[test]
fn framing_bounds_precede_parse_and_truncated_frames_fail() {
    let mut at = vec![b' '; FRAME_LIMIT];
    at.push(b'\n');
    assert_eq!(
        read_frame(&mut io::BufReader::with_capacity(7, io::Cursor::new(at)))
            .unwrap()
            .unwrap()
            .len(),
        FRAME_LIMIT
    );
    assert!(read_frame(&mut io::Cursor::new(vec![0; FRAME_LIMIT + 1])).is_err());
    assert!(read_frame(&mut io::Cursor::new(b"{}")).is_err());
    assert!(read_frame(&mut io::Cursor::new([])).unwrap().is_none());
}

#[test]
fn lease_expiry_releases_outputs_even_when_both_pipes_are_stalled() {
    use std::sync::{mpsc, Condvar};
    struct ChannelInput {
        receiver: mpsc::Receiver<Vec<u8>>,
        pending: io::Cursor<Vec<u8>>,
    }
    impl io::Read for ChannelInput {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if self.pending.position() as usize == self.pending.get_ref().len() {
                match self.receiver.recv() {
                    Ok(next) => self.pending = io::Cursor::new(next),
                    Err(_) => return Ok(0),
                }
            }
            io::Read::read(&mut self.pending, bytes)
        }
    }
    struct StalledOutput(Arc<(Mutex<bool>, Condvar)>);
    impl Write for StalledOutput {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let (mutex, condition) = &*self.0;
            let guard = mutex.lock().unwrap();
            let _guard = condition.wait_while(guard, |released| !*released).unwrap();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let data = messages();
    let first = data.iter().position(|b| *b == b'\n').unwrap() + 1;
    let (input, receiver) = mpsc::channel();
    input.send(data[..first].to_vec()).unwrap();
    let blocked = Arc::new((Mutex::new(false), Condvar::new()));
    let output = StalledOutput(blocked.clone());
    let driver = DriverFixture::default();
    let owned = driver.clone();
    let (result, completion) = mpsc::channel();
    let task = thread::spawn(move || {
        let value = serve(
            owned,
            io::BufReader::new(ChannelInput {
                receiver,
                pending: io::Cursor::new(vec![]),
            }),
            output,
            "fixture",
        );
        let _ = result.send(value);
    });
    let completed = completion.recv_timeout(Duration::from_secs(2));
    // Unblock detached I/O threads even if the assertion below fails.
    drop(input);
    *blocked.0.lock().unwrap() = true;
    blocked.1.notify_all();
    let error = completed
        .expect("The lease must not wait for stdin or stdout")
        .unwrap_err();
    assert!(error.contains("heartbeat expired"));
    let calls = driver.0.lock().unwrap();
    assert!(calls.iter().any(|c| c == "acquire"));
    assert_eq!(calls.last().unwrap(), "release");
    drop(calls);
    task.join().unwrap();
}

#[test]
fn lease_expiry_forwards_its_cause_only_after_owned_output_is_released() {
    struct DelayedEof(io::Cursor<Vec<u8>>);
    impl io::Read for DelayedEof {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let count = io::Read::read(&mut self.0, bytes)?;
            if count == 0 {
                thread::sleep(Duration::from_millis(500));
            }
            Ok(count)
        }
    }
    struct CheckedOutput(SharedOutput, DriverFixture);
    impl Write for CheckedOutput {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            // serde's string formatting can write a frame in several chunks.
            let count = self.0.write(bytes)?;
            let output = self.0 .0.lock().unwrap();
            if output
                .windows(b"heartbeat expired".len())
                .any(|s| s == b"heartbeat expired")
            {
                assert_eq!(self.1 .0.lock().unwrap().last().unwrap(), "release");
            }
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let data = messages();
    let first = data.iter().position(|b| *b == b'\n').unwrap() + 1;
    let driver = DriverFixture::default();
    let output = SharedOutput::default();
    let error = serve(
        driver.clone(),
        io::BufReader::new(DelayedEof(io::Cursor::new(data[..first].to_vec()))),
        CheckedOutput(output.clone(), driver),
        "fixture",
    )
    .unwrap_err();
    assert!(error.contains("heartbeat expired"));
    let responses: Vec<serde_json::Value> = output
        .0
        .lock()
        .unwrap()
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(|b| serde_json::from_slice(b).unwrap())
        .collect();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[2]["fatal"], true);
    assert_eq!(responses[2]["active"], false);
    assert_eq!(responses[2]["error"], error);
}

#[test]
fn negative_acknowledgement_is_written_only_after_fallback_and_release() {
    struct CheckedOutput {
        driver: DriverFixture,
        buffer: Vec<u8>,
        rejected: Arc<std::sync::atomic::AtomicBool>,
    }
    impl Write for CheckedOutput {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            for byte in bytes {
                if *byte == b'\n' {
                    let response: serde_json::Value = serde_json::from_slice(&self.buffer).unwrap();
                    if response["ok"] == false {
                        assert_eq!(self.driver.0.lock().unwrap().last().unwrap(), "release");
                        assert_eq!(response["active"], false);
                        self.rejected
                            .store(true, std::sync::atomic::Ordering::Release);
                    }
                    self.buffer.clear();
                } else {
                    self.buffer.push(*byte);
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let data = messages();
    let first = data.iter().position(|b| *b == b'\n').unwrap() + 1;
    let mut bytes = data[..first].to_vec();
    let bad = Message::Renew {
        token: RunToken {
            session: uuid::Uuid::new_v4().to_string(),
            generation: Counter(1),
        },
        sequence: Counter(1),
    };
    bytes.extend(serde_json::to_vec(&bad).unwrap());
    bytes.push(b'\n');
    let driver = DriverFixture::default();
    let rejected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    assert!(serve(
        driver.clone(),
        io::Cursor::new(bytes),
        CheckedOutput {
            driver,
            buffer: vec![],
            rejected: rejected.clone()
        },
        "fixture"
    )
    .is_err());
    assert!(rejected.load(std::sync::atomic::Ordering::Acquire));
}

use crate::native_duplex::{self, CancelIo, DeadlineWriter, DuplexParts};
use osg_streams::NativeWake;
use std::{
    sync::{mpsc, Arc},
    thread,
};
use zeroize::Zeroizing;

type Chunk = Zeroizing<Vec<u8>>;

pub(super) struct ReadPump {
    pub writer: Box<dyn DeadlineWriter>,
    pub baud: u32,
    cancel: Arc<dyn CancelIo>,
    received: Option<mpsc::Receiver<Result<Chunk, String>>>,
    reader: Option<thread::JoinHandle<()>>,
}

impl ReadPump {
    pub fn open(path: &str, baud: u32, wake: Arc<NativeWake>) -> Result<Self, String> {
        Self::start(native_duplex::open(path, baud)?, baud, wake)
    }

    fn start(parts: DuplexParts, baud: u32, wake: Arc<NativeWake>) -> Result<Self, String> {
        let DuplexParts { mut receiver, mut writer, cancel } = parts;
        let _ = writer.control_lines(false, false);
        let (send, received) = mpsc::sync_channel(16);
        let reader = thread::Builder::new().name("companion-serial-rx".into()).spawn(move || {
            let mut buffer = Zeroizing::new([0u8; 512]);
            loop {
                let chunk = match receiver.receive(&mut *buffer) {
                    Ok(0) => Err("USB disconnected.".into()),
                    Ok(count) => Ok(Zeroizing::new(buffer[..count].to_vec())),
                    Err(error) => Err(error),
                };
                let closed = chunk.is_err();
                if send.send(chunk).is_err() { break; }
                wake.notify();
                if closed { break; }
            }
        }).map_err(|error| format!("Cannot start USB receiver: {error}"))?;
        Ok(Self { writer, baud, cancel, received: Some(received), reader: Some(reader) })
    }

    pub fn try_read(&self) -> Result<Option<Chunk>, String> {
        match self.received.as_ref().expect("live receive queue").try_recv() {
            Ok(result) => result.map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err("USB disconnected.".into()),
        }
    }
}

impl Drop for ReadPump {
    fn drop(&mut self) {
        self.cancel.cancel();
        // Release a reader blocked by a full queue before joining it.
        drop(self.received.take());
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_duplex::BlockingReceiver;
    use std::time::{Duration, Instant};

    struct Receiver(mpsc::Receiver<Vec<u8>>);
    impl BlockingReceiver for Receiver {
        fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
            let bytes = self.0.recv().map_err(|_| "closed".to_owned())?;
            buffer[..bytes.len()].copy_from_slice(&bytes);
            Ok(bytes.len())
        }
    }
    struct Writer;
    impl DeadlineWriter for Writer {
        fn write_all(&mut self, _: &[u8], _: Instant) -> Result<(), String> { Ok(()) }
        fn control_lines(&mut self, _: bool, _: bool) -> Result<(), String> { Ok(()) }
    }
    struct Cancel(mpsc::Sender<Vec<u8>>);
    impl CancelIo for Cancel {
        fn cancel(&self) { let _ = self.0.send(Vec::new()); }
    }

    #[test]
    fn absent_port_never_starts_a_reader() {
        let path = format!("osg-absent-{}", uuid::Uuid::new_v4());
        assert!(ReadPump::open(&path, 115_200, Arc::default()).is_err());
    }

    #[test]
    fn incoming_bytes_and_terminal_loss_wake_the_worker() {
        let (input, receive) = mpsc::channel();
        let wake = Arc::new(NativeWake::default());
        wake.bind_thread(thread::current()).unwrap();
        let pump = ReadPump::start(DuplexParts {
            receiver: Box::new(Receiver(receive)), writer: Box::new(Writer),
            cancel: Arc::new(Cancel(input.clone())),
        }, 115_200, wake.clone()).unwrap();
        assert!(pump.try_read().unwrap().is_none());
        let next = |bytes| {
            let observed = wake.observed();
            input.send(bytes).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while observed == wake.observed() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(!remaining.is_zero());
                wake.wait(observed, remaining);
            }
            pump.try_read()
        };
        assert_eq!(&**next(b"abc".to_vec()).unwrap().unwrap(), b"abc");
        assert!(next(Vec::new()).is_err());
    }
}

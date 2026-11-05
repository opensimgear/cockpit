use super::*;
use osg_mapping::{desktop::{Delivery, Key}, engine::{Counter, typed::Token}};
use std::{io::{self, Cursor, Read}, sync::Mutex, time::Instant};

#[derive(Clone, Default)]
struct Output(Arc<Mutex<Vec<u8>>>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
struct Recording(Arc<Mutex<Vec<Vec<NativeEvent>>>>);
impl Adapter for Recording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.lock().unwrap().push(events.to_vec());
        Ok(native_count(events))
    }
}
fn clock() -> u64 { 1_000 }
fn requests() -> Vec<Request> {
    let run = RunToken { session: "serve-recording".into(), generation: Counter(1) };
    let producer = Token { run: run.clone(), node_id: "button".into(), dependencies: vec![] };
    let down = Delivery { producer: producer.clone(), action_id: "tap".into(),
        operation_id: Counter(1), sequence: Counter(1), at_us: Counter(0), deadline_us: None,
        intent: Intent::Held { controls: vec![Control::Key { key: Key(4) }] } };
    let mut up = down.clone();
    up.operation_id = Counter(2);
    up.sequence = Counter(2);
    up.intent = Intent::Held { controls: vec![] };
    [Command::Activate { run, scope: Scope::DesktopWide },
        Command::Arm { producer, action_id: "tap".into() },
        Command::DeliverGuarded { delivery: down, release_after_us: Counter(1_000) },
        Command::Deliver { delivery: up }, Command::Stop]
        .into_iter().enumerate().map(|(index, command)| Request {
            sequence: Counter(index as u64 + 1), deadline_us: None, command,
        }).collect()
}
fn frames(requests: &[Request]) -> Vec<u8> {
    let mut bytes = vec![];
    for request in requests {
        serde_json::to_writer(&mut bytes, request).unwrap();
        bytes.push(b'\n');
    }
    bytes
}
fn acknowledgements(output: &Output) -> Vec<Ack> {
    let bytes = output.0.lock().unwrap();
    let mut lines = bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty());
    let hello: serde_json::Value = serde_json::from_slice(lines.next().unwrap()).unwrap();
    assert_eq!(hello["protocol"], PROTOCOL);
    assert_eq!(hello["backend"], "desktop-input");
    lines.map(|line| serde_json::from_slice(line).unwrap()).collect()
}

#[test]
fn recording_protocol_preserves_ordered_guarded_down_up_and_terminal_stop_acks() {
    let events = Arc::new(Mutex::new(vec![]));
    let output = Output::default();
    serve(Recording(events.clone()), Cursor::new(frames(&requests())), output.clone(), clock).unwrap();
    let acks = acknowledgements(&output);
    assert_eq!(acks.iter().map(|ack| ack.sequence.0).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    assert!(acks.iter().all(|ack| ack.ok && !ack.uncertain && ack.released_guards.is_empty()));
    assert_eq!((acks[2].attempted, acks[2].confirmed, acks[2].held.len()), (1, 1, 1));
    assert_eq!((acks[3].attempted, acks[3].confirmed, acks[3].held.len()), (1, 1, 0));
    assert!(acks[4].held.is_empty() && acks[4].cleanup_complete);
    assert_eq!(*events.lock().unwrap(), [
        vec![NativeEvent::Key { usage: 4, down: true, repeat: false }],
        vec![NativeEvent::Key { usage: 4, down: false, repeat: false }],
    ]);
}

#[test]
fn recording_eof_and_parse_error_release_held_input_without_inventing_an_ack() {
    for invalid_tail in [false, true] {
        let events = Arc::new(Mutex::new(vec![]));
        let output = Output::default();
        let mut bytes = frames(&requests()[..3]);
        if invalid_tail { bytes.extend_from_slice(b"invalid-json\n"); }
        let result = serve(Recording(events.clone()), Cursor::new(bytes), output.clone(), clock);
        assert_eq!(result.is_err(), invalid_tail);
        let acks = acknowledgements(&output);
        assert_eq!(acks.len(), 3);
        assert!(acks[2].ok && acks[2].held.len() == 1);
        let batches = events.lock().unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1], vec![NativeEvent::Key { usage: 4, down: false, repeat: false }]);
    }
}

struct WaitingInput { receiver: mpsc::Receiver<Vec<u8>>, current: Cursor<Vec<u8>> }
impl BufRead for WaitingInput {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.current.position() as usize == self.current.get_ref().len() {
            self.current = Cursor::new(self.receiver.recv().unwrap_or_default());
        }
        self.current.fill_buf()
    }
    fn consume(&mut self, amount: usize) { self.current.consume(amount); }
}
impl Read for WaitingInput {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(bytes.len());
        bytes[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}
struct BrokenOutput;
impl Write for BrokenOutput {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> { Err(io::ErrorKind::BrokenPipe.into()) }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[test]
fn broken_response_pipe_wakes_an_idle_worker_without_waiting_for_reader_eof() {
    let events = Arc::new(Mutex::new(vec![]));
    let (hold_reader_open, receiver) = mpsc::sync_channel(1);
    let before = Instant::now();
    let result = serve(Recording(events.clone()), WaitingInput { receiver, current: Cursor::new(vec![]) }, BrokenOutput, clock);
    drop(hold_reader_open);
    assert!(result.unwrap_err().contains("transport failed"));
    assert!(events.lock().unwrap().is_empty());
    assert!(before.elapsed() < Duration::from_millis(500));
}

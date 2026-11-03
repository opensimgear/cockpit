use super::*;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};
use std::time::Duration;

enum Packet { Bytes(Vec<u8>), Eof, Panic, Cancel }
struct Cancel {
    stopped: AtomicBool,
    input: mpsc::Sender<Packet>,
}
impl CancelIo for Cancel {
    fn cancel(&self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.input.send(Packet::Cancel);
    }
}
struct Receiver {
    input: mpsc::Receiver<Packet>,
    entered: mpsc::Sender<()>,
    pending: VecDeque<u8>,
    cancel: Arc<Cancel>,
}
impl BlockingReceiver for Receiver {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        if self.cancel.stopped.load(Ordering::Acquire) { return Err("cancelled".into()); }
        if self.pending.is_empty() {
            let _ = self.entered.send(());
            match self.input.recv().unwrap() {
                Packet::Bytes(bytes) => self.pending.extend(bytes),
                Packet::Eof => return Ok(0),
                Packet::Panic => panic!("fake native reader panic"),
                Packet::Cancel => return Err("cancelled".into()),
            }
        }
        let count = buffer.len().min(self.pending.len());
        for byte in &mut buffer[..count] { *byte = self.pending.pop_front().unwrap(); }
        Ok(count)
    }
}
struct Writer {
    written: mpsc::Sender<Vec<u8>>,
    controls: Arc<Mutex<Vec<(bool, bool)>>>,
    failed: Arc<AtomicBool>,
}
impl DeadlineWriter for Writer {
    fn write_all(&mut self, bytes: &[u8], _: Instant) -> Result<(), String> {
        self.written.send(bytes.to_vec()).unwrap();
        if self.failed.load(Ordering::Acquire) { Err("partial write".into()) } else { Ok(()) }
    }
    fn control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String> {
        self.controls.lock().unwrap().push((dtr, rts));
        Ok(())
    }
}
struct Peer {
    input: mpsc::Sender<Packet>,
    written: mpsc::Receiver<Vec<u8>>,
    entered: mpsc::Receiver<()>,
    captured: mpsc::Receiver<Input>,
    controls: Arc<Mutex<Vec<(bool, bool)>>>,
    failed: Arc<AtomicBool>,
}
impl Peer {
    fn bytes(&self, text: impl AsRef<[u8]>) {
        self.input.send(Packet::Bytes(text.as_ref().to_vec())).unwrap();
    }
    fn write(&self) -> Vec<u8> { wait(&self.written) }
    fn lost(&self) -> String {
        loop { if let Input::Lost(error) = wait(&self.captured) { return error; } }
    }
}
fn wait<T>(receiver: &mpsc::Receiver<T>) -> T {
    receiver.recv_timeout(Duration::from_secs(2)).expect("actor did not make progress")
}
fn fixture() -> (StreamConsole, Peer) { fixture_sink(None) }
fn fixture_sink(sink: Option<Sink>) -> (StreamConsole, Peer) {
    let (input, incoming) = mpsc::channel();
    let (entered, waiting) = mpsc::channel();
    let (written, outgoing) = mpsc::channel();
    let (capture, captured) = mpsc::channel();
    let controls = Arc::new(Mutex::new(vec![]));
    let failed = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(Cancel { stopped: AtomicBool::new(false), input: input.clone() });
    let parts = DuplexParts {
        receiver: Box::new(Receiver { input: incoming, entered,
            pending: VecDeque::new(), cancel: cancel.clone() }),
        writer: Box::new(Writer { written, controls: controls.clone(), failed: failed.clone() }),
        cancel,
    };
    let sink = sink.unwrap_or_else(|| Arc::new(move |event| { let _ = capture.send(event); }));
    let console = StreamConsole::new(parts, sink).unwrap();
    let peer = Peer { input, written: outgoing, entered: waiting,
        captured, controls, failed };
    wait(&peer.entered);
    (console, peer)
}
fn command(mut console: StreamConsole, text: &'static str, kind: ReplyKind)
    -> JoinHandle<(StreamConsole, Result<Reply, String>)>
{
    thread::spawn(move || {
        let result = console.exchange(text, kind, Instant::now() + Duration::from_secs(2));
        (console, result)
    })
}
fn snapshot() -> Value {
    serde_json::json!({"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,
        "dirty":false,"pendingApply":false,"view":"active","valid":true,
        "config":{"name":"Panel with spaces","expanderMask":0,"buttons":[[0,0,0]],
            "axes":[],"hats":[],"encoders":[],"selectors":[]},
        "layout":{"buttons":1,"axes":0,"hats":0,"reportBytes":1},"descriptorHex":"0501"})
}

#[test]
fn real_firmware_frames_cross_native_socket_and_decoder_while_config_json_is_pending(){
    use std::io::{Read,Write};
    let(parts,mut wire)=crate::transport::duplex::tcp_fixture();
    let(sent,received)=mpsc::channel();
    let decoder=Mutex::new(super::super::push::Decoder::new(77));
    let sink:Sink=Arc::new(move|input|{
        if let Input::Line{line,received_at}=input{
            let frame=decoder.lock().unwrap().decode(&line,received_at);
            let _=sent.send(frame);
        }
    });
    let console=StreamConsole::new(parts,sink).unwrap();
    let worker=command(console,"config get_active",ReplyKind::Config);
    let mut request=[0;18];wire.read_exact(&mut request).unwrap();
    assert_eq!(&request,b"config get_active\n");
    wire.write_all(b"OK\n").unwrap();
    wire.write_all(include_bytes!("fixtures/events-push.jsonl")).unwrap();
    let mut final_frame=None;
    for sequence in 1..=9{
        let frame=received.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!(frame.sequence,sequence);final_frame=Some(frame);
    }
    let final_frame=final_frame.unwrap();assert_eq!(final_frame.stream,6);assert_eq!(final_frame.cursor,1);
    assert!(!worker.is_finished(),"input delivery must not require the pending configuration snapshot");
    wire.write_all(format!("{}\n",snapshot()).as_bytes()).unwrap();
    let(console,reply)=worker.join().unwrap();
    assert!(matches!(reply.unwrap(),Reply::Config{error:None,..}));drop(console);
}

#[test]
fn push_is_delivered_during_json_wait_and_write_is_one_armed_frame() {
    let (console, peer) = fixture();
    let worker = command(console, "events state", ReplyKind::Json);
    assert_eq!(peer.write(), b"events state\n");
    peer.bytes("@events {\"edge\":1}\r\n");
    assert!(matches!(wait(&peer.captured), Input::Line { line, .. } if line.contains("edge")));
    peer.bytes("{\"ok\":true}\r\n");
    let (console, reply) = worker.join().unwrap();
    assert!(matches!(reply.unwrap(), Reply::Json(value) if value["ok"] == true));
    drop(console);
    assert_eq!(*peer.controls.lock().unwrap(), [(true, false), (false, false)]);
}

#[test]
fn config_negative_ack_consumes_valid_snapshot_and_interleaved_push() {
    let (console, peer) = fixture();
    let worker = command(console, "config apply", ReplyKind::Config);
    peer.write();
    peer.bytes("ERR claim_required\n@events {}\n");
    assert!(matches!(wait(&peer.captured), Input::Line { .. }));
    peer.bytes(format!("{}\n", snapshot()));
    let (console, result) = worker.join().unwrap();
    assert!(matches!(result.unwrap(), Reply::Config { error: Some(error), snapshot: value }
        if error == "claim_required" && value == snapshot()));
    let worker = command(console, "config read", ReplyKind::Config);
    peer.write();
    peer.bytes(format!("OK\n{}\n", snapshot()));
    let (_, result) = worker.join().unwrap();
    assert!(matches!(result.unwrap(), Reply::Config { error: None, .. }));
}

#[test]
fn first_fragment_clock_survives_chunks_and_mailbox_lock() {
    let (console, peer) = fixture();
    peer.bytes("@eve");
    // RX asks for the next fragment only after storing the first receipt time.
    wait(&peer.entered);
    let cutoff = Instant::now();
    let mailbox = console.shared.lock();
    peer.bytes("nts {\"a\":1}\r\n@events {}\n");
    drop(mailbox);
    let Input::Line { line, received_at } = wait(&peer.captured) else { panic!("lost input") };
    assert_eq!(line, "@events {\"a\":1}");
    assert!(received_at <= cutoff);
    assert!(matches!(wait(&peer.captured), Input::Line { line, .. } if line == "@events {}"));
}

#[test]
fn unsupported_json_is_a_complete_reply_but_malformed_reply_is_terminal() {
    let (console, peer) = fixture();
    let worker = command(console, "capabilities", ReplyKind::Json);
    peer.write();peer.bytes("ERR unknown command\n");
    let (console, result) = worker.join().unwrap();
    assert!(matches!(result.unwrap(), Reply::Unsupported));
    let worker = command(console, "capabilities", ReplyKind::Json);
    peer.write();peer.bytes("{\"ok\":7}\n");
    let (mut console, result) = worker.join().unwrap();
    assert!(result.unwrap_err().contains("invalid capability"));
    peer.lost();
    assert!(console.exchange("again", ReplyKind::Text, Instant::now()+Duration::from_secs(1)).is_err());
    assert!(peer.written.try_recv().is_err());
}

#[test]
fn no_active_reply_and_late_reply_invalidate_connection() {
    let (mut console, peer) = fixture();
    peer.bytes("OK\n");
    assert!(peer.lost().contains("unsolicited"));
    assert!(console.exchange("later", ReplyKind::Text, Instant::now()+Duration::from_secs(1)).is_err());
    assert!(peer.written.try_recv().is_err());
    let (console, peer) = fixture();
    let worker = command(console, "status", ReplyKind::Text);
    peer.write();peer.bytes("answer\n");
    let (console, result) = worker.join().unwrap();
    assert!(matches!(result.unwrap(), Reply::Text(line) if line == "answer"));
    peer.bytes("late ACK\n");assert!(peer.lost().contains("unsolicited"));
    drop(console);
}

#[test]
fn reserved_mailbox_rejects_extra_replies_without_eviction() {
    let (console, peer) = fixture();
    // Exercise the RX admission boundary without letting a consumer drain it.
    console.shared.lock().expected = Some(Expected { limit: 2, received: 0 });
    peer.bytes("OK\n{}\nextra\n");
    assert!(peer.lost().contains("overflow"));
    let state = console.shared.lock();
    assert!(state.replies.is_empty());assert!(state.expected.is_none());
}

#[test]
fn initial_banner_is_ignored_but_restart_banner_is_terminal() {
    let (mut console, peer) = fixture();
    peer.bytes(format!("{CONSOLE_BANNER}\r\n"));wait(&peer.entered);
    console.finish_handshake();
    peer.bytes(format!("{CONSOLE_BANNER}\n"));
    assert!(peer.lost().contains("restarted"));
}

#[test]
fn cancellation_wakes_blocked_exchange_and_prevents_second_write() {
    let (console, peer) = fixture();
    let cancel = console.cancel_handle();
    let worker = command(console, "config apply", ReplyKind::Config);
    peer.write();let started = Instant::now();cancel.cancel();cancel.cancel();
    let (mut console, result) = worker.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));assert!(result.is_err());
    assert!(console.exchange("retry", ReplyKind::Text, Instant::now()+Duration::from_secs(1)).is_err());
    assert!(peer.written.try_recv().is_err());
}

#[test]
fn deadline_and_unknown_write_outcome_are_terminal_without_replay() {
    let (mut console, peer) = fixture();
    let started = Instant::now();
    let result = console.exchange("status", ReplyKind::Text, started+Duration::from_millis(20));
    assert!(result.unwrap_err().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(1));peer.write();peer.lost();
    assert!(console.exchange("again", ReplyKind::Text, Instant::now()+Duration::from_secs(1)).is_err());
    assert!(peer.written.try_recv().is_err());
    let (mut console, peer) = fixture();peer.failed.store(true, Ordering::Release);
    assert!(console.exchange("config apply", ReplyKind::Config, Instant::now()+Duration::from_secs(1))
        .unwrap_err().contains("result is unknown"));
    peer.write();peer.lost();
    assert!(console.exchange("again", ReplyKind::Text, Instant::now()+Duration::from_secs(1)).is_err());
    assert!(peer.written.try_recv().is_err());
}

#[test]
fn invalid_commands_do_not_write_or_poison_a_healthy_connection() {
    let (mut console, peer) = fixture();
    for invalid in ["".to_owned(), "status\n".into(), "é".into(), "x".repeat(384),
        format!("config {}", "x".repeat(384))] {
        assert!(console.exchange(&invalid, ReplyKind::Text, Instant::now()+Duration::from_secs(1)).is_err());
    }
    assert!(peer.written.try_recv().is_err());
    let worker = command(console, "status", ReplyKind::Text);
    peer.write();peer.bytes("OK\n");assert!(worker.join().unwrap().1.is_ok());
}

#[test]
fn malformed_config_ack_is_immediate_and_invalid_snapshot_is_terminal() {
    for (ack, value) in [("ERR bad-reason", snapshot()), ("OK", serde_json::json!({}))] {
        let (console, peer) = fixture();
        let worker = command(console, "config read", ReplyKind::Config);
        peer.write();
        if ack == "OK" { peer.bytes(format!("{ack}\n{value}\n")); }
        else { peer.bytes(format!("{ack}\n")); }
        let (_, result) = worker.join().unwrap();
        let error = result.unwrap_err();
        if ack != "OK" { assert!(error.contains("acknowledgement")); }
        peer.lost();
    }
}

#[test]
fn eof_utf8_line_bounds_and_reader_panics_become_explicit_loss() {
    for packet in [Packet::Eof, Packet::Bytes(vec![0xff, b'\n']),
        Packet::Bytes(vec![b'x'; MAX_LINE_BYTES+2]), Packet::Panic] {
        let (console, peer) = fixture();peer.input.send(packet).unwrap();
        assert!(!peer.lost().is_empty());drop(console);
    }
    let (console, peer) = fixture();
    let text = format!("@events {}\r\n", "x".repeat(MAX_LINE_BYTES-8));
    peer.bytes(text);
    assert!(matches!(wait(&peer.captured), Input::Line { line, .. } if line.len() == MAX_LINE_BYTES));
    drop(console);
}

#[test]
fn capture_panic_is_terminal_and_drop_cancels_an_idle_reader() {
    let (console, peer) = fixture_sink(Some(Arc::new(|_| panic!("capture panic"))));
    peer.bytes("@events {}\n");
    // Synchronize directly on the terminal condition since the sink itself panics.
    let state = console.shared.lock();
    let (state, _) = console.shared.changed.wait_timeout_while(state, Duration::from_secs(2),
        |state| state.fault.is_none()).unwrap();
    assert!(state.fault.as_ref().unwrap().contains("capture panicked"));
    drop(state);drop(console);
    let (console, peer) = fixture();let cancel = console.cancel_handle();
    let started = Instant::now();drop(console);cancel.cancel();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(*peer.controls.lock().unwrap(), [(true, false), (false, false)]);
}

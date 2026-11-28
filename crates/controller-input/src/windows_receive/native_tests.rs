use super::*;
use std::collections::VecDeque;

pub(super) enum Finish {
    Data(Vec<u8>, usize),
    Error(u32),
}
struct State {
    starts: usize,
    cancels: usize,
    finishes: VecDeque<Finish>,
}
static STATE: Mutex<State> = Mutex::new(State {
    starts: 0,
    cancels: 0,
    finishes: VecDeque::new(),
});
pub(super) static MOCK: io::Io = io::Io {
    start,
    complete,
    wait_event,
    cancel,
};
pub(super) static SERIAL: Mutex<()> = Mutex::new(());
fn start(_: &File, _: &mut Pending, kind: Option<ReportKind>) -> Result<(), ReceiveError> {
    if kind == Some(ReportKind::Output) {
        return Err(error(ErrorKind::InvalidRequest, "No output test operation"));
    }
    STATE.lock().unwrap().starts += 1;
    Ok(())
}
fn complete(_: &File, pending: &mut Pending) -> Option<Result<usize, ReceiveError>> {
    STATE
        .lock()
        .unwrap()
        .finishes
        .pop_front()
        .map(|finish| match finish {
            Finish::Data(bytes, count) => {
                let copied = bytes.len().min(pending.bytes.len());
                pending.bytes[..copied].copy_from_slice(&bytes[..copied]);
                Ok(count)
            }
            Finish::Error(code) => Err(policy::os(code, "test completion")),
        })
}
fn wait_event(_: &Pending, _: Duration) -> Result<bool, ReceiveError> {
    Ok(false)
}
fn cancel(_: &File, _: &Pending) {
    STATE.lock().unwrap().cancels += 1;
}
pub(super) fn finish(value: Finish) {
    STATE.lock().unwrap().finishes.push_back(value);
}
pub(super) fn session() -> Session {
    SLOTS.fetch_add(1, Ordering::AcqRel);
    Session {
        file: Arc::new(File {
            handle: INVALID_HANDLE_VALUE,
            path: "fake-kernel".into(),
            pp: 0,
            _permit: Permit,
            io: &MOCK,
        }),
        stream: None,
        get: None,
    }
}
pub(super) fn reset() {
    let mut state = STATE.lock().unwrap();
    state.starts = 0;
    state.cancels = 0;
    state.finishes.clear();
}

#[test]
fn actual_receive_state_machine_preserves_order_fences_late_get_and_retires_without_waiting() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    let slots_before = SLOTS.load(Ordering::Acquire);
    let mut stream = session();
    assert!(stream.poll(4, Duration::ZERO).unwrap().is_none());
    let address = (&stream.stream.as_ref().unwrap().overlap) as *const OVERLAPPED;
    assert!(stream.poll(4, Duration::ZERO).unwrap().is_none());
    assert_eq!(
        address,
        (&stream.stream.as_ref().unwrap().overlap) as *const OVERLAPPED
    );
    assert_eq!(STATE.lock().unwrap().starts, 1);
    assert_eq!(STATE.lock().unwrap().cancels, 0);
    for _ in 0..2 {
        finish(Finish::Data(vec![3, 0x81, 0x7f], 3));
    }
    assert_eq!(
        stream.poll(4, Duration::ZERO).unwrap(),
        Some(vec![3, 0x81, 0x7f])
    );
    assert_eq!(
        stream.poll(4, Duration::ZERO).unwrap(),
        Some(vec![3, 0x81, 0x7f])
    );
    assert_eq!(
        STATE.lock().unwrap().starts,
        2,
        "Repeated equal reports are independent ordered frames"
    );
    finish(Finish::Error(ERROR_DEVICE_NOT_CONNECTED));
    assert_eq!(
        stream.poll(4, Duration::ZERO).unwrap_err().kind,
        ErrorKind::Removed
    );
    finish(Finish::Error(ERROR_OPERATION_ABORTED));
    drop(stream);

    let mut get = session();
    assert_eq!(
        get.get(ReportKind::Feature, 3, 4, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::Timeout
    );
    let started = STATE.lock().unwrap().starts;
    assert_eq!(
        get.get(ReportKind::Feature, 3, 4, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::Busy
    );
    assert_eq!(STATE.lock().unwrap().starts, started);
    finish(Finish::Data(vec![3, 0xee], 2));
    assert_eq!(
        get.get(ReportKind::Feature, 3, 4, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::Timeout,
        "Expired GET data must be discarded before the fresh GET"
    );
    finish(Finish::Error(ERROR_OPERATION_ABORTED));
    finish(Finish::Data(vec![3, 1, 2], 3));
    assert_eq!(
        get.get(ReportKind::Input, 3, 4, Duration::ZERO).unwrap(),
        vec![3, 1, 2]
    );
    finish(Finish::Data(vec![0, 1, 2], 2));
    assert_eq!(
        get.get(ReportKind::Feature, 0, 4, Duration::ZERO).unwrap(),
        vec![0, 1, 2]
    );
    drop(get);

    let mut retired = session();
    assert!(retired.poll(4, Duration::ZERO).unwrap().is_none());
    drop(retired);
    assert_eq!(
        SLOTS.load(Ordering::Acquire),
        slots_before + 1,
        "Pending cancellation retains its bounded session permit"
    );
    assert!(RETIRED
        .lock()
        .unwrap()
        .iter()
        .any(|entry| std::ptr::eq(entry.file.io, &MOCK)));
    finish(Finish::Error(ERROR_OPERATION_ABORTED));
    reap();
    assert_eq!(SLOTS.load(Ordering::Acquire), slots_before);
    assert!(!RETIRED
        .lock()
        .unwrap()
        .iter()
        .any(|entry| std::ptr::eq(entry.file.io, &MOCK)));
}

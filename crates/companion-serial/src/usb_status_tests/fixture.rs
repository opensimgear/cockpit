use super::*;

pub(super) fn duplex() -> (DuplexParts, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Value>) {
    let (device, incoming) = mpsc::channel();
    let (outgoing, commands) = mpsc::channel();
    let parts = DuplexParts {
        receiver: Box::new(ReceiverHalf(incoming)),
        writer: Box::new(WriterHalf(outgoing)),
        cancel: Arc::new(Cancel(device.clone())),
    };
    (parts, device, commands)
}

pub(super) fn start_worker(expected: Option<&'static str>, paired: bool) -> WorkerFixture {
    start_worker_with_reopen(expected, paired, None)
}

pub(super) fn start_worker_with_reopen(
    expected: Option<&'static str>,
    paired: bool,
    mut reopen: Option<DuplexParts>,
) -> WorkerFixture {
    let (first, device, outgoing) = duplex();
    let mut first = Some(first);
    let (mutations, committed) = mpsc::channel();
    let (control, commands) = mpsc::channel();
    let wake = Arc::new(NativeWake::default());
    let worker_wake = wake.clone();
    let public = Arc::new(Mutex::new(json!({"connected":true,"ready":false})));
    let observed = public.clone();
    let observer = StatusObserver::new(move || {
        let state = observed.lock().unwrap();
        mutations.send(state["device"]["status"].clone()).unwrap();
    });
    let worker_public = public.clone();
    let handle = thread::spawn(move || {
        let hub = Hub::new(
            "test-host".into(),
            Arc::new(osg_companion_service::NullSourceSink),
        );
        if paired {
            assign_screen(&hub);
        }
        worker(
            &hub,
            &commands,
            &worker_public,
            expected,
            &observer,
            worker_wake,
            |baud, wake| {
                let port = if baud == 115200 { first.take() } else { reopen.take() };
                ReadPump::start(port.ok_or("Unexpected reopen")?, baud, wake)
            },
        )
    });
    WorkerFixture {
        device,
        outgoing,
        committed,
        control,
        wake,
        public,
        handle,
    }
}

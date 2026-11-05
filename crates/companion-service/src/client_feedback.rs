use super::*;

const TRANSFER: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_millis(250);

pub(super) fn connect_feedback(
    credential: &SavedCredential,
    identity: &Value,
    scope: &IoScope,
) -> Result<Socket, String> {
    let mut socket = connect(&credential.endpoint, &credential.ca_certificate, &credential.ca_spki_sha256, scope)?;
    let reply = request(&mut socket, json!({
        "type":"feedback-subscribe","token":credential.token,"sessionId":identity["sessionId"],
        "generation":identity["generation"],"publicationHash":identity["publicationHash"],
    }), TRANSFER)?;
    if reply["type"] != "feedback-subscribed" {
        return Err("Host did not admit feedback observation".into());
    }
    Ok(socket)
}

pub(super) struct Reader {
    stop: Arc<AtomicBool>,
    scope: IoScope,
}

impl Reader {
    pub(super) fn start(
        shared: Arc<Shared>,
        epoch: u64,
        credential: SavedCredential,
        identity: Value,
        socket: Socket,
        scope: IoScope,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let reader_scope = scope.clone();
        thread::spawn(move || {
            let _ = reader_scope.bind_thread();
            let mut socket = Some(socket);
            while !cancelled.load(Ordering::Acquire)
                && !reader_scope.is_closed()
                && current(&shared, epoch)
            {
                // Capture before I/O so a close racing with the operation cannot
                // be mistaken for a fresh notification and lose the wakeup.
                let observed = reader_scope.observed();
                let result = match socket.as_mut() {
                    Some(socket) => read_event(socket, &shared, epoch, &reader_scope),
                    None => connect_feedback(&credential, &identity, &reader_scope).map(|connected| { socket = Some(connected); }),
                };
                if reader_scope.is_closed() || !current(&shared, epoch) || cancelled.load(Ordering::Acquire) {
                    break;
                }
                if result.is_err() {
                    socket = None;
                    if let Ok(mut state) = shared.state.lock() {
                        if state.status.epoch == epoch && !state.status.feedback.is_empty() {
                            state.status.feedback.clear();
                            state.feedback_accumulator = Default::default();
                            state.status.feedback_revision = state.status.feedback_revision.saturating_add(1);
                            shared.status_wake.notify();
                        }
                    }
                    if reader_scope.wait(observed, RETRY) { break; }
                }
            }
        });
        Self { stop, scope }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.scope.close();
    }
}

fn read_event(socket: &mut Socket, shared: &Shared, epoch: u64, scope: &IoScope) -> Result<(), String> {
    if scope.is_closed() || !current(shared, epoch) { return Ok(()); }
    // The first frame is an idle read. The scope's cancellation registration
    // interrupts it when this reader or its epoch is retired.
    let first = match socket.read() {
        Ok(message) => message,
        Err(error) => return Err(error.to_string()),
    };
    if scope.is_closed() || !current(shared, epoch) { return Ok(()); }
    let event = receive_event(socket, Some(first), TRANSFER)?;
    let mut state = shared.state.lock().map_err(|_| "Client state lock poisoned")?;
    if state.status.epoch != epoch || !current(shared, epoch) { return Ok(()); }
    if event["ok"] != true { return Err("Host rejected feedback observation".into()); }
    if event["type"] == "assignments-event" && event["assignments"].is_array() {
        if state.status.assignments != event["assignments"] {
            state.status.assignments = event["assignments"].clone();
            shared.status_wake.notify();
        }
        return Ok(());
    }
    if event["type"] != "feedback-event" { return Err("Invalid feedback event".into()); }
    let sequence = event["resyncSequence"].as_str().and_then(|value| value.parse::<u64>().ok())
        .ok_or("Missing feedback resync sequence")?;
    let resync = state.feedback_reset_pending;
    if resync && sequence != shared.feedback_resync.load(Ordering::SeqCst) { return Ok(()); }
    let revision = state.status.feedback_revision;
    apply_feedback(&mut state, &event["feedback"], shared.started.elapsed().as_millis() as u64, resync)?;
    if revision != state.status.feedback_revision { shared.status_wake.notify(); }
    Ok(())
}

use super::*;

const TRANSFER: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_millis(250);

pub(super) fn connect_feedback(credential: &SavedCredential, identity: &Value) -> Result<Socket, String> {
    let mut socket = connect(&credential.endpoint, &credential.ca_certificate, &credential.ca_spki_sha256)?;
    let reply = request(&mut socket, json!({
        "type":"feedback-subscribe","token":credential.token,"sessionId":identity["sessionId"],
        "generation":identity["generation"],"publicationHash":identity["publicationHash"],
    }), TRANSFER)?;
    if reply["type"] != "feedback-subscribed" {
        return Err("Host did not admit feedback observation".into());
    }
    Ok(socket)
}

pub(super) struct Reader(Arc<AtomicBool>);

impl Reader {
    pub(super) fn start(shared: Arc<Shared>, epoch: u64, credential: SavedCredential, identity: Value, socket: Socket) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        thread::spawn(move || {
            let mut socket = Some(socket);
            while !cancelled.load(Ordering::Acquire) && current(&shared, epoch) {
                let result = match socket.as_mut() {
                    Some(socket) => read_event(socket, &shared, epoch),
                    None => connect_feedback(&credential, &identity).map(|connected| { socket = Some(connected); }),
                };
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
                    thread::sleep(RETRY);
                }
            }
        });
        Self(stop)
    }
}

impl Drop for Reader {
    fn drop(&mut self) { self.0.store(true, Ordering::Release); }
}

fn read_event(socket: &mut Socket, shared: &Shared, epoch: u64) -> Result<(), String> {
    let first = match socket.read() {
        Ok(message) => message,
        Err(error) if crate::server::timed_out(&error) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let event = crate::server::receive_json_from(socket, TRANSFER, Some(first))?;
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

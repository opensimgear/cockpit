use super::*;
use osg_streams::NativeWake;
use std::sync::Weak;

const RECONNECT: Duration = Duration::from_secs(1);
const KEEPALIVE: Duration = Duration::from_secs(2);

struct Presence {
    primary: Socket,
    assignments: Socket,
    last_ping: Instant,
}

impl Presence {
    fn connect(credential: &SavedCredential) -> Result<Self, String> {
        let open = || connect(&credential.endpoint, &credential.ca_certificate, &credential.ca_spki_sha256);
        let mut primary = open()?;
        let auth = request(&mut primary, json!({"type":"auth","token":credential.token}), KEEPALIVE)?;
        if auth["hostId"] != credential.host_id || auth["companionId"] != credential.companion_id {
            return Err("Assigned host identity mismatch".into());
        }
        let mut assignments = open()?;
        let reply = request(&mut assignments, json!({
            "type":"assignments-subscribe","connectionId":auth["connectionId"],"token":credential.token,
        }), KEEPALIVE)?;
        if reply["type"] != "assignments-subscribed" {
            return Err("Host did not admit assignment observation".into());
        }
        Ok(Self { primary, assignments, last_ping: Instant::now() })
    }

    fn read(&mut self) -> Result<Option<Value>, String> {
        if self.last_ping.elapsed() >= KEEPALIVE {
            let reply = request(&mut self.primary, json!({"type":"ping"}), KEEPALIVE)?;
            if reply["type"] != "pong" { return Err("Invalid presence acknowledgement".into()); }
            self.last_ping = Instant::now();
        }
        let message = match self.assignments.read() {
            Ok(message) => message,
            Err(error) if crate::server::timed_out(&error) => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let event = crate::server::receive_json_from(&mut self.assignments, KEEPALIVE, Some(message))?;
        if event["ok"] != true || event["type"] != "assignments-event" || !event["assignments"].is_array() {
            return Err("Invalid assignment event".into());
        }
        Ok(Some(event["assignments"].clone()))
    }
}

// This connection never creates an input source or renews an input lease.
pub(super) fn start_presence(owner: Weak<Shared>, wake: Arc<NativeWake>) {
    thread::spawn(move || {
        let _ = wake.bind_thread(thread::current());
        let mut presence = None;
        let mut identity = Zeroizing::new(String::new());
        loop {
            let observed = wake.observed();
            let Some(shared) = owner.upgrade() else { break };
            let credential = shared.state.lock().ok().and_then(|state| {
                if state.status.protected_store_available && !shared.running.load(Ordering::SeqCst) {
                    state.credential.clone()
                } else {
                    None
                }
            });
            drop(shared);
            let Some(credential) = credential else {
                presence = None;
                identity.clear();
                wake.wait_scheduled(observed, None);
                continue;
            };
            if identity.as_str() != credential.token {
                presence = None;
                *identity = credential.token.clone();
            }
            let result = match &mut presence {
                Some(connection) => Presence::read(connection),
                None => Presence::connect(&credential).map(|connection| { presence = Some(connection); None }),
            };
            match result {
                Ok(Some(rows)) => {
                    let Some(shared) = owner.upgrade() else { break };
                    if let Ok(mut state) = shared.state.lock() {
                        if !shared.running.load(Ordering::SeqCst)
                            && state.credential.as_ref().is_some_and(|saved| saved.token == credential.token)
                            && state.status.assignments != rows {
                            state.status.assignments = rows;
                            shared.status_wake.notify();
                        }
                    };
                }
                Ok(None) => {}
                Err(_) => {
                    presence = None;
                    wake.wait_scheduled(observed, Some(RECONNECT));
                }
            }
        }
    });
}

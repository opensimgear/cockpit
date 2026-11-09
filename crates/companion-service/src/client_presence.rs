use super::*;
use osg_streams::NativeWake;
use std::sync::Weak;

const RECONNECT: Duration = Duration::from_secs(1);
const KEEPALIVE: Duration = Duration::from_secs(2);

struct Presence {
    primary: Socket,
    assignments: Socket,
    last_ping: Instant,
    scope: IoScope,
    epoch: u64,
}

impl Presence {
    fn connect(credential: &SavedCredential, scope: &IoScope, epoch: u64) -> Result<Self, String> {
        let open = || connect(&credential.endpoint, &credential.ca_certificate, &credential.ca_spki_sha256, scope);
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
        Ok(Self { primary, assignments, last_ping: Instant::now(), scope: scope.clone(), epoch })
    }

    fn read(&mut self) -> Result<Option<Value>, String> {
        if self.scope.is_closed() { return Err("Presence I/O scope is closed".into()); }
        if Instant::now() >= self.last_ping + KEEPALIVE {
            let reply = request(&mut self.primary, json!({"type":"ping"}), KEEPALIVE)?;
            if reply["type"] != "pong" { return Err("Invalid presence acknowledgement".into()); }
            self.last_ping = Instant::now();
        }
        // Keep the passive assignment connection responsive at least once per
        // heartbeat interval; a timeout is the opportunity to ping primary.
        let deadline = self.last_ping + KEEPALIVE;
        let read_result = {
            let mut operation = self.assignments.operation(Some(deadline));
            operation.read()
        };
        let message = match read_result {
            Ok(message) => message,
            Err(error) if crate::server::timed_out(&error) => {
                if self.scope.is_closed() { return Err("Presence I/O scope is closed".into()); }
                let reply = request(&mut self.primary, json!({"type":"ping"}), KEEPALIVE)?;
                if reply["type"] != "pong" { return Err("Invalid presence acknowledgement".into()); }
                self.last_ping = Instant::now();
                return Ok(None);
            }
            Err(error) => return Err(error.to_string()),
        };
        if self.scope.is_closed() { return Err("Presence I/O scope is closed".into()); }
        let event = receive_event(&mut self.assignments, Some(message), KEEPALIVE)?;
        if event["ok"] != true || event["type"] != "assignments-event" || !event["assignments"].is_array() {
            return Err("Invalid assignment event".into());
        }
        Ok(Some(event["assignments"].clone()))
    }
}

fn idle_credential<'a>(shared: &Shared, state: &'a State) -> Option<&'a SavedCredential> {
    if state.status.authentication_available
        && shared.network_access.load(Ordering::SeqCst)
        && !shared.running.load(Ordering::SeqCst)
    {
        state.credential.as_ref()
    } else {
        None
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
            let selection = shared.state.lock().ok().and_then(|state| {
                let credential = idle_credential(&shared, &state)?;
                // Copy the credential only when opening a transport. An idle
                // socket timeout or assignment delivery needs only identity checks.
                Some((if presence.as_ref().is_some_and(|current: &Presence| !current.scope.is_closed() && current.epoch == state.status.epoch) && identity.as_str() == credential.token {
                    None
                } else {
                    Some(credential.clone())
                }, state.status.epoch))
            });
            let selection = selection.map(|(credential, epoch)| {
                let scope = credential.as_ref().map(|_| io_scope(&shared, epoch));
                (credential, scope, epoch)
            });
            drop(shared);
            let Some(selection) = selection else {
                presence = None;
                identity.clear();
                wake.wait_scheduled(observed, None);
                continue;
            };
            let result = match selection {
                (Some(credential), Some(Ok(scope)), epoch) => {
                    presence = None;
                    *identity = credential.token.clone();
                    let _ = scope.bind_thread();
                    Presence::connect(&credential, &scope, epoch).map(|connection| { presence = Some(connection); None })
                }
                (Some(_), Some(Err(error)), _) => Err(error),
                (None, None, _) => {
                    let Some(current) = presence.as_mut() else { continue };
                    current.read()
                }
                _ => continue,
            };
            match result {
                Ok(Some(rows)) => {
                    let Some(shared) = owner.upgrade() else { break };
                    if let Ok(mut state) = shared.state.lock() {
                        if presence.as_ref().is_some_and(|current| !current.scope.is_closed()
                            && current.epoch == shared.epoch.load(Ordering::SeqCst))
                            && idle_credential(&shared, &state).is_some_and(|saved| saved.token == identity.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_presence_requires_authentication_and_yields_to_the_session_owner() {
        let directory = tempfile::tempdir().unwrap();
        let client = ClientRuntime::open(directory.path().to_path_buf()).unwrap();
        let mut state = client.shared.state.lock().unwrap();
        state.credential = Some(SavedCredential {
            companion_id: "fixture".into(), name: "Fixture".into(), token: "fixture".into(),
            endpoint: String::new(), host_id: String::new(), ca_certificate: String::new(),
            ca_spki_sha256: String::new(), assignments: json!([]),
        });
        state.status.protected_store_available = true;
        state.status.authentication_available = false;
        assert!(idle_credential(&client.shared, &state).is_none());
        state.status.authentication_available = true;
        assert!(idle_credential(&client.shared, &state).is_some());
        client.shared.network_access.store(false, Ordering::SeqCst);
        assert!(idle_credential(&client.shared, &state).is_none());
        client.shared.network_access.store(true, Ordering::SeqCst);
        client.shared.running.store(true, Ordering::SeqCst);
        assert!(idle_credential(&client.shared, &state).is_none());
    }
}

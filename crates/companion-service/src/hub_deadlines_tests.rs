use super::*;
use crate::{hub::{CodeOffer, Offer, Pending, Session}, SourceToken};
use mio::{Events, Poll, Token, Waker};
use serde_json::json;
use std::{collections::{BTreeMap, VecDeque}, sync::Arc, time::Duration};
use zeroize::Zeroizing;

fn session(last_seen: u64) -> Session {
    Session {
        connection: "connection".into(),
        transport: crate::hub::SourceTransport::Network,
        client_id: "client".into(),
        companion_id: "companion".into(),
        token: SourceToken {
            source_id: "instance".into(),
            session_id: "session".into(),
            generation: 1,
            publication_hash: "publication".into(),
        },
        armed: false,
        baseline_pending: true,
        panel_error: false,
        operation: None,
        exhausted: false,
        sequence: 0,
        heartbeat: 0,
        last_seen,
        #[cfg(all(debug_assertions, feature = "reactive-qualification"))]
        lease_evidence: Default::default(),
        batches: VecDeque::new(),
        values: BTreeMap::new(),
        feedback_cursor: Default::default(),
        feedback_wake: None,
        feedback_stream_resync: None,
    }
}

#[cfg(all(debug_assertions, feature = "reactive-qualification"))]
#[test]
fn qualification_counts_armed_renewals_and_includes_the_current_lease_gap() {
    let hub = Hub::new("lease-host".into(), Arc::new(crate::hub::NullSourceSink));
    let mut first = session(100);
    first.note_input_renewal(200);
    first.armed = true;
    first.note_input_renewal(500);
    first.last_seen = 500;
    first.note_input_renewal(900);
    first.last_seen = 900;
    let mut second = session(950);
    second.armed = true;
    second.token.source_id = "peer".into();
    second.token.session_id = "peer-session".into();
    {
        let mut inner = hub.inner.lock().unwrap();
        inner.last_now = 1000;
        inner.sessions.insert("session".into(), first);
        inner.sessions.insert("peer-session".into(), second);
    }
    let first = json!({
        "instanceId": "instance", "sessionId": "session",
        "generation": "1", "publicationHash": "publication",
    });
    let second = json!({
        "instanceId": "peer", "sessionId": "peer-session",
        "generation": "1", "publicationHash": "publication",
    });
    let read = |first, second| {
        hub.qualify_input_leases(serde_json::from_value(json!([first, second])).unwrap())
    };
    let rows = read(first.clone(), second.clone()).unwrap();
    assert_eq!(rows[0]["acceptedRenewals"], 2);
    assert_eq!(rows[0]["maxGapMs"], 400);
    assert_eq!(rows[0]["gapAtReadMs"], 100);
    assert_eq!(rows[0]["minimumMarginMs"], 1100);
    assert_eq!(rows[1]["acceptedRenewals"], 0);
    assert_eq!(rows[1]["maxGapMs"], 50);
    let mut stale = second.clone();
    stale["generation"] = json!("2");
    assert!(read(first.clone(), stale).is_err());
    assert!(read(first.clone(), first.clone()).is_err());
    hub.inner.lock().unwrap().last_now = 2400;
    assert!(read(first, second).is_err());
}

#[test]
fn no_deadline_until_expiring_state_is_created() {
    let hub = Hub::new("deadline-host".into(), Arc::new(crate::hub::NullSourceSink));
    assert_eq!(hub.next_deadline_ms(), None);

    let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    assert_eq!(hub.next_deadline_ms(), offer["expiresAtMs"].as_u64());
}

#[test]
fn next_deadline_is_the_minimum_across_pairing_and_session_leases() {
    let hub = Hub::new("deadline-host".into(), Arc::new(crate::hub::NullSourceSink));
    {
        let mut inner = hub.inner.lock().unwrap();
        inner.offer = Some(Offer { secret_hash: String::new(), expires: 90, consumed: false, auto_approve: false, kind: "desktop".into() });
        inner.code_offer = Some(CodeOffer { code: Zeroizing::new("code".into()), payload: json!({}), expires: 80, attempts: 0 });
        inner.pending.insert("pending".into(), Pending {
            kind: "desktop".into(), companion_id: "companion".into(), name: "Screen".into(),
            secret_hash: String::new(), expires: 70, token: None,
        });
        inner.sessions.insert("session".into(), session(0));
    }
    assert_eq!(hub.next_deadline_ms(), Some(70));
}

#[test]
fn baseline_shortens_a_preparing_session_deadline() {
    let hub = Hub::new("deadline-host".into(), Arc::new(crate::hub::NullSourceSink));
    hub.inner.lock().unwrap().sessions.insert("session".into(), session(100));
    assert_eq!(hub.next_deadline_ms(), Some(100 + PANEL_PREPARATION_LEASE_MS));
    {
        let mut inner = hub.mutation();
        let session = inner.sessions.get_mut("session").unwrap();
        session.armed = true;
        session.baseline_pending = false;
    }
    assert_eq!(hub.next_deadline_ms(), Some(100 + LEASE_MS));
}

#[test]
fn registering_deadline_observer_wakes_poll_for_a_new_deadline() {
    let hub = Hub::new("deadline-host".into(), Arc::new(crate::hub::NullSourceSink));
    let mut poll = Poll::new().unwrap();
    let wake = Arc::new(Waker::new(poll.registry(), Token(7)).unwrap());
    hub.observe_deadlines(wake.clone()).unwrap();
    let mut events = Events::with_capacity(8);
    poll.poll(&mut events, Some(Duration::from_millis(100))).unwrap();

    hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    poll.poll(&mut events, Some(Duration::from_millis(500))).unwrap();
    assert!(events.iter().any(|event| event.token() == Token(7)));

    // Drain the offer notification, then introduce a session lease that is earlier.
    events.clear();
    poll.poll(&mut events, Some(Duration::from_millis(100))).unwrap();
    {
        let mut inner = hub.mutation();
        let mut session = session(hub.now_ms());
        session.armed = true;
        session.baseline_pending = false;
        inner.sessions.insert("session".into(), session);
    }
    poll.poll(&mut events, Some(Duration::from_millis(500))).unwrap();
    assert!(events.iter().any(|event| event.token() == Token(7)));

    {
        let mut inner = hub.mutation();
        inner.sessions.get_mut("session").unwrap().last_seen += 100;
    }
    poll.poll(&mut events, Some(Duration::ZERO)).unwrap();
    assert!(events.is_empty(), "Lease extension must not add a wake per input report");
}

#[test]
fn deadline_observers_are_deduplicated_and_bounded() {
    let hub = Hub::new("deadline-host".into(), Arc::new(crate::hub::NullSourceSink));
    let mut polls = Vec::new();
    let mut wakes = Vec::new();
    for index in 0..MAX_DEADLINE_OBSERVERS {
        let poll = Poll::new().unwrap();
        let wake = Arc::new(Waker::new(poll.registry(), Token(index)).unwrap());
        hub.observe_deadlines(wake.clone()).unwrap();
        wakes.push(wake);
        polls.push(poll);
    }
    hub.observe_deadlines(wakes[0].clone()).unwrap();
    let extra_poll = Poll::new().unwrap();
    let extra = Arc::new(Waker::new(extra_poll.registry(), Token(99)).unwrap());
    assert!(hub.observe_deadlines(extra).is_err());
    wakes.pop();
    let replacement_poll = Poll::new().unwrap();
    let replacement = Arc::new(Waker::new(replacement_poll.registry(), Token(100)).unwrap());
    hub.observe_deadlines(replacement).unwrap();
}

#[test]
fn the_parked_listener_expires_a_shortened_lease_without_status_or_tick_reads() {
    struct Loss(std::sync::mpsc::Sender<String>);
    impl crate::SourceSink for Loss {
        fn register(&self, _: crate::SourceDefinition) -> Result<(), String> { Ok(()) }
        fn baseline(&self, _: &SourceToken, _: u64, _: &[crate::SourceChange]) -> Result<(), String> { Ok(()) }
        fn changes(&self, _: &SourceToken, _: u64, _: &[crate::SourceChange]) -> Result<(), String> { Ok(()) }
        fn loss(&self, token: &SourceToken) -> Result<(), String> {
            self.0.send(token.session_id.clone()).map_err(|error| error.to_string())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let (lost, receipt) = std::sync::mpsc::channel();
    let hub = Hub::new("deadline-owner".into(), Arc::new(Loss(lost)));
    let server = crate::server::ServerRuntime::new(hub.clone(), directory.path().to_owned()).unwrap();
    server.start(crate::server::ServerConfig {
        bind:"127.0.0.1:0".parse().unwrap(), advertised_host:"127.0.0.1".into(), discovery:false,
    }).unwrap();
    {
        let mut inner = hub.mutation();
        inner.sessions.insert("session".into(), session(hub.now_ms()));
    }
    {
        let mut inner = hub.mutation();
        let session = inner.sessions.get_mut("session").unwrap();
        session.armed = true;
        session.baseline_pending = false;
    }
    // Only the listener may drive expiry here. Reads that call Hub::expire or
    // a manual tick would conceal a lost rearm notification.
    assert_eq!(receipt.recv_timeout(Duration::from_secs(4)).unwrap(), "session");
    assert!(!hub.inner.lock().unwrap().sessions.contains_key("session"));
    server.stop();
}

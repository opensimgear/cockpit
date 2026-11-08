use super::*;
use std::io::{BufRead, BufReader};

#[test]
fn configuration_read_waits_for_route_evidence_without_retry_queries() {
    exercise_route(false, false);
}

#[test]
fn action_read_consumes_route_change_received_before_the_rejection() {
    exercise_route(true, false);
}

#[test]
fn configuration_read_rejects_a_restarted_module_after_route_change() {
    exercise_route(false, true);
}

fn exercise_route(action: bool, restarted: bool) {
    use crate::transport::duplex::tcp_fixture;

    let (parts, mut peer) = tcp_fixture();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let device = thread::spawn(move || {
        let mut input = BufReader::new(peer.try_clone().unwrap());
        let read = |input: &mut BufReader<std::net::TcpStream>| {
            let mut line = String::new();
            assert!(input.read_line(&mut line).unwrap() > 0);
            line.trim_end().to_owned()
        };
        let command = |input: &mut BufReader<std::net::TcpStream>, expected: &str| {
            let line = read(input);
            let mut fields = line.splitn(3, ' ');
            assert_eq!(fields.next(), Some("cockpit"));
            let id = fields.next().unwrap().parse::<u32>().unwrap();
            assert_eq!(fields.next(), Some(expected));
            id
        };
        let mut state = wire();
        state["configurationRouteEvents"] = json!(true);
        let snapshot = |peer: &mut std::net::TcpStream, id, state: &Value| {
            writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state})).unwrap();
        };
        let event = |peer: &mut std::net::TcpStream, sequence: &str| {
            writeln!(peer, "{}", json!({"publicationStream":"change","epoch":10,"sequence":sequence})).unwrap();
        };
        snapshot(&mut peer, command(&mut input, "snapshot"), &state);
        assert_eq!(read(&mut input), "panel-stream");
        writeln!(peer, "{}", json!({"panelStream":"reset"})).unwrap();
        event(&mut peer, "1");
        ready_tx.send(()).unwrap();

        let readback = if action {
            format!("10 action {MODULE} 20 state")
        } else {
            snapshot(&mut peer, command(&mut input, "snapshot"), &state);
            format!("10 config {MODULE} 20 get_active")
        };
        let request = command(&mut input, &readback);
        if action { event(&mut peer, "2"); }
        writeln!(peer, "{}", json!({"id":request,"ok":false,"error":"configuration_route_not_ready","state":state})).unwrap();
        if !action {
            input.get_ref().set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            let mut line = String::new();
            let error = input.read_line(&mut line).expect_err("the route waiter queried while the event stream was quiet");
            assert!(matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock), "{error}");
            input.get_ref().set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            event(&mut peer, "2");
        }
        if restarted { state["modules"][0]["boot"] = json!(21); }
        snapshot(&mut peer, command(&mut input, "snapshot"), &state);
        if !restarted {
            let request = command(&mut input, &readback);
            let mut reply = json!({"id":request,"ok":true,"state":state});
            if action {
                reply["targetAction"] = json!({"ok":true});
            } else {
                reply["targetConfig"] = json!({"generation":2,"savedGeneration":2,"owner":17,
                    "dirty":false,"pendingApply":false,"view":"active","valid":true,
                    "config":{"name":"Saved pedals","expanderMask":0,"buttons":[[0,0,0]],
                        "axes":[],"hats":[],"encoders":[],"selectors":[]}});
            }
            writeln!(peer, "{reply}").unwrap();
        }
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });

    let mut timing = Timing::default();
    timing.command = Duration::from_millis(650);
    timing.reconnect = Duration::from_secs(2);
    let workspace = Arc::new(workspace::Cache::new(empty_state()));
    workspace.set_owned(true);
    let mut gateway = Gateway::new(
        Box::new(DuplexFactory(Some(parts))), timing, Arc::new(AtomicBool::new(false)),
    );
    gateway.workspace = Some(workspace.clone());
    gateway.open(port(), Some(UID.into()), Instant::now() + Duration::from_secs(2)).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while workspace.publication.current().is_none() {
        assert!(Instant::now() < deadline, "missing initial publication record");
        thread::sleep(Duration::from_millis(1));
    }

    if action {
        let mut route = action_route(&gateway.state, MODULE).unwrap();
        assert_eq!(gateway.action_command(MODULE, &mut route, "state").unwrap(), json!({"ok":true}));
    } else {
        let result = gateway.read_active(MODULE, Some(&json!(20)));
        if restarted {
            assert!(result.unwrap_err().contains("Module restarted"));
        } else {
            result.unwrap();
            assert_eq!(gateway.active_configs[MODULE].2["generation"], 2);
        }
    }
    done_tx.send(()).unwrap();
    device.join().unwrap();
}

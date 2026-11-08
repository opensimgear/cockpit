use super::*;
use std::io::{BufRead, BufReader};

#[test]
fn activation_waits_for_target_generation_then_the_replacement_route() {
    exercise_activation(Scenario::RouteNotReady);
}

#[test]
fn activation_observes_generation_change_before_the_pending_reply() {
    exercise_activation(Scenario::EarlyChange);
}

#[test]
fn activation_refuses_readback_from_a_restarted_peripheral() {
    exercise_activation(Scenario::Restarted);
}

#[test]
fn activation_discards_a_readback_from_the_previous_generation() {
    exercise_activation(Scenario::StaleReadback);
}

#[test]
fn activation_bounds_readback_by_the_original_deadline() {
    exercise_activation(Scenario::StalledReadback);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    RouteNotReady,
    EarlyChange,
    Restarted,
    StaleReadback,
    StalledReadback,
}

fn exercise_activation(scenario: Scenario) {
    use crate::transport::duplex::tcp_fixture;

    let (parts, mut peer) = tcp_fixture();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let change_before_reply = scenario != Scenario::RouteNotReady;
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let device = thread::spawn(move || {
        let mut input = BufReader::new(peer.try_clone().unwrap());
        let read = |input: &mut BufReader<std::net::TcpStream>| {
            let mut line = String::new();
            assert!(input.read_line(&mut line).unwrap() > 0);
            line.trim_end().to_owned()
        };
        let request = |input: &mut BufReader<std::net::TcpStream>, expected: &str| {
            let line = read(input);
            let mut fields = line.splitn(3, ' ');
            assert_eq!(fields.next(), Some("cockpit"));
            let id = fields.next().unwrap().parse::<u32>().unwrap();
            assert_eq!(fields.next(), Some(expected));
            id
        };
        let quiet = |input: &mut BufReader<std::net::TcpStream>| {
            input.get_ref().set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            let mut line = String::new();
            let error = input.read_line(&mut line).expect_err("unexpected request during a quiet publication wait");
            assert!(matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock), "{error}");
            input.get_ref().set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        };
        let mut state = wire();
        state["configurationRouteEvents"] = json!(true);
        state["modules"][0]["configurationGeneration"] = json!(1);
        let snapshot = |peer: &mut std::net::TcpStream, id, state: &Value| {
            writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state})).unwrap();
        };
        let event = |peer: &mut std::net::TcpStream, sequence: u64| {
            writeln!(peer, "{}", json!({"publicationStream":"change","epoch":10,"sequence":sequence.to_string()})).unwrap();
        };
        let config = |generation, pending| json!({"generation":generation,"savedGeneration":2,"owner":17,
            "dirty":false,"pendingApply":pending,"view":"active","valid":true,
            "config":{"name":"Saved pedals","expanderMask":0,"buttons":[[0,0,0]],
                "axes":[],"hats":[],"encoders":[],"selectors":[]}});
        snapshot(&mut peer, request(&mut input, "snapshot"), &state);
        assert_eq!(read(&mut input), "panel-stream");
        writeln!(peer, "{}", json!({"panelStream":"reset"})).unwrap();
        event(&mut peer, 1);
        ready_tx.send(()).unwrap();

        let mutation = request(&mut input, &format!("10 config {MODULE} 20 apply_save 17"));
        if change_before_reply { event(&mut peer, 2); }
        writeln!(peer, "{}", json!({"id":mutation,"ok":true,"state":state,"targetConfig":config(1,true)})).unwrap();
        if !change_before_reply {
            quiet(&mut input);
            event(&mut peer, 2);
            snapshot(&mut peer, request(&mut input, "snapshot"), &state);
            quiet(&mut input);
            event(&mut peer, 3);
        }
        state["modules"][0]["configurationGeneration"] = json!(2);
        state["modules"][0]["lease"] = json!(12);
        if scenario == Scenario::Restarted { state["modules"][0]["boot"] = json!(21); }
        snapshot(&mut peer, request(&mut input, "snapshot"), &state);
        if scenario != Scenario::Restarted {
            let get_active = format!("10 config {MODULE} 20 get_active");
            if !change_before_reply {
                let id = request(&mut input, &get_active);
                writeln!(peer, "{}", json!({"id":id,"ok":false,"error":"configuration_route_not_ready","state":state})).unwrap();
                quiet(&mut input);
                event(&mut peer, 4);
                snapshot(&mut peer, request(&mut input, "snapshot"), &state);
            }
            let id = request(&mut input, &get_active);
            if scenario != Scenario::StalledReadback {
                let generation = if scenario == Scenario::StaleReadback { 1 } else { 2 };
                writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state,"targetConfig":config(generation,false)})).unwrap();
            }
        }
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });

    let mut timing = Timing::default();
    timing.command = Duration::from_millis(650);
    timing.reconnect = if scenario == Scenario::StalledReadback {
        Duration::from_millis(250)
    } else {
        Duration::from_secs(2)
    };
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

    let started = Instant::now();
    let result = gateway.config_commit(MODULE, "apply_save 17", None, true);
    match scenario {
        Scenario::Restarted => assert!(result.unwrap_err().contains("Module restarted")),
        Scenario::StaleReadback => {
            assert!(result.unwrap_err().contains("could not confirm the activated configuration generation"));
            assert!(!gateway.configs.contains_key(MODULE));
            assert!(!gateway.active_configs.contains_key(MODULE));
        }
        Scenario::StalledReadback => {
            assert!(result.is_err());
            assert!(started.elapsed() < Duration::from_millis(550));
        }
        _ => {
            result.unwrap();
            assert_eq!(gateway.configs[MODULE]["generation"], 2);
            assert_eq!(gateway.configs[MODULE]["pendingApply"], false);
        }
    }
    done_tx.send(()).unwrap();
    device.join().unwrap();
}

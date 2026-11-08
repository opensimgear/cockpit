use super::*;

#[test]
fn automatic_layout_waits_for_publication_change_and_accepts_terminal_snapshot() {
    exercise_layout(false, false);
}

#[test]
fn automatic_layout_observes_publication_before_the_pending_command_reply() {
    exercise_layout(true, false);
}

#[test]
fn automatic_layout_revalidates_preflight_if_publication_changes_during_readback() {
    exercise_layout(false, true);
}

fn exercise_layout(event_before_reply: bool, superseded_preflight: bool) {
    use crate::transport::duplex::tcp_fixture;
    use std::io::{BufRead, BufReader};

    let (parts, mut peer) = tcp_fixture();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (pending_tx, pending_rx) = mpsc::channel();
    let (change_tx, change_rx) = mpsc::channel();
    let (terminal_tx, terminal_rx) = mpsc::channel();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

    let device = thread::spawn(move || {
        let mut input = BufReader::new(peer.try_clone().unwrap());
        let read_line = |input: &mut BufReader<std::net::TcpStream>| {
            let mut line = String::new();
            input.read_line(&mut line).unwrap();
            line.trim_end().to_owned()
        };
        let read_snapshot = |input: &mut BufReader<std::net::TcpStream>| {
            let line = read_line(input);
            let fields: Vec<_> = line.split_ascii_whitespace().collect();
            assert_eq!(fields.len(), 3, "unexpected request: {line}");
            assert_eq!((fields[0], fields[2]), ("cockpit", "snapshot"));
            fields[1].parse::<u32>().unwrap()
        };
        let reply = |peer: &mut std::net::TcpStream, id: u32, state: Value| {
            writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state})).unwrap();
        };
        let event = |peer: &mut std::net::TcpStream, sequence: u64| {
            writeln!(peer, "{}", json!({"publicationStream":"change","epoch":10,"sequence":sequence.to_string()})).unwrap();
        };

        reply(&mut peer, read_snapshot(&mut input), wire());
        assert_eq!(read_line(&mut input), "panel-stream");
        writeln!(peer, "{}", json!({"panelStream":"reset"})).unwrap();
        event(&mut peer, 1);
        ready_tx.send(()).unwrap();

        // The request confirms a current preflight snapshot, then sends Disable.
        let preflight = read_snapshot(&mut input);
        let sequence = if superseded_preflight {
            event(&mut peer, 2);
            reply(&mut peer, preflight, wire());
            reply(&mut peer, read_snapshot(&mut input), wire());
            2
        } else {
            reply(&mut peer, preflight, wire());
            1
        };
        let mutation = read_line(&mut input);
        let fields: Vec<_> = mutation.split_ascii_whitespace().collect();
        assert_eq!(fields.len(), 5, "unexpected mutation: {mutation}");
        assert_eq!(fields[0], "cockpit");
        assert_eq!(&fields[2..], ["10", "disable", MODULE]);
        let mutation_id = fields[1].parse::<u32>().unwrap();
        let mut pending = wire();
        pending["layoutPending"] = json!(true);
        if event_before_reply { event(&mut peer, sequence + 1); }
        reply(&mut peer, mutation_id, pending);
        pending_tx.send(()).unwrap();
        change_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        if !event_before_reply {
            // The feed is live and the device is quiet. No poll may run.
            input.get_ref().set_read_timeout(Some(Duration::from_millis(90))).unwrap();
            let mut unexpected = String::new();
            match input.read_line(&mut unexpected) {
                Err(error) if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {}
                Ok(0) => panic!("serial stream ended while layout was pending"),
                Ok(_) => panic!("unexpected command while publication feed was quiet: {unexpected}"),
                Err(error) => panic!("unexpected serial read error: {error}"),
            }
            input.get_ref().set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            event(&mut peer, sequence + 1);
        }

        // A publication change starts one readback. The next change lands during it.
        let obsolete_id = read_snapshot(&mut input);
        event(&mut peer, sequence + 2);
        reply(&mut peer, obsolete_id, wire());

        // Discard the obsolete response and read once against the latest event.
        let snapshot_id = read_snapshot(&mut input);
        let mut terminal = wire();
        terminal["layoutPending"] = json!(false);
        terminal["modules"][0]["enabled"] = json!(false);
        terminal["modules"][0]["published"] = json!(false);
        terminal["modules"][0]["descriptorIndex"] = json!(-1);
        terminal["modules"][0]["reportId"] = Value::Null;
        terminal["modules"][0]["report"] = json!("00");
        terminal["modules"][0]["profileMatch"] = json!(false);
        terminal["modules"][0]["hasInput"] = json!(false);
        terminal["modules"][0]["configuredEnabled"] = json!(false);
        terminal["modules"][0]["configuredDescriptorIndex"] = json!(-1);
        terminal["modules"][0]["publicationReportId"] = json!(0);
        reply(&mut peer, snapshot_id, terminal);
        terminal_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });

    let mut timing = Timing::default();
    timing.command = Duration::from_secs(1);
    timing.reconnect = Duration::from_secs(2);
    timing.settle = Duration::ZERO;
    timing.heartbeat = Duration::from_secs(3600);
    let workspace = Arc::new(workspace::Cache::new(empty_state()));
    workspace.set_owned(true);
    let mut gateway = Gateway::new(
        Box::new(DuplexFactory(Some(parts))), timing, Arc::new(AtomicBool::new(false)),
    );
    gateway.workspace = Some(workspace.clone());
    gateway.open(port(), Some(UID.into()), Instant::now() + Duration::from_secs(2)).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let feed_deadline = Instant::now() + Duration::from_secs(1);
    while workspace.publication.current().is_none() {
        assert!(Instant::now() < feed_deadline, "the initial publication event was not accepted");
        thread::sleep(Duration::from_millis(1));
    }

    let mutation = thread::spawn(move || gateway.request(json!({
        "command":"disable", "moduleId":MODULE, "boot":20, "clientId":"layout-publication-test",
    })));
    if pending_rx.recv_timeout(Duration::from_secs(2)).is_err() {
        panic!("Disable never reached pending layout: {}", mutation.join().unwrap());
    }
    change_tx.send(()).unwrap();
    let result = mutation.join().unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["state"]["layoutPending"], false);
    let module = &result["state"]["modules"][0];
    assert_eq!(module["id"], MODULE);
    assert_eq!(module["enabled"], false);
    assert_eq!(module["published"], false);

    terminal_tx.send(()).unwrap();
    device.join().unwrap();
}

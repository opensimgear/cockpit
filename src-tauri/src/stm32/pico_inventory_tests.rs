use super::*;
use crate::transport::duplex::tcp_fixture;
use std::io::{BufRead, BufReader, Write};

#[test]
fn pico_inventory_wakes_on_change_and_renews_watchdog_without_reading_state() {
    let (parts, mut peer) = tcp_fixture();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let (changed, change) = mpsc::channel();
    let (inspected, inspect) = mpsc::channel();
    let device = thread::spawn(move || {
        let mut lines = BufReader::new(peer.try_clone().unwrap()).lines();
        let state = tests::pico_state();
        let epoch = state["epoch"].as_u64().unwrap();
        let mut debug = tests::debug_state();
        let snapshot = |line: String| {
            let args: Vec<_> = line.split_ascii_whitespace().collect();
            assert_eq!(args.len(), 3, "{line}");
            assert_eq!((args[0], args[2]), ("cockpit", "snapshot"));
            args[1].parse::<u32>().unwrap()
        };
        let id = snapshot(lines.next().unwrap().unwrap());
        writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state,"debug":debug})).unwrap();
        assert_eq!(lines.next().unwrap().unwrap(), "panel-stream");
        writeln!(peer, "{}", json!({"publicationStream":"change","epoch":epoch,"sequence":"1"})).unwrap();
        let id = snapshot(lines.next().unwrap().unwrap());
        writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state,"debug":debug})).unwrap();

        // The first two idle deadlines may only send a compact safety renewal.
        for _ in 0..2 {
            let line = lines.next().unwrap().unwrap();
            let args: Vec<_> = line.split_ascii_whitespace().collect();
            assert_eq!(args.len(), 3, "{line}");
            assert_eq!(args[0], "debug-watchdog");
            assert_eq!(args[2].parse::<u64>().unwrap(), epoch);
            let id = args[1].parse::<u32>().unwrap();
            writeln!(peer, "{}", json!({"id":id,"ok":true})).unwrap();
        }
        writeln!(peer, "{}", json!({"publicationStream":"inventory","epoch":epoch,"sequence":"2"})).unwrap();
        let id = snapshot(lines.next().unwrap().unwrap());
        debug["devices"][0]["axes"][0] = json!(2048);
        writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state,"debug":debug})).unwrap();
        changed.send(()).unwrap();
        inspect.recv_timeout(Duration::from_secs(3)).unwrap();
    });
    let service = crate::pico::stream_fixture(parts);
    let wake = Arc::new(osg_streams::NativeWake::default());
    service.watch_workspace(wake.clone());
    service.set_workspace_owned(true);
    let before_connect = wake.observed();
    let connected = service.request(json!({"command":"connect","port":"COM42","clientId":"pico-inventory-test"}));
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(connected["state"]["protocolVersion"], 3);
    assert!(wake.observed() > before_connect);
    change.recv_timeout(Duration::from_secs(10)).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if service.workspace_snapshot().0["debug"]["devices"][0]["axes"][0] == 2048 { break; }
        assert!(Instant::now() < until, "Pico workspace never received the changed axis");
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(service.try_cached_state().unwrap()["protocolVersion"], 3);
    assert!(service.renew_workspace_client("pico-inventory-test").unwrap().is_empty());
    let disconnected = service.request(json!({"command":"disconnect","clientId":"pico-inventory-test"}));
    assert_eq!(disconnected["ok"], true, "{disconnected}");
    assert_eq!(service.workspace_snapshot().0["connected"], false);
    inspected.send(()).unwrap();
    device.join().unwrap();
}

use super::*;
use osg_profiles::{
    Binding, Direction, Endpoint, Port, PortContract, PortValue, ProcessingNode, Resource,
    ResourceKind,
};

#[test]
fn qualification_duration_preserves_both_stop_edges_within_the_capture_window() {
    assert_eq!(burst_duration(None, false), Ok(192));
    assert_eq!(burst_duration(Some(4_800), false), Ok(4_800));
    for duration in [0, 176, 200, 4_816, u64::MAX] {
        assert!(burst_duration(Some(duration), false).is_err());
    }
    assert_eq!(burst_duration(Some(1_800_000), true), Ok(1_800_000));
    assert!(burst_duration(Some(1_800_016), true).is_err());
    assert!(burst_duration(Some(1_799_999), true).is_err());
}

#[test]
fn trace_records_keep_little_endian_timing_and_never_replace_an_existing_capture() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(TRACE_FILES[0]);
    let mut capture = Capture::open(Some(directory.path()), 0, BURST_MS).unwrap();
    capture.record(Submission {
        at_ns: 4_294_967_296,
        dispatch_at_ns: 4_294_967_297,
        capture_before_ns: 4_294_967_298,
        capture_after_ns: 4_294_967_299,
        admitted_at_ns: 4_294_967_300,
        round_trip_ms: 0.0,
        ingress_wait_ms: 0.0,
        ingress_work_ms: 0.0,
        batch_index: 3,
        sequence: 8,
        pending_frames: 4,
        pending_owned_bytes: 0x0102_0304,
        value: true,
    }).unwrap();
    assert_eq!(capture.finish().unwrap(), json!(1));
    assert!(Capture::open(Some(directory.path()), 0, BURST_MS).is_err());
    let trace = std::fs::read(path).unwrap();
    assert_eq!(&trace[..8], b"OSGDBG02");
    assert_eq!(&trace[8..], [
        0, 0, 0, 0, 1, 0, 0, 0,
        1, 0, 0, 0, 1, 0, 0, 0,
        2, 0, 0, 0, 1, 0, 0, 0,
        3, 0, 0, 0, 1, 0, 0, 0,
        4, 0, 0, 0, 1, 0, 0, 0,
        8, 0, 0, 0, 4, 0, 0, 0,
        4, 3, 2, 1, 3, 0, 0, 0,
    ]);
}

#[test]
fn capture_bracket_uses_the_exact_host_clock_and_rejects_a_future_floor() {
    let qpc = Clock::new().unwrap();
    let controllers = osg_controller_input::ControllerService::new(None, false);
    let source = controllers.capture_clock();
    let floor = source.now_us();
    let capture = qpc.capture(source, floor).unwrap();
    assert!(capture.before_ns <= capture.after_ns);
    assert!(capture.native_us >= floor && capture.native_us <= source.now_us());
    assert!(qpc.capture(source, u64::MAX).is_err());
}

#[test]
fn qualification_requires_explicit_enable_and_the_current_neutral_owner() {
    let manager = GatewayManager::mcp_test_manager();
    if !matches!(
        std::env::var("OSG_REACTIVE_QUALIFICATION").as_deref(),
        Ok("1")
    ) {
        let result = run(&manager, Value::Null);
        assert_eq!(
            result["error"],
            "The desktop qualification source is disabled."
        );
        manager.shutdown();
        return;
    }

    let mut document = osg_profiles::default_document();
    let port = |id: &str, direction| Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract: PortContract::State {
            schema: osg_mapping::signal::ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(PortValue::Boolean(false)),
        },
    };
    let nodes = ["debug", "debug-peer"];
    for node in nodes {
        let sink = format!("{node}-sink");
        document.resources.push(Resource {
            id: node.into(),
            name: node.into(),
            kind: ResourceKind::DebugController,
            required: false,
            ports: vec![port("button-1", Direction::Output)],
        });
        document.processing_nodes.push(ProcessingNode {
            id: sink.clone(),
            name: sink.clone(),
            operation: osg_mapping::program::Operation::Identity {},
            draft: None,
            inputs: vec![port("in", Direction::Input)],
            outputs: vec![port("out", Direction::Output)],
        });
        document.bindings.push(Binding {
            id: format!("{node}-wire"),
            from: Endpoint {
                node_id: node.into(),
                port_id: "button-1".into(),
            },
            to: Endpoint {
                node_id: sink,
                port_id: "in".into(),
            },
        });
    }
    let saved = manager.request(json!({
        "command":"profile_editor_save", "clientId":CLIENT_ID, "document":document,
        "expectedRevision":1, "assignments":[]
    }));
    assert_eq!(saved["ok"], true, "{saved}");
    let applied = manager.request(json!({
        "command":"profile_apply", "clientId":CLIENT_ID, "id":document.id, "expectedRevision":2
    }));
    assert_eq!(applied["ok"], true, "{applied}");
    let owner = &applied["state"]["profiles"]["profileSession"]["owner"];
    let clock = Clock::new().unwrap();
    let request = || {
        json!({
            "command":"qualification_debug_burst", "owner":owner, "nodes":nodes,
            "startNs":(clock.now().unwrap() + 50 * NS_PER_MS).to_string()
        })
    };
    // This test manager has no native profile worker.
    let result = thread::scope(|scope| {
        let request = request();
        let burst = scope.spawn(|| run(&manager, request));
        while !burst.is_finished() {
            manager.poll_qualification_test_profile().unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        burst.join().unwrap()
    });
    assert_eq!(result["ok"], true, "{result}");
    for (index, count) in [193, 25].into_iter().enumerate() {
        let lane = result["state"]["qualificationBurst"][index]
            .as_array()
            .unwrap();
        assert_eq!(lane.len(), count);
        assert_eq!(result["state"]["captureToReducerBatchMs"][index]["count"], count);
        assert!(lane
            .iter()
            .enumerate()
            .all(|(edge, row)| row["value"] == (edge % 2 == 0)));
    }
    let held = run(&manager, request());
    assert_eq!(
        held["error"],
        "Both qualification lanes require an applied neutral debug button."
    );
    let cleanup = Burst {
        manager: &manager,
        request: serde_json::from_value(request()).unwrap(),
        clock: Clock(0),
        duration: BURST_MS,
        expires: 0,
        streams: std::array::from_fn(|_| uuid::Uuid::new_v4().to_string()),
        cancelled: AtomicBool::new(true),
        attempted: std::array::from_fn(|_| AtomicU64::new(1)),
    };
    cleanup.release();
    for node in nodes {
        let status = manager.request(json!({
            "command":"profile_debug_controller_status", "clientId":CLIENT_ID,
            "owner":owner, "nodeId":node,
        }));
        assert_eq!(status["state"]["debugController"]["values"][0]["value"], false);
    }
    let stopped = manager.request(json!({"command":"profile_stop", "clientId":CLIENT_ID}));
    assert_eq!(stopped["ok"], true, "{stopped}");
    assert_eq!(run(&manager, request())["ok"], false);
    manager.shutdown();
}

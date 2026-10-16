use super::*;
fn row(instance: &str, companion: &str, p: &Publication) -> Value {
    json!({"instanceId":instance,"companionId":companion,"publicationHash":p.publication_hash,"capabilityFingerprint":p.capability_fingerprint})
}
fn empty(instance: &str) -> Value {
    json!({"instanceId":instance,"companionId":null,"publicationHash":null,"capabilityFingerprint":null})
}
fn snapshot(rows: Vec<Value>) -> Value {
    json!({"schemaVersion":1,"assignments":rows})
}
#[test]
fn compare_swap_replaces_only_expected_authority_and_never_arms_or_mutates_other_instances() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    let alpha = paired(&h, "alpha");
    let beta = paired(&h, "beta");
    let first = publish(&h, fixture());
    let second = publish(
        &h,
        mutated(|d| d["ui"]["title"] = json!("Frozen revised panel")),
    );
    assign(&h, "alpha", "left", &first.publication_hash);
    assign(&h, "beta", "right", &first.publication_hash);
    let left = connect(&h, "left-connection", &alpha, "left");
    let right = connect(&h, "right-connection", &beta, "right");
    baseline(&h, "left-connection", &left, &first);
    baseline(&h, "right-connection", &right, &first);
    let expected = h
        .assignment_snapshot(&["missing".into(), "left".into()])
        .unwrap();
    assert_eq!(expected["assignments"][1], empty("missing"));
    let desired = snapshot(vec![
        row("left", "alpha", &second),
        row("missing", "beta", &first),
    ]);
    let before = sink.events.lock().unwrap().len();
    h.preflight_assignments(&desired).unwrap();
    assert_eq!(sink.events.lock().unwrap().len(), before);
    let changed = h.compare_assignments(&expected, &desired).unwrap();
    assert_eq!(changed, desired);
    assert_eq!(
        sink.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(kind, _)| kind == "loss")
            .count(),
        1
    );
    assert!(!sink.owners.lock().unwrap().contains_key("left"));
    assert!(sink.owners.lock().unwrap().contains_key("right"));
    assert!(!sink.owners.lock().unwrap().contains_key("missing"));
    assert_eq!(
        wire(&h, "right-connection", input(&right, &first, "1"))["ok"],
        true
    );
    assert_eq!(
        h.compare_assignments(&desired, &expected).unwrap(),
        expected
    );
    assert!(!sink.owners.lock().unwrap().contains_key("left"));
    assert_eq!(
        wire(&h, "left-connection", input(&left, &first, "1"))["ok"],
        false
    );
    assert_eq!(
        h.assignment_snapshot(&["right".into()]).unwrap(),
        snapshot(vec![row("right", "beta", &first)])
    );
    let unchanged = h.assignment_snapshot(&["right".into()]).unwrap();
    let before = sink.events.lock().unwrap().len();
    h.compare_assignments(&unchanged, &unchanged).unwrap();
    assert_eq!(sink.events.lock().unwrap().len(), before);
}
#[test]
fn malformed_capture_or_missing_authority_is_rejected_before_any_side_effect() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    paired(&h, "peer");
    let p = publish(&h, fixture());
    let desired = snapshot(vec![row("instance", "peer", &p)]);
    let expected = h.assignment_snapshot(&["instance".into()]).unwrap();
    let before = sink.events.lock().unwrap().len();
    for fault in 0..12 {
        let mut bad = desired.clone();
        match fault {
            0 => bad["schemaVersion"] = json!(2),
            1 => bad["assignments"] = json!([]),
            2 => bad["assignments"][0]["instanceId"] = json!("bad/id"),
            3 => bad["assignments"]
                .as_array_mut()
                .unwrap()
                .push(row("instance", "peer", &p)),
            4 => bad["assignments"][0]["companionId"] = Value::Null,
            5 => bad["assignments"][0]["publicationHash"] = json!("invalid"),
            6 => bad["assignments"][0]["companionId"] = json!("revoked"),
            7 => {
                bad["assignments"][0]["publicationHash"] =
                    json!(format!("sha256:{}", "b".repeat(64)))
            }
            8 => {
                bad["assignments"][0]["capabilityFingerprint"] =
                    json!(format!("sha256:{}", "c".repeat(64)))
            }
            9 => bad["extra"] = json!(true),
            10 => {
                bad["assignments"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("capabilityFingerprint");
            }
            _ => bad["assignments"][0]["companionId"] = json!(3),
        };
        assert!(h.preflight_assignments(&bad).is_err(), "fault{fault}");
        assert!(
            h.compare_assignments(&expected, &bad).is_err(),
            "fault{fault}"
        );
        assert_eq!(sink.events.lock().unwrap().len(), before);
        assert_eq!(
            h.assignment_snapshot(&["instance".into()]).unwrap(),
            expected
        );
    }
    assert!(h.assignment_snapshot(&[]).is_err());
    assert!(h.assignment_snapshot(&["x".into(), "x".into()]).is_err());
    assert!(h.assignment_snapshot(&["bad/id".into()]).is_err());
    assert!(h
        .assignment_snapshot(&(0..9).map(|n| format!("i{n}")).collect::<Vec<_>>())
        .is_err());
    assert!(h
        .compare_assignments(&expected, &snapshot(vec![empty("different")]))
        .is_err());
    assert!(h
        .preflight_assignments(&json!({"huge":"x".repeat(8193)}))
        .is_err());
    assert!(h
        .compare_assignments(&json!({"huge":"x".repeat(8193)}), &desired)
        .is_err());
}
#[test]
fn external_changes_and_failed_durable_commit_preserve_authority_and_leave_changed_sources_disarmed(
) {
    let memory = Arc::new(Memory::default());
    let sink = Arc::new(Sink::default());
    let h = Hub::with_storage(memory.clone(), sink.clone()).unwrap();
    let secret = paired(&h, "peer");
    let p = publish(&h, fixture());
    let revised = publish(&h, mutated(|d| d["ui"]["title"] = json!("Revised")));
    assign(&h, "peer", "instance", &p.publication_hash);
    let open = connect(&h, "connection", &secret, "instance");
    baseline(&h, "connection", &open, &p);
    let expected = h.assignment_snapshot(&["instance".into()]).unwrap();
    let desired = snapshot(vec![row("instance", "peer", &revised)]);
    let bytes = memory.value.lock().unwrap().clone();
    memory.fail.store(true, Ordering::SeqCst);
    assert!(h
        .compare_assignments(&expected, &desired)
        .unwrap_err()
        .contains("not committed"));
    assert_eq!(memory.value.lock().unwrap().clone(), bytes);
    assert_eq!(
        h.assignment_snapshot(&["instance".into()]).unwrap(),
        expected
    );
    assert!(sink.owners.lock().unwrap().is_empty());
    assert_eq!(
        sink.defs.lock().unwrap()["instance"].publication_hash,
        p.publication_hash
    );
    memory.fail.store(false, Ordering::SeqCst);
    h.compare_assignments(&expected, &desired).unwrap();
    assert!(h
        .compare_assignments(&expected, &desired)
        .unwrap_err()
        .contains("externally"));
    assert_eq!(
        h.assignment_snapshot(&["instance".into()]).unwrap(),
        desired
    );
    let restored = Hub::with_storage(memory, Arc::new(Sink::default())).unwrap();
    assert!(restored.assignment_snapshot(&["instance".into()]).is_err());
    publish(&restored, mutated(|d| d["ui"]["title"] = json!("Revised")));
    assert_eq!(
        restored.assignment_snapshot(&["instance".into()]).unwrap(),
        desired
    );
    assert_eq!(
        restored
            .command(&json!({"command":"companion_status"}))
            .unwrap()["instances"][0]["armed"],
        false
    );
}
#[test]
fn every_desired_row_is_preflighted_before_source_release_and_total_instances_remain_bounded() {
    let sink = Arc::new(Sink::default());
    let h = Hub::new("host".into(), sink.clone());
    paired(&h, "peer");
    let p = publish(&h, fixture());
    for i in 0..32 {
        assign(&h, "peer", &format!("i{i}"), &p.publication_hash);
    }
    let expected = h.assignment_snapshot(&["extra".into()]).unwrap();
    let desired = snapshot(vec![row("extra", "peer", &p)]);
    assert!(h
        .preflight_assignments(&desired)
        .unwrap_err()
        .contains("budget"));
    assert!(h.compare_assignments(&expected, &desired).is_err());
    let desired = snapshot(vec![empty("i0"), row("extra", "peer", &p)]);
    h.preflight_assignments(&desired).unwrap();
    let expected = h
        .assignment_snapshot(&["i0".into(), "extra".into()])
        .unwrap();
    h.compare_assignments(&expected, &desired).unwrap();
    assert_eq!(
        h.command(&json!({"command":"companion_status"})).unwrap()["instances"]
            .as_array()
            .unwrap()
            .len(),
        32
    );
}
struct RejectRegister {
    sink: Sink,
    remaining: std::sync::atomic::AtomicUsize,
}
impl SourceSink for RejectRegister {
    fn register(&self, d: SourceDefinition) -> Result<(), String> {
        let n = self.remaining.load(Ordering::SeqCst);
        if n > 0 && self.remaining.fetch_sub(1, Ordering::SeqCst) == 1 {
            return Err("descriptor rejected".into());
        }
        self.sink.register(d)
    }
    fn baseline(&self, t: &SourceToken, s: u64, c: &[SourceChange]) -> Result<(), String> {
        self.sink.baseline(t, s, c)
    }
    fn changes(&self, t: &SourceToken, s: u64, c: &[SourceChange]) -> Result<(), String> {
        self.sink.changes(t, s, c)
    }
    fn loss(&self, t: &SourceToken) -> Result<(), String> {
        self.sink.loss(t)
    }
}
#[test]
fn multi_instance_descriptor_failure_never_partially_commits_assignment_authority() {
    let sink = Arc::new(RejectRegister {
        sink: Sink::default(),
        remaining: std::sync::atomic::AtomicUsize::new(0),
    });
    let h = Hub::new("host".into(), sink.clone());
    paired(&h, "peer");
    let first = publish(&h, fixture());
    let second = publish(&h, mutated(|d| d["ui"]["title"] = json!("Second")));
    assign(&h, "peer", "a", &first.publication_hash);
    assign(&h, "peer", "b", &first.publication_hash);
    let before = h.assignment_snapshot(&["a".into(), "b".into()]).unwrap();
    let desired = snapshot(vec![row("a", "peer", &second), row("b", "peer", &second)]);
    sink.remaining.store(2, Ordering::SeqCst);
    assert!(h.compare_assignments(&before, &desired).is_err());
    assert_eq!(
        h.assignment_snapshot(&["a".into(), "b".into()]).unwrap(),
        before
    );
    assert!(sink
        .sink
        .defs
        .lock()
        .unwrap()
        .values()
        .all(|d| d.publication_hash == first.publication_hash));
}

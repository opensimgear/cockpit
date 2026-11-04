use super::*;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

static NEXT_PROCESS_ID: AtomicU32 = AtomicU32::new(20_000);

fn connection() -> (UdpSocket, Value) {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut value = defaults(12);
    value["listenPort"] = json!(0);
    value["port"] = json!(server.local_addr().unwrap().port());
    value["processIdentity"] = json!({
        "pid": NEXT_PROCESS_ID.fetch_add(1, Ordering::Relaxed),
        "path": "C:/X-Plane/X-Plane.exe",
        "epoch": 1
    });
    (server, value)
}

fn catalogue() -> BTreeMap<String, datarefs::Channel> {
    legacy_channels()
        .into_iter()
        .map(|channel| (channel.id.clone(), channel))
        .collect()
}

fn create_owner(connection: &Value) -> Result<Owner, String> {
    Owner::start(connection, None, catalogue())
}

fn open(connection: &Value, outputs: Vec<Value>) -> Result<Source, String> {
    Source::open_with(connection, outputs, || create_owner(connection))
}

impl Source {
    pub(crate) fn inspector_fixture() -> (Self, impl Fn(u16)) {
        let (socket, connection) = connection();
        let mut source = open(&connection, vec![]).unwrap();
        let snapshot = source.native_snapshot().unwrap();
        let shared = source.owner.shared.clone();
        (source, move |counter| {
            let _socket = &socket;
            let mut snapshot = snapshot.clone();
            snapshot.counter = counter;
            shared.publish(snapshot, Instant::now()).unwrap();
        })
    }
}

#[test]
fn registry_shares_one_factory_and_socket_for_concurrent_identical_configs() {
    let (_server, connection) = connection();
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let mut threads = Vec::new();
    for _ in 0..8 {
        let connection = connection.clone();
        let calls = calls.clone();
        let barrier = barrier.clone();
        threads.push(thread::spawn(move || {
            barrier.wait();
            Source::open_with(&connection, vec![], || {
                calls.fetch_add(1, Ordering::SeqCst);
                create_owner(&connection)
            })
            .unwrap()
        }));
    }
    let sources: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(sources
        .iter()
        .all(|source| Arc::ptr_eq(&source.owner, &sources[0].owner)));
    assert!(sources
        .iter()
        .all(|source| source.address_test() == sources[0].address_test()));
}

#[test]
fn a_different_captured_setup_is_rejected_before_its_factory_runs() {
    let (_server, first_connection) = connection();
    let first = open(&first_connection, vec![]).unwrap();
    let mut replacement = first_connection.clone();
    replacement["freshnessMs"] = json!(1600);
    let calls = AtomicUsize::new(0);
    let error = Source::open_with(&replacement, vec![], || {
        calls.fetch_add(1, Ordering::SeqCst);
        create_owner(&replacement)
    })
    .err()
    .unwrap();
    assert!(error.contains("different captured setup"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(first);
}

#[test]
fn failed_factory_leaves_no_cached_owner_or_reader_lease() {
    let (_server, connection) = connection();
    let calls = AtomicUsize::new(0);
    let error = Source::open_with(&connection, vec![], || {
        calls.fetch_add(1, Ordering::SeqCst);
        Err("synthetic owner setup failure".into())
    })
    .err()
    .unwrap();
    assert!(error.contains("synthetic owner setup failure"));
    let source = Source::open_with(&connection, vec![], || {
        calls.fetch_add(1, Ordering::SeqCst);
        create_owner(&connection)
    })
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(source.owner.shared.active_leases.load(Ordering::Acquire), 1);
}

#[test]
fn a_failed_cached_owner_is_replaced_without_waiting_for_worker_cleanup() {
    let (_server, connection) = connection();
    let mut cached = open(&connection, vec![]).unwrap();
    let old_owner = cached.owner.clone();
    old_owner.shared.fail("synthetic capture failure".into());
    let replacement = open(&connection, vec![]).unwrap();
    assert!(!Arc::ptr_eq(&old_owner, &replacement.owner));
    assert!(replacement.owner.shared.error().is_none());
    assert!(cached.native_snapshot().is_err());
    assert!(cached.native_snapshot().is_err());
    assert_eq!(old_owner.shared.active_leases.load(Ordering::Acquire), 0);
    drop(replacement);
}

#[test]
fn invalid_output_requests_and_subscriber_admission_failures_release_their_leases() {
    let (_server, connection) = connection();
    let source = open(&connection, vec![]).unwrap();
    let owner = source.owner.clone();
    let oversized_id = "x".repeat(257);
    for outputs in [
        vec![json!({"channel":"airspeed_kts"})],
        vec![json!({"id":oversized_id,"channel":"airspeed_kts"})],
        vec![json!({"id":"valid","channel":"not-installed"})],
        (0..1025)
            .map(|_| json!({"id":"valid","channel":"airspeed_kts"}))
            .collect(),
    ] {
        assert!(Source::open_with(&connection, outputs, || {
            panic!("cached registry owner must be reused")
        })
        .is_err());
        assert_eq!(owner.shared.active_leases.load(Ordering::Acquire), 1);
    }

    let mut readers: Vec<_> = (0..63)
        .map(|_| Source::open_with(&connection, vec![], || panic!("must reuse owner")).unwrap())
        .collect();
    assert_eq!(owner.shared.active_leases.load(Ordering::Acquire), 64);
    assert!(Source::open_with(&connection, vec![], || panic!("must reuse owner")).is_err());
    assert_eq!(owner.shared.active_leases.load(Ordering::Acquire), 64);
    drop(readers.pop());
    let until = Instant::now() + Duration::from_secs(1);
    while owner.shared.active_leases.load(Ordering::Acquire) != 63 {
        assert!(
            Instant::now() < until,
            "dropped reader lease was not released"
        );
        thread::yield_now();
    }
    let replacement =
        Source::open_with(&connection, vec![], || panic!("must reuse owner")).unwrap();
    assert_eq!(owner.shared.active_leases.load(Ordering::Acquire), 64);
    drop(replacement);
    drop(source);
}

#[test]
fn rejected_connection_and_process_identity_never_reach_the_factory() {
    let (_server, mut invalid) = connection();
    invalid["port"] = json!(0);
    let calls = AtomicUsize::new(0);
    assert!(Source::open_with(&invalid, vec![], || {
        calls.fetch_add(1, Ordering::SeqCst);
        create_owner(&invalid)
    })
    .is_err());
    let (_other_server, mut missing_identity) = connection();
    missing_identity
        .as_object_mut()
        .unwrap()
        .remove("processIdentity");
    assert!(Source::open_with(&missing_identity, vec![], || {
        calls.fetch_add(1, Ordering::SeqCst);
        create_owner(&missing_identity)
    })
    .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

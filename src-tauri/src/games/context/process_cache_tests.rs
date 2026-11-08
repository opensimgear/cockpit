use super::*;
use crate::game_discovery::Platform;
use crate::game_discovery::{ProcessIdentity, ProcessLookupError};
use serde_json::json;
use std::sync::{atomic::{AtomicUsize, Ordering}, Mutex};

struct FakePlatform {
    responses: Mutex<Vec<Vec<ProcessLookup>>>,
    reads: AtomicUsize,
}

impl FakePlatform {
    fn new(responses: Vec<Vec<ProcessLookup>>) -> Self {
        Self { responses: Mutex::new(responses), reads: AtomicUsize::new(0) }
    }
}

impl Platform for FakePlatform {
    fn catalogue(&self, _: &std::collections::BTreeMap<String, String>) -> Vec<Value> { Vec::new() }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> { Ok(()) }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> { unreachable!() }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> { unreachable!() }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> { unreachable!() }
    fn integration_processes(&self, _: &[&Value]) -> Vec<ProcessLookup> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let mut responses = self.responses.lock().unwrap();
        if responses.len() > 1 { responses.remove(0) } else { responses[0].clone() }
    }
}

fn games() -> Vec<Value> {
    ["alpha", "beta", "gamma"].into_iter().map(|id| json!({
        "id": id,
        "provider": "fixture",
        "integrationKey": id,
        "integrationCompatible": true,
        "executable": format!("{id}.exe"),
        "processExecutables": [format!("{id}.exe")],
        "processMonitoringSupported": true,
    })).collect()
}

fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity { pid, path: format!("/{pid}.exe"), epoch: 77 }
}

fn rows_with_unavailable() -> Vec<ProcessLookup> {
    vec![
        Ok(Some(identity(10))),
        Err(ProcessLookupError::Unavailable("temporarily unreadable".into())),
        Err(ProcessLookupError::Ambiguous("multiple matches".into())),
    ]
}

#[test]
fn keeps_successful_and_ambiguous_rows_usable_while_retrying_unavailable_rows() {
    let games = games();
    let selected: Vec<_> = games.iter().collect();
    let clean = vec![Ok(Some(identity(10))), Ok(None), Err(ProcessLookupError::Ambiguous("multiple matches".into()))];
    let platform = FakePlatform::new(vec![rows_with_unavailable()]);
    let mut cache = Cache::default();
    let epoch = (4, 9);

    assert_eq!(cache.read(&platform, &selected, Some(epoch)), rows_with_unavailable());
    assert!(!cache.is_current(epoch), "an Unavailable row keeps the full inventory retryable");
    let snapshot = cache.snapshot().expect("cardinality-correct results are cached per row");
    assert_eq!(snapshot.process(&games[0], epoch), Some(Ok(Some(identity(10)))));
    assert_eq!(snapshot.process(&games[1], epoch), Some(Err(ProcessLookupError::Unavailable("temporarily unreadable".into()))));
    assert_eq!(snapshot.process(&games[2], epoch), Some(Err(ProcessLookupError::Ambiguous("multiple matches".into()))));
    assert_eq!(snapshot.process(&games[0], (4, 10)), None);
    let mut changed_selection = games[0].clone();
    changed_selection["executable"] = json!("replacement.exe");
    assert_eq!(snapshot.process(&changed_selection, epoch), None);

    let partial = cache.read_using(&selected, Some(epoch), |retried| {
        assert_eq!(retried, [&games[1]], "Only the unavailable game may be read again");
        Vec::new()
    });
    assert_eq!(partial, rows_with_unavailable(), "An incomplete retry cannot erase healthy rows");
    assert!(!cache.is_current(epoch));

    let recovered = cache.read_using(&selected, Some(epoch), |retried| {
        assert_eq!(retried, [&games[1]]);
        vec![Ok(None)]
    });
    assert_eq!(recovered, clean);
    assert!(cache.is_current(epoch), "Ambiguous is definitive and does not require a retry");
    assert_eq!(cache.read(&platform, &selected, Some(epoch)), clean);
    assert_eq!(platform.reads.load(Ordering::SeqCst), 1, "a definitive inventory is reusable");

    let replaced = cache.read_using(&selected, Some((epoch.0, epoch.1 + 1)), |retried| {
        assert_eq!(retried, selected);
        vec![Ok(Some(identity(20))), Ok(None), Ok(None)]
    });
    assert_eq!(replaced[0], Ok(Some(identity(20))), "A new event epoch requires new proof");
}

#[test]
fn rejects_snapshots_when_inventory_cardinality_is_wrong() {
    let games = games();
    let selected: Vec<_> = games.iter().collect();
    let platform = FakePlatform::new(vec![vec![Ok(Some(identity(10)))]]);
    let mut cache = Cache::default();

    let result = cache.read(&platform, &selected, Some((4, 9)));
    assert_eq!(result, unavailable_rows(&selected, "Game process inventory was incomplete."));
    assert!(cache.snapshot().is_none());
    assert!(!cache.is_current((4, 9)));
}

#[test]
fn snapshot_requires_supported_exact_selection_and_matching_epoch() {
    let games = games();
    let selected: Vec<_> = games.iter().collect();
    let rows = vec![Ok(Some(identity(10))), Ok(None), Err(ProcessLookupError::Ambiguous("multiple matches".into()))];
    let platform = FakePlatform::new(vec![rows]);
    let mut cache = Cache::default();
    cache.read(&platform, &selected, Some((4, 9)));
    let snapshot = cache.snapshot().unwrap();

    assert_eq!(snapshot.process(&games[0], (4, 10)), None);
    let mut unsupported = games[0].clone();
    unsupported["processMonitoringSupported"] = json!(false);
    assert_eq!(snapshot.process(&unsupported, (4, 9)), None);
}

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct Sink {
    definitions: Mutex<BTreeMap<String, SourceDefinition>>,
    fail_remove: AtomicBool,
    fail_restore: AtomicBool,
}
impl SourceSink for Sink {
    fn register(&self, definition: SourceDefinition) -> Result<(), String> {
        if self.fail_restore.load(Ordering::SeqCst) { return Err("restore failed".into()); }
        self.definitions.lock().unwrap().insert(definition.source_id.clone(), definition);
        Ok(())
    }
    fn unregister(&self, id: &str) -> Result<(), String> {
        self.definitions.lock().unwrap().remove(id);
        if self.fail_remove.load(Ordering::SeqCst) { return Err("remove failed".into()); }
        Ok(())
    }
    fn baseline(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> { Ok(()) }
    fn changes(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> { Ok(()) }
    fn loss(&self, _: &SourceToken) -> Result<(), String> { Ok(()) }
}
struct Memory { bytes: Mutex<Vec<u8>>, fail: AtomicBool }
impl StateStorage for Memory {
    fn read(&self) -> Result<Option<Vec<u8>>, String> { Ok(Some(self.bytes.lock().unwrap().clone())) }
    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        if self.fail.load(Ordering::SeqCst) { return Err("persist failed".into()); }
        *self.bytes.lock().unwrap() = bytes.to_vec();
        Ok(())
    }
}
fn fixture() -> (Hub, Arc<Sink>, Arc<Memory>) {
    let upload = serde_json::from_str(include_str!("../tests/fixtures/compiled-toggle-publication.json")).unwrap();
    let publication = Publication::validate(upload).unwrap();
    let record = Record {
        host_id: "host".into(),
        credentials: vec![Credential {companion_id: "screen".into(), name: "Screen".into(), token_hash: "a".repeat(64), kind: "browser".into()}],
        instances: ["first", "second"].into_iter().map(|instance| Assignment {
            instance_id: instance.into(), companion_id: "screen".into(),
            publication_hash: publication.publication_hash.clone(), definition: publication.definition(instance), ownership: None,
        }).collect(),
    };
    let storage = Arc::new(Memory {bytes: Mutex::new(serde_json::to_vec(&record).unwrap()), fail: AtomicBool::new(false)});
    let sink = Arc::new(Sink::default());
    let hub = Hub::with_existing_storage(storage.clone(), sink.clone()).unwrap().unwrap();
    (hub, sink, storage)
}
fn remove(hub: &Hub, command: &str) -> Result<Value, String> {
    hub.command(&json!({"command":command,"instanceId":"first","companionId":"screen"}))
}
#[test]
fn removals_retire_native_descriptors_and_reopen_without_removed_config() {
    for command in ["companion_unassign", "companion_revoke"] {
        let (hub, sink, storage) = fixture();
        remove(&hub, command).unwrap();
        let remaining = if command == "companion_unassign" {1} else {0};
        assert_eq!(sink.definitions.lock().unwrap().len(), remaining);
        let restored = Arc::new(Sink::default());
        Hub::with_existing_storage(storage, restored.clone()).unwrap().unwrap();
        assert_eq!(restored.definitions.lock().unwrap().len(), remaining);
    }
}
#[test]
fn failed_descriptor_or_storage_removal_preserves_authority_and_restores_descriptors() {
    for command in ["companion_unassign", "companion_revoke"] {
        for fail_storage in [false, true] {
            let (hub, sink, storage) = fixture();
            let before = storage.bytes.lock().unwrap().clone();
            storage.fail.store(fail_storage, Ordering::SeqCst);
            sink.fail_remove.store(!fail_storage, Ordering::SeqCst);
            assert!(remove(&hub, command).is_err());
            assert_eq!(*storage.bytes.lock().unwrap(), before);
            assert_eq!(sink.definitions.lock().unwrap().len(), 2);
            assert_eq!(hub.resource_instance_ids().len(), 2);
        }
    }
}
#[test]
fn failed_descriptor_compensation_surfaces_the_preserved_authority() {
    let (hub, sink, storage) = fixture();
    storage.fail.store(true, Ordering::SeqCst);
    sink.fail_restore.store(true, Ordering::SeqCst);
    assert!(remove(&hub, "companion_revoke").is_err());
    assert_eq!(hub.resource_instance_ids().len(), 2);
    assert!(hub.inner.lock().unwrap().diagnostic.as_ref().unwrap().contains("restoration"));
}

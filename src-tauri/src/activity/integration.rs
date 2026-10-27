//! Cached setup metadata for the activity collector. No probes or filesystem I/O.
use super::*;
impl GameIntegrationSetup {
    pub(crate) fn activity_status(&self, key: &str) -> Value {
        let states=self.inner.states.lock().unwrap_or_else(|p|p.into_inner());
        let Some(state)=states.get(key) else {return Value::Null};
        let mut result=serde_json::Map::new();
        for field in ["state","message","installed","verified","version"] {
            if let Some(value)=state.get(field) {result.insert(field.into(),value.clone());}
        }
        Value::Object(result)
    }
}
#[cfg(test)]
mod activity_tests {
    use super::*;
    #[test]
    fn projection_reads_cached_setup_without_probing_and_excludes_connections() {
        let root=crate::test_directory();let runtime=GameIntegrationSetup::new(root.path().join("setup.json"),false);
        assert_eq!(runtime.activity_status("dcs"),Value::Null);
        runtime.inner.states.lock().unwrap().insert("dcs".into(),json!({"state":"needs-attention","message":"Close the game before restoring files.","installed":true,"verified":false,"version":"1","connection":{"secret":"hidden"},"progress":"installing","percent":75}));
        let state=runtime.activity_status("dcs");assert_eq!(state["state"],"needs-attention");assert_eq!(state["message"],"Close the game before restoring files.");assert!(state.get("connection").is_none());assert!(state.get("progress").is_none());
    }
}

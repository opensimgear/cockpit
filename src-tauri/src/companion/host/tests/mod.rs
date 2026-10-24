use super::*;
    pub(super) fn runtime() -> Runtime {
        Runtime::new(None, Arc::new(CompanionProvider::new(Arc::new(|| 1))))
    }
    pub(super) fn call(runtime: &Runtime, command: &str, payload: Value) -> Result<Value, String> {
        runtime.request(&json!({"command":command,"clientId":"operator-one","companion":payload}))
    }
mod group1;
mod group2;
mod group3;
mod passive_inventory;

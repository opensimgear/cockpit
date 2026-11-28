#[cfg(target_os = "macos")]
mod inventory;
#[cfg(all(test, not(target_os = "macos")))]
mod inventory {
    mod policy;
}
mod receiver;

#[cfg(target_os = "macos")]
use super::{Backend, Reader};
#[cfg(target_os = "macos")]
use serde_json::Value;

#[cfg(target_os = "macos")]
#[derive(Default)]
pub(super) struct Mac;

#[cfg(target_os = "macos")]
impl Backend for Mac {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
        inventory::inventory(target)
    }

    fn open(&self, target: &str) -> Result<Box<dyn Reader>, String> {
        receiver::open(target)
    }
}

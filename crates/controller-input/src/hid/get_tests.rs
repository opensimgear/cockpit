use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
struct Reads(Arc<AtomicUsize>);
impl Backend for Reads {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
        Ok(json!({"interfaceId":target}))
    }
    fn open(&self, _: &str) -> Result<Box<dyn Reader>, String> {
        Ok(Box::new(Read(self.0.clone())))
    }
}
struct Read(Arc<AtomicUsize>);
impl Reader for Read {
    fn poll(&mut self, _: Duration) -> Result<Option<Vec<u8>>, Value> {
        std::thread::sleep(Duration::from_millis(1));
        Ok(None)
    }
    fn project(&self, _: &[u8]) -> Value {
        Value::Null
    }
    fn get(&mut self, request: &Get, _: Duration) -> Result<Value, Value> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let count = if request.id == "capacity" {
            2_200_000
        } else {
            1_100_000
        };
        Ok(
            json!({"kind":"feature","wireHex":"002a","status":"complete","samples":{"opaque":"x".repeat(count)}}),
        )
    }
}
#[test]
fn native_get_slices_retain_exact_result_without_another_device_read_and_fence_replacement() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Service {
        backend: Some(Arc::new(Reads(calls.clone()))),
        ..Service::new(false, None)
    };
    let opened = service
        .request("controller_hid_open", &json!({"interfaceId":"get"}))
        .unwrap();
    let mut request = opened.clone();
    for (key, value) in [
        ("requestId", json!("first")),
        ("kind", json!("feature")),
        ("reportId", json!(0)),
        ("maxBytes", json!(64)),
        ("deadlineMs", json!(2000)),
        ("bounded", json!(true)),
    ] {
        request[key] = value;
    }
    let reply = service.request("controller_hid_get", &request).unwrap();
    assert!(reply.to_string().len() < 1024 * 1024);
    assert_eq!(reply["result"]["wireHex"], "002a");
    assert_eq!(reply["result"]["status"], "complete");
    assert!(reply["result"]["getSlice"]["totalBytes"].as_u64().unwrap() > 1_000_000);
    let mut slice = request.clone();
    slice["offset"] = json!(0);
    slice["limit"] = json!(32768);
    let mut output = Vec::new();
    loop {
        let result = service.request("controller_hid_get_slice", &slice).unwrap();
        output.extend(transfer::decode(result["result"]["data"].as_str().unwrap()).unwrap());
        if result["result"]["done"] == true {
            break;
        }
        slice["offset"] = result["result"]["nextOffset"].clone();
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap()["samples"]["opaque"],
        "x".repeat(1_100_000)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    request["requestId"] = json!("second");
    service.request("controller_hid_get", &request).unwrap();
    assert!(service.request("controller_hid_get_slice", &slice).is_err());
    request["requestId"] = json!("capacity");
    let over = service.request("controller_hid_get", &request).unwrap();
    assert_eq!(over["result"]["getSlice"]["unavailable"], "capacity");
    assert_eq!(over["result"]["wireHex"], "002a");
    service.request("controller_hid_close", &opened).unwrap();
    assert!(service.request("controller_hid_get_slice", &slice).is_err());
}

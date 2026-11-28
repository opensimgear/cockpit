use super::*;

use std::time::{Duration, Instant};

struct Large;
impl Backend for Large {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
        Ok(match target {
            Some(id) => json!({"interfaceId":id,"descriptorHex":"ab".repeat(600_000)}),
            None => json!({"interfaces":[{"interfaceId":"large"}],"failures":[],"truncated":false}),
        })
    }
    fn open(&self, _: &str) -> Result<Box<dyn Reader>, String> {
        Ok(Box::new(LargeInput(false)))
    }
}
struct LargeInput(bool);
impl Reader for LargeInput {
    fn poll(&mut self, _: Duration) -> Result<Option<Vec<u8>>, Value> {
        if !self.0 {
            self.0 = true;
            return Ok(Some(vec![0, 42]));
        }
        std::thread::sleep(Duration::from_millis(1));
        Ok(None)
    }
    fn project(&self, _: &[u8]) -> Value {
        json!({"opaque":"é".repeat(600_000)})
    }
    fn get(&mut self, _: &Get, _: Duration) -> Result<Value, Value> {
        unreachable!()
    }
}
fn large_service() -> Service {
    Service {
        backend: Some(Arc::new(Large)),
        ..Service::new(false, None)
    }
}
#[test]
fn native_large_report_is_discoverable_and_exactly_sliceable() {
    let service = large_service();
    let opened = service
        .request("controller_hid_open", &json!({"interfaceId":"large"}))
        .unwrap();
    let start = Instant::now();
    let page = loop {
        let page = service.request("controller_hid_reports", &opened).unwrap();
        if page["result"]["latest"] == "1" {
            break page;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
    };
    assert!(page.to_string().len() < 1024 * 1024);
    assert!(
        page["result"]["reports"][0]["reportSlice"]["totalBytes"]
            .as_u64()
            .unwrap()
            > 1_000_000
    );
    let mut request = opened.clone();
    request["sequence"] = json!("1");
    request["offset"] = json!(0);
    request["limit"] = json!(32768);
    let mut output = Vec::new();
    loop {
        let reply = service
            .request("controller_hid_report_slice", &request)
            .unwrap();
        let result = &reply["result"];
        assert!(reply.to_string().len() < 1024 * 1024);
        output.extend(transfer::decode(result["data"].as_str().unwrap()).unwrap());
        if result["done"] == true {
            break;
        }
        request["offset"] = result["nextOffset"].clone();
    }
    let exact: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(exact["samples"]["opaque"], "é".repeat(600_000));
    assert_eq!(exact["wireHex"], "002a");
    assert_eq!(exact["sequence"], "1");
    let status = service
        .request("controller_hid_capture_status", &opened)
        .unwrap();
    assert_eq!(status["result"]["latest"], "1");
    assert_eq!(status["result"]["oldest"], "1");
    assert_eq!(status["result"]["retainedRecords"], 1);
    assert_eq!(status["result"]["status"], "open");
    assert!(status.to_string().len() < 1024);
    assert!(status["result"].get("values").is_none());
    request["sequence"] = json!("2");
    assert!(service
        .request("controller_hid_report_slice", &request)
        .is_err());
    request["sequence"] = json!("+1");
    assert!(service
        .request("controller_hid_report_slice", &request)
        .is_err());
    request["sequence"] = json!("1");
    request["generation"] = json!("stale");
    assert!(service
        .request("controller_hid_report_slice", &request)
        .unwrap_err()
        .contains("Stale"));
    service.shutdown();
}
#[test]
fn native_report_limit_and_next_cursor_are_validated() {
    let service = large_service();
    let mut request = service
        .request("controller_hid_open", &json!({"interfaceId":"large"}))
        .unwrap();
    request["limit"] = json!(0);
    assert!(service.request("controller_hid_reports", &request).is_err());
    request["limit"] = json!(129);
    assert!(service.request("controller_hid_reports", &request).is_err());
    request["limit"] = json!(1);
    let start = Instant::now();
    loop {
        let page = service.request("controller_hid_reports", &request).unwrap();
        if page["result"]["latest"] == "1" {
            assert_eq!(page["result"]["next"], "1");
            assert_eq!(page["result"]["hasMore"], false);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
#[test]
fn native_inspection_slices_fence_changed_contracts_and_require_continuity_identity() {
    let service = large_service();
    let mut request = json!({"source":"inspect","interfaceId":"large","offset":0,"limit":32768});
    let first = service
        .request("controller_hid_inspection_slice", &request)
        .unwrap();
    assert_eq!(first["offset"], 0);
    assert_eq!(first["done"], false);
    request["offset"] = first["nextOffset"].clone();
    assert!(service
        .request("controller_hid_inspection_slice", &request)
        .is_err());
    request["contentFingerprint"] = first["contentFingerprint"].clone();
    let second = service
        .request("controller_hid_inspection_slice", &request)
        .unwrap();
    assert_eq!(second["contentFingerprint"], first["contentFingerprint"]);
    request["contentFingerprint"] = json!("0".repeat(64));
    assert!(service
        .request("controller_hid_inspection_slice", &request)
        .unwrap_err()
        .contains("Stale"));
    for invalid in [json!(0), json!(32769), json!(-1), json!(1.5)] {
        request["offset"] = json!(0);
        request["limit"] = invalid;
        assert!(service
            .request("controller_hid_inspection_slice", &request)
            .is_err());
    }
    request["source"] = json!("get");
    assert!(service
        .request("controller_hid_inspection_slice", &request)
        .is_err());
}
#[test]
fn native_inspection_reconstructs_same_native_response_without_a_copy_store() {
    let service = large_service();
    let mut request = json!({"source":"list","offset":0,"limit":7});
    let mut output = Vec::new();
    loop {
        let slice = service
            .request("controller_hid_inspection_slice", &request)
            .unwrap();
        output.extend(transfer::decode(slice["data"].as_str().unwrap()).unwrap());
        if slice["done"] == true {
            break;
        }
        request["offset"] = slice["nextOffset"].clone();
        request["contentFingerprint"] = slice["contentFingerprint"].clone();
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap(),
        service.request("controller_hid_list", &json!({})).unwrap()
    );
}

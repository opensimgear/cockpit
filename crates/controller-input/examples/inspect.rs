use osg_controller_input::ControllerService;
use serde_json::json;
fn main() {
    let service = if std::env::args().any(|a| a == "--qualification-probe") {
        ControllerService::qualification_probe()
    } else {
        ControllerService::new(None, true)
    };
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let state = service
            .request(&json!({"command":"controller_list"}))
            .unwrap();
        println!("{state}");
        for source in state["sources"].as_array().unwrap() {
            let detail = service
                .request(&json!({"command":"controller_inspect","sourceId":source["id"]}))
                .unwrap();
            println!(
                "{}",
                json!({"name":source["name"],"controls":detail["controls"],"diagnostics":detail["diagnostics"]})
            );
        }
    }
    service.shutdown();
}

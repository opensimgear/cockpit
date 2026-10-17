// A legal graph plus exact local pins exceeds the former 576 KiB request cap.
// Included by the pure Service and actual native Manager/service transport tests.
fn editor_boundary_request() -> serde_json::Value {
    let mut resources = Vec::new();
    let mut assignments = Vec::new();
    for resource in 0..16 {
        let mut ports = Vec::new();
        let mut expected = Vec::new();
        for port in 0..64 {
            let id = format!("port-{port:02}{}", "p".repeat(73));
            ports.push(
                serde_json::json!({"id":id,"name":"Port".repeat(20),"direction":"output",
                "contract":{"delivery":"event","schema":{"type":"boolean"}}}),
            );
            expected.push(serde_json::json!({"profilePortId":id,"observedPortId":format!("button-{port:02}{}","x".repeat(247)),"expectedContractHash":format!("sha256:{}","a".repeat(64))}));
        }
        let id = format!("device-{resource}");
        resources.push(serde_json::json!({"id":id,"name":format!("Device {resource}"),"kind":"device","required":false,"ports":ports}));
        assignments.push(serde_json::json!({"resourceId":id,"sourceId":format!("native:{resource}{}","s".repeat(1000)),"ports":expected}));
    }
    serde_json::json!({"command":"profile_editor_save","expectedRevision":null,"assignments":assignments,
        "document":{"schemaVersion":2,"id":"0cda9ec2-85fd-4938-aa17-091131874af9","name":"Large editor boundary","revision":1,
        "eligibility":{"gameIds":[]},"resources":resources,"bindings":[],"processingNodes":[],"virtualOutputs":[],"visibility":[],"overrides":[],"companionAssignments":[],"editor":{"positions":[],"viewport":{"x":0.0,"y":0.0,"zoom":1.0}}}})
}

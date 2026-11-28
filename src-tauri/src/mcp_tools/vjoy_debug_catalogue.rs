use super::*;

#[cfg(test)]
#[test]
fn retry_of_a_large_vjoy_open_retains_the_exact_capture_identity() {
    let result=json!({"ok":true,"data":{"slot":3,"generation":"active","capabilities":{"descriptorHex":"ab".repeat(65536)}}});
    let receipt=compact_operation_result(&result,"operation");
    assert_eq!(receipt["data"]["slot"],3);
    assert_eq!(receipt["data"]["generation"],"active");
    assert_eq!(receipt["data"]["inspectionTool"],"controller_vjoy_debug_inspect");
    assert!(receipt.to_string().len()<1024);
}
pub(super) fn catalogue() -> Vec<Value> {
    let slot = || vec![("slot", integer(1, 16))];
    let session = || fields(slot(), vec![("generation", string(128))]);
    let paging = || {
        vec![
            (
                "after",
                json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20}),
            ),
            ("limit", integer(1, 128)),
        ]
    };
    let record = json!({"type":"object","additionalProperties":false,"required":["sequence","command","wireHex"],"properties":{
        "sequence":{"type":"string","pattern":"^[1-9][0-9]*$","maxLength":20},"command":integer(0,u32::MAX as u64),
        "wireHex":{"type":"string","pattern":"^([0-9a-fA-F]{2})*$","maxLength":496}}});
    let archive = json!({"type":"object","additionalProperties":false,"required":["format","slot","records"],"properties":{
        "format":{"type":"string","enum":["osg-vjoy-ffb"]},"slot":integer(1,16),"records":{"type":"array","maxItems":512,"items":record},
        "startingSequence":{"type":"string","maxLength":20,"pattern":"^(0|[1-9][0-9]*)$"},
        "capabilities":{"type":"object"},"reason":{"type":"string","maxLength":256}}});
    vec![
        tool("controller_vjoy_debug_inspect","Inspect the selected Windows vJoy slot, SDK versions, report layout and effect capabilities without acquiring it.",slot(),&["slot"]),
        tool("controller_vjoy_debug_open","Acquire an available FFB slot for explicit receive-only observation. Return a new capture generation.",slot(),&["slot"]),
        tool("controller_vjoy_debug_read","Read ordered native callbacks, original bytes, decoded requests and observed effect state. Continue with next; gap reports lost history.",fields(session(),paging()),&["slot","generation"]),
        tool("controller_vjoy_debug_close","Release this client's vJoy capture and retire its generation. Export the recording first.",session(),&["slot","generation"]),
        tool("controller_vjoy_debug_record_start","Start recording subsequent callbacks on this client's capture. Replaces a previous stopped recording.",session(),&["slot","generation"]),
        tool("controller_vjoy_debug_record_stop","Seal this client's recording at its observed prefix.",session(),&["slot","generation"]),
        tool("controller_vjoy_debug_export","Export the sealed native observation archive for this capture.",session(),&["slot","generation"]),
        tool("controller_vjoy_debug_replay","Decode an observation archive offline through the same native decoder. Returns effect state through the page cursor.",fields(vec![("recording",archive)],paging()),&["recording"]),
    ]
}

use super::*;
use crate::profile_desktop_preview::{MAX_ADVANCES, MAX_CHANGES, MAX_FRAMES};

pub(super) fn desktop_preview() -> Value {
    let counter = json!({"type":"string","minLength":1,"maxLength":20,"pattern":"^(0|[1-9][0-9]*)$"});
    let scalar = json!({"type":"object","additionalProperties":false,
        "properties":{"kind":{"const":"scalar"},"value":{"type":["boolean","number","string"]}},
        "required":["kind","value"]});
    let change = json!({"type":"object","additionalProperties":false,
        "properties":{"portId":string(80),"sample":{"type":"object","additionalProperties":false,
            "properties":{"status":{"const":"known"},"value":scalar},"required":["status","value"]}},
        "required":["portId","sample"]});
    let frame = json!({"type":"object","additionalProperties":false,
        "properties":{"source":string(80),"generation":counter,"sequence":counter,"capturedUs":counter,
            "reason":choice(&["baseline","change"]),
            "changes":{"type":"array","maxItems":MAX_CHANGES,"items":change}},
        "required":["source","generation","sequence","capturedUs","reason","changes"]});
    tool("profile_desktop_preview",
        "Record keyboard and mouse output from a recent compilation and bounded synthetic scalar samples. This uses a virtual clock and never submits native input or activates a profile.",
        vec![("compilationId",string(36)),
            ("frames",json!({"type":"array","maxItems":MAX_FRAMES,"items":frame})),
            ("advances",json!({"type":"array","maxItems":MAX_ADVANCES,"items":counter}))],
        &["compilationId"])
}

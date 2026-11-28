//! Explicit receive-only HID tasks; every operation stays on the native service path.
use super::*;

pub(super) fn catalogue() -> Vec<Value> {
    let selected=||vec![("interfaceId",string(128))];
    let capture=||fields(selected(),vec![("generation",string(128))]);
    vec![
        tool("controller_hid_list","Enumerate native HID interfaces and metadata availability. A captureGeneration identifies an already open shared receive stream. This never opens a receiver. Native sensitive-device restrictions remain authoritative.",vec![],&[]),
        tool("controller_hid_inspect","Inspect the selected native HID descriptor and contractFingerprint without opening a receiver. Preserve native partial-capability failures and provenance.",selected(),&["interfaceId"]),
        tool("controller_hid_catalog","List cached last-observed HID descriptor summaries. Offline snapshots do not establish current device availability and never start discovery or capture.",vec![],&[]),
        tool("controller_hid_snapshot","Read the full cached last-observed descriptor by interfaceId or snapshotId supplied as interfaceId. This reads process-local cached evidence; it does not probe or open a device.",selected(),&["interfaceId"]),
        tool("controller_hid_open","Explicitly open one selected receive-only HID capture. Optional contractFingerprint fences the inspected contract. Returns the native generation; retain it for state/reports/get/close. Native access-denied and metadata-only devices remain unavailable. Close your capture when finished.",fields(selected(),vec![("contractFingerprint",string(128))]),&["interfaceId"]),
        tool("controller_hid_state","Read the native decoded current state for an exact selected capture generation. Unknown/unavailable/stale values, diagnostics and terminal failures are native evidence. This does not open a capture or reconstruct reports.",capture(),&["interfaceId","generation"]),
        tool("controller_hid_reports","Read a bounded native report page for an exact capture generation; after is an unsigned decimal cursor. Preserve repeated packets, gap and terminal evidence. Share the same captureGeneration as the UI; do not open a second receive stream.",fields(capture(),vec![("after",json!({"type":"string","minLength":1,"maxLength":20,"pattern":"^(0|[1-9][0-9]*)$"}))]),&["interfaceId","generation"]),
        tool("controller_hid_get","Explicit bounded input/feature GET on an existing receive-only HID generation. This sends no output report. Use a unique requestId; native support, sensitive-device restrictions, report bounds and deadlines remain authoritative.",fields(capture(),vec![("requestId",string(128)),("kind",choice(&["input","feature"])),("reportId",integer(0,255)),("maxBytes",integer(1,65536)),("deadlineMs",integer(1,2000))]),&["interfaceId","generation","requestId","kind","reportId","maxBytes","deadlineMs"]),
        tool("controller_hid_close","Close the exact selected HID capture generation you opened. This retires its receiver; a stale generation cannot close a replacement. Do not close a shared capture owned by another workflow.",capture(),&["interfaceId","generation"]),
    ]
}

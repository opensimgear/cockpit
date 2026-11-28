//! IOHID exposes synchronous GET; the bounded owner retains all native resources
//! until actual completion, independently of caller deadline/session close.
use super::*;
pub(super) fn access_granted() -> bool {
    super::super::super::inventory::listen_access_status() == "granted"
}
pub(super) fn status_kind(status: i32) -> &'static str {
    match status as u32 {
        0xe000_02c1 | 0xe000_02e2 => "permission-denied",
        0xe000_02c0 | 0xe000_02d8 => "removed",
        0xe000_02d6 => "timeout",
        0xe000_02c7 => "unsupported",
        _ => "io-error",
    }
}
struct Opened(HidDevice);
impl Drop for Opened {
    fn drop(&mut self) {
        let _ = self.0.close_with_options(0);
    }
}
pub(super) fn get(
    device: &HidDevice,
    interface: &str,
    req: &Get,
    length: usize,
    timeout: Duration,
) -> Result<Vec<u8>, Value> {
    let deadline = Instant::now() + timeout;
    let registry = device.id().map_err(error)?.0;
    let descriptor = super::super::super::inventory::descriptor(device)
        .map_err(|e| failure("stale-generation", e))?;
    let kind = if req.feature {
        HidReportType::Feature
    } else {
        HidReportType::Input
    };
    let report_id = req.report_id;
    let interface = interface.to_owned();
    let remaining = deadline.saturating_duration_since(Instant::now());
    super::super::get_worker::read(registry, remaining, move || {
        // HidDevice is intentionally !Send. Resolve the exact registry lifetime
        // on its owning worker and recheck the complete capture restriction tree.
        let (retained, _) = super::super::super::inventory::selected(&interface).map_err(|e| {
            let kind = if e.contains("Restricted") {
                "permission-denied"
            } else {
                "stale-generation"
            };
            failure(kind, e)
        })?;
        let verify = |handle: &HidDevice| -> Result<(), Value> {
            let current = super::super::super::inventory::descriptor(handle)
                .map_err(|e| failure("stale-generation", e))?;
            if handle.id().map_err(error)?.0 != registry || current != descriptor {
                return Err(failure(
                    "stale-generation",
                    "GET registry identity or descriptor changed",
                ));
            }
            Ok(())
        };
        verify(&retained)?;
        if Instant::now() >= deadline {
            return Err(failure(
                "timeout",
                "GET deadline expired before opening the device",
            ));
        }
        retained.open_with_options(0).map_err(error)?;
        let owned = Opened(retained);
        verify(&owned.0)?;
        if Instant::now() >= deadline {
            return Err(failure(
                "timeout",
                "GET deadline expired before issuing the native request",
            ));
        }
        let bytes = owned
            .0
            .get_report(kind, u32::from(report_id), length)
            .map_err(error)?;
        verify(&owned.0)?;
        Ok(bytes)
    })
}

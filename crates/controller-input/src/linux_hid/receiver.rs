use super::{
    hex,
    inventory::{self, Interface},
    io, parse, Get, Reader,
};
use osg_hid_report::{decode_report, Descriptor, Framing, ReportKind, MAX_REPORT_BYTES};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn failure(kind: &str, message: impl AsRef<str>) -> Value {
    json!({"kind":kind,"message":message.as_ref()})
}

trait Transport {
    fn verify(&self) -> Result<(), Value>;
    fn poll(&mut self, timeout: Duration) -> io::Receive;
    fn get(&mut self, req: &Get, length: usize, timeout: Duration) -> Result<Vec<u8>, Value>;
}
struct Handle {
    device: io::Device,
    row: Interface,
    descriptor: Vec<u8>,
}
impl Transport for Handle {
    fn verify(&self) -> Result<(), Value> {
        inventory::verify_opened(&self.row, self.device.file(), &self.descriptor)
            .map(|_| ())
            .map_err(|e| json!(e))
    }
    fn poll(&mut self, timeout: Duration) -> io::Receive {
        self.device.poll(MAX_REPORT_BYTES, timeout)
    }
    fn get(&mut self, req: &Get, length: usize, timeout: Duration) -> Result<Vec<u8>, Value> {
        let kind = if req.feature {
            io::GetKind::Feature
        } else {
            io::GetKind::Input
        };
        self.device
            .get(&req.id, kind, req.report_id, length, timeout)
            .map(|p| p.bytes)
            .map_err(|e| json!(e))
    }
}

pub(super) fn open(row: Interface) -> Result<Box<dyn Reader>, String> {
    if row.restricted {
        return Err(format!(
            "Metadata only: {}",
            row.restriction_reasons.join(", ")
        ));
    }
    let device = io::Device::open(&row.path).map_err(|e| format!("{:?}: {}", e.kind, e.message))?;
    let metadata = device
        .metadata()
        .map_err(|e| format!("{:?}: {}", e.kind, e.message))?;
    let parsed = parse(&metadata.descriptor)?;
    let handle = Handle {
        device,
        row,
        descriptor: metadata.descriptor,
    };
    handle.verify().map_err(|e| e.to_string())?;
    Ok(Box::new(Capture::new(handle, parsed)))
}

struct Capture<T> {
    io: T,
    parsed: Descriptor,
    last_verified: Instant,
}
impl<T: Transport> Capture<T> {
    fn new(io: T, parsed: Descriptor) -> Self {
        Self {
            io,
            parsed,
            last_verified: Instant::now(),
        }
    }
    fn framing(&self, kind: ReportKind) -> Framing {
        if self
            .parsed
            .layouts
            .iter()
            .any(|l| l.kind == kind && l.report_id.is_some())
        {
            Framing::IdPrefix
        } else {
            Framing::Unnumbered
        }
    }
    fn decoded(&self, kind: ReportKind, bytes: &[u8]) -> Value {
        match decode_report(&self.parsed, kind, self.framing(kind), bytes) {
            Ok(report) => json!(report),
            Err(d) => json!({"raw":hex(bytes),"diagnostics":[d]}),
        }
    }
}
impl<T: Transport> Reader for Capture<T> {
    fn poll(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, Value> {
        // The open fd owns this connection. Also detect permission/namespace or
        // descriptor replacement while a device is idle and has no new reports.
        if self.last_verified.elapsed() >= Duration::from_millis(100) {
            self.io.verify()?;
            self.last_verified = Instant::now();
        }
        match self.io.poll(timeout) {
            io::Receive::Report(bytes) => {
                self.io.verify()?;
                self.last_verified = Instant::now();
                Ok(Some(bytes))
            }
            io::Receive::Timeout => Ok(None),
            io::Receive::Removed => {
                Err(failure("removed", "Selected hidraw interface disconnected"))
            }
            io::Receive::Error(e) => Err(json!(e)),
        }
    }
    fn project(&self, bytes: &[u8]) -> Value {
        self.decoded(ReportKind::Input, bytes)
    }
    fn get(&mut self, req: &Get, remaining: Duration) -> Result<Value, Value> {
        let start = Instant::now();
        self.io.verify()?;
        let kind = if req.feature {
            ReportKind::Feature
        } else {
            ReportKind::Input
        };
        if !self.parsed.diagnostics.is_empty() {
            return Err(failure(
                "unsupported",
                "Descriptor has unresolved diagnostics",
            ));
        }
        let id = (req.report_id != 0).then_some(req.report_id);
        let layout = self
            .parsed
            .layouts
            .iter()
            .find(|l| l.kind == kind && l.report_id == id)
            .ok_or_else(|| {
                failure(
                    "unsupported",
                    "Descriptor does not declare the requested kind and ID",
                )
            })?;
        let payload = layout.payload_bits.div_ceil(8);
        let expected = payload + usize::from(id.is_some());
        if expected > req.max_bytes {
            return Err(failure(
                "capacity",
                "Caller byte limit is below the declared report length",
            ));
        }
        let timeout = remaining
            .checked_sub(start.elapsed())
            .filter(|t| !t.is_zero())
            .ok_or_else(|| failure("timeout", "Deadline expired before issuing GET"))?;
        // Both ioctl requests include a selector byte. An unnumbered response
        // starts at payload byte zero; a numbered response retains its ID.
        let bytes = self.io.get(req, payload + 1, timeout)?;
        self.io.verify()?;
        if bytes.len() != expected || id.is_some_and(|i| bytes.first() != Some(&i)) {
            return Err(
                json!({"kind":"malformed","message":"GET response ID or length differs from descriptor",
                "wireHex":hex(&bytes)}),
            );
        }
        Ok(json!({"kind":kind,"wireHex":hex(&bytes),"samples":self.decoded(kind,&bytes)}))
    }
}

#[cfg(test)]
#[path = "receiver_tests.rs"]
mod tests;

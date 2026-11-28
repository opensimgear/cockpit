//! Selected IOHID stream. Element values are never reconstructed into wire reports.
#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
pub(super) fn open(id: &str) -> Result<Box<dyn Reader>, String> {
    native::open(id)
}
#[cfg(any(target_os = "macos", test))]
mod get_worker;
use super::super::{Get, Reader};
use osg_hid_report::{decode_report, parse_descriptor, Descriptor, Framing, ReportKind};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn failure(kind: &str, message: impl AsRef<str>) -> Value {
    json!({"kind":kind,"message":message.as_ref()})
}
fn hex(raw: &[u8]) -> String {
    super::super::session::hex(raw)
}

enum Event {
    Report {
        bytes: Vec<u8>,
        report_id: u32,
        timestamp: u64,
    },
    Element {
        sample: Value,
        report_id: u32,
        timestamp: u64,
    },
}
impl Event {
    fn provenance(&self) -> Value {
        let (source, wire, id, timestamp) = match self {
            Self::Report {
                report_id,
                timestamp,
                ..
            } => ("iohid-input-report", true, *report_id, *timestamp),
            Self::Element {
                report_id,
                timestamp,
                ..
            } => ("iohid-element", false, *report_id, *timestamp),
        };
        json!({"source":source,"wireAvailable":wire,"reportId":id,"timestamp":timestamp.to_string()})
    }
}
trait Transport {
    fn verify(&self) -> Result<(), Value>;
    fn poll(&mut self, timeout: Duration) -> Result<Option<Event>, Value>;
    fn get(&mut self, req: &Get, length: usize, timeout: Duration) -> Result<Vec<u8>, Value>;
}
struct Capture<T> {
    io: T,
    parsed: Option<Descriptor>,
    last: Option<Event>,
}
impl<T: Transport> Capture<T> {
    fn new(io: T, parsed: Option<Descriptor>) -> Self {
        Self {
            io,
            parsed,
            last: None,
        }
    }
    fn decoded(&self, kind: ReportKind, id: u32, bytes: &[u8]) -> Value {
        let Some(parsed) = &self.parsed else {
            return json!({"unavailable":"IOHID report descriptor unavailable","report_id":id});
        };
        // IOHID supplies an independent ID. Preserve OS bytes and choose framing
        // by the declared length; never insert an ID into the captured report.
        let id = u8::try_from(id).ok().filter(|id| *id != 0);
        let prefix = parsed
            .layouts
            .iter()
            .find(|l| l.kind == kind && l.report_id == id)
            .is_some_and(|l| {
                id.is_some()
                    && bytes.len() == l.payload_bits.div_ceil(8) + 1
                    && bytes.first() == id.as_ref()
            });
        let framing = if prefix {
            Framing::IdPrefix
        } else {
            Framing::OutOfBand(id)
        };
        match decode_report(parsed, kind, framing, bytes) {
            Ok(v) => json!(v),
            Err(e) => json!({"raw":hex(bytes),"report_id":id,"diagnostics":[e]}),
        }
    }
}
impl<T: Transport> Reader for Capture<T> {
    fn poll(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, Value> {
        self.io.verify()?;
        let Some(event) = self.io.poll(timeout)? else {
            return Ok(None);
        };
        self.io.verify()?;
        let raw = match &event {
            Event::Report { bytes, .. } => bytes.clone(),
            Event::Element { .. } => Vec::new(),
        };
        self.last = Some(event);
        Ok(Some(raw))
    }
    fn provenance(&self) -> Option<Value> {
        self.last.as_ref().map(Event::provenance)
    }
    fn project(&self, bytes: &[u8]) -> Value {
        match &self.last {
            Some(Event::Element { sample, .. }) => json!({"elements":[sample]}),
            Some(Event::Report { report_id, .. }) => {
                self.decoded(ReportKind::Input, *report_id, bytes)
            }
            None => json!({"unavailable":"No IOHID callback received"}),
        }
    }
    fn get(&mut self, req: &Get, remaining: Duration) -> Result<Value, Value> {
        let started = Instant::now();
        self.io.verify()?;
        let kind = if req.feature {
            ReportKind::Feature
        } else {
            ReportKind::Input
        };
        let parsed = self
            .parsed
            .as_ref()
            .filter(|p| p.diagnostics.is_empty())
            .ok_or_else(|| {
                failure(
                    "unsupported",
                    "A complete descriptor is required for explicit GET",
                )
            })?;
        let id = (req.report_id != 0).then_some(req.report_id);
        let layout = parsed
            .layouts
            .iter()
            .find(|l| l.kind == kind && l.report_id == id)
            .ok_or_else(|| {
                failure(
                    "unsupported",
                    "Descriptor does not declare requested kind and report ID",
                )
            })?;
        let payload = layout.payload_bits.div_ceil(8);
        let capacity = payload + usize::from(id.is_some());
        if capacity > req.max_bytes {
            return Err(failure(
                "capacity",
                "Caller byte limit is below declared report length",
            ));
        }
        let timeout = remaining
            .checked_sub(started.elapsed())
            .filter(|t| !t.is_zero())
            .ok_or_else(|| failure("timeout", "GET deadline expired"))?;
        let bytes = self.io.get(req, capacity.max(1), timeout)?;
        self.io.verify()?;
        if started.elapsed() >= remaining {
            return Err(failure("timeout", "GET deadline expired"));
        }
        let valid = bytes.len() == payload
            || (id.is_some() && bytes.len() == capacity && bytes.first() == id.as_ref());
        if !valid {
            return Err(
                json!({"kind":"malformed","message":"GET length or report ID differs from descriptor","wireHex":hex(&bytes)}),
            );
        }
        Ok(
            json!({"kind":kind,"reportId":req.report_id,"wireHex":hex(&bytes),
            "provenance":{"source":"iohid-get-report","wireAvailable":true,"reportId":req.report_id},
            "samples":self.decoded(kind,u32::from(req.report_id),&bytes)}),
        )
    }
}
#[cfg(test)]
mod tests;

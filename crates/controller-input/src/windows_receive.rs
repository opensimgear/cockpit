//! Shared, read-only HID I/O for one explicitly selected top-level collection.
//! Pending buffers survive timeouts; drop never waits indefinitely on a driver.
mod native;
mod policy;
#[cfg(test)]
mod tests;

use crate::windows_caps::{self, ReportKind, Samples, Snapshot};
use serde::Serialize;
use std::time::Duration;

pub const MAX_REPORT_BYTES: usize = 65_536;
pub const MAX_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct Selection {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version_number: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub input_report_bytes: usize,
    pub feature_report_bytes: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    MetadataOnly,
    InvalidRequest,
    Busy,
    Timeout,
    Removed,
    AccessDenied,
    Unsupported,
    Capacity,
    Os,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveError {
    pub kind: ErrorKind,
    pub code: Option<u32>,
    pub message: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "kebab-case")]
pub enum Receive {
    Report(Vec<u8>),
    Timeout,
    Removed,
    Error(ReceiveError),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetReport {
    pub request_id: String,
    pub bytes: Vec<u8>,
}

pub struct Receiver {
    native: native::Session,
    snapshot: Snapshot,
    input_ids: Vec<u8>,
    feature_ids: Vec<u8>,
    input_bytes: usize,
    feature_bytes: usize,
}
fn error(kind: ErrorKind, message: impl Into<String>) -> ReceiveError {
    ReceiveError {
        kind,
        code: None,
        message: message.into(),
    }
}
/// Exclusive Windows input collections, plus this inspector's system-power
/// policy. Other Generic Desktop, consumer, sensor and vendor usages are shared.
pub(crate) fn metadata_only(page: u16, usage: u16) -> bool {
    page == 7
        || (page == 1 && [1, 2, 6, 7, 0x80].contains(&usage))
        || (page == 0x0d && [1, 2, 4, 5].contains(&usage))
}
pub fn open(selection: &Selection) -> Result<Receiver, ReceiveError> {
    policy::selection(selection)?;
    let (native, snapshot) = native::Session::open(selection)?;
    Ok(Receiver::validated(selection, native, snapshot))
}
impl Receiver {
    // Native construction has already verified the opened interface identity,
    // TLC and report lengths. Keeping admission here also serves fake-kernel tests.
    fn validated(selection: &Selection, native: native::Session, snapshot: Snapshot) -> Self {
        let mut input_ids = vec![];
        let mut feature_ids = vec![];
        for report in &snapshot.reports {
            if !snapshot.available || !report.complete {
                continue;
            }
            match report.kind {
                ReportKind::Input => input_ids = report.report_ids.clone(),
                ReportKind::Feature => feature_ids = report.report_ids.clone(),
                ReportKind::Output => (),
            }
        }
        Receiver {
            native,
            snapshot,
            input_ids,
            feature_ids,
            input_bytes: selection.input_report_bytes,
            feature_bytes: selection.feature_report_bytes,
        }
    }
    pub fn poll(&mut self, timeout: Duration) -> Receive {
        if let Err(error) = policy::deadline(timeout) {
            return Receive::Error(error);
        }
        if self.input_bytes == 0 {
            return Receive::Error(error(
                ErrorKind::Unsupported,
                "This collection declares no input report",
            ));
        }
        match self.native.poll(self.input_bytes, timeout) {
            Ok(Some(bytes)) => Receive::Report(bytes),
            Ok(None) => Receive::Timeout,
            Err(error) if error.kind == ErrorKind::Removed => Receive::Removed,
            Err(error) => Receive::Error(error),
        }
    }
    pub fn get(
        &mut self,
        request_id: &str,
        kind: ReportKind,
        report_id: u8,
        max_bytes: usize,
        deadline: Duration,
    ) -> Result<GetReport, ReceiveError> {
        let (length, ids) = match kind {
            ReportKind::Input => (self.input_bytes, &self.input_ids),
            ReportKind::Feature => (self.feature_bytes, &self.feature_ids),
            ReportKind::Output => {
                return Err(error(
                    ErrorKind::InvalidRequest,
                    "Output reports are never authorized",
                ))
            }
        };
        policy::get(request_id, report_id, max_bytes, deadline, length, ids)?;
        let bytes = self.native.get(kind, report_id, length, deadline)?;
        policy::completed_get(report_id, length, &bytes)?;
        Ok(GetReport {
            request_id: request_id.into(),
            bytes,
        })
    }
    pub fn samples(&self, kind: ReportKind, wire: &[u8]) -> Samples {
        unsafe { windows_caps::samples(self.native.preparsed(), &self.snapshot, kind, wire) }
    }
}

use super::*;
use iohidmanager::{
    DeviceRemovalSubscription, HidDevice, HidElementType, HidError, HidReportType,
    TimestampedReportSubscription, ValueSubscription,
};
use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};
mod request;
fn enqueue(
    tx: &mpsc::SyncSender<(Event, usize)>,
    budget: &AtomicUsize,
    lost: &AtomicBool,
    event: Event,
) {
    let size = 128
        + match &event {
            Event::Report { bytes, .. } => bytes.len(),
            Event::Element { sample, .. } => sample.to_string().len(),
        };
    let prior = budget.fetch_add(size, Ordering::AcqRel);
    if prior + size > 4 * 1024 * 1024 {
        budget.fetch_sub(size, Ordering::Release);
        lost.store(true, Ordering::Release);
        return;
    }
    if tx.try_send((event, size)).is_err() {
        budget.fetch_sub(size, Ordering::Release);
        lost.store(true, Ordering::Release);
    }
}
pub(super) fn open(id: &str) -> Result<Box<dyn Reader>, String> {
    let (device, row) = super::super::inventory::selected(id)?;
    let registry = device.id().map_err(|e| e.to_string())?.0;
    let descriptor = super::super::inventory::descriptor(&device)?;
    let parsed = descriptor
        .as_deref()
        .map(parse_descriptor)
        .transpose()
        .map_err(|e| format!("Invalid HID descriptor: {e:?}"))?;
    let max = row["maxInputReportBytes"].as_u64().unwrap_or(0);
    if max > osg_hid_report::MAX_REPORT_BYTES as u64 {
        return Err("IOHID input report exceeds capture capacity".into());
    }
    let (tx, rx) = mpsc::sync_channel(256);
    let removed = Arc::new(AtomicBool::new(false));
    let overflow = Arc::new(AtomicBool::new(false));
    let budget = Arc::new(AtomicUsize::new(0));
    let flag = removed.clone();
    let removal = device
        .on_removal(move || {
            flag.store(true, Ordering::Release);
        })
        .map_err(|e| error(e).to_string())?;
    let report = if max > 0 {
        let lost = overflow.clone();
        let bytes = budget.clone();
        let sender = tx.clone();
        match device.on_input_report_with_timestamp(move |r| {
            if r.report_type != HidReportType::Input {
                return;
            }
            if r.bytes.len() > osg_hid_report::MAX_REPORT_BYTES {
                lost.store(true, Ordering::Release);
                return;
            }
            enqueue(
                &sender,
                &bytes,
                &lost,
                Event::Report {
                    bytes: r.bytes,
                    report_id: r.report_id,
                    timestamp: r.timestamp,
                },
            );
        }) {
            Ok(subscription) => Some(subscription),
            Err(e) if error(e.clone())["kind"] == "unsupported" => None,
            Err(e) => return Err(error(e).to_string()),
        }
    } else {
        None
    };
    // Both subscriptions refer to this concrete service. Element callbacks retain
    // their own cookie/ID/timestamp; no same-packet relationship is inferred.
    let lost = overflow.clone();
    let bytes = budget.clone();
    let value = match device.on_input_value(&[], move |v| {
        let element = v.element();
        if !matches!(
            element.element_kind(),
            HidElementType::InputMisc
                | HidElementType::InputButton
                | HidElementType::InputAxis
                | HidElementType::InputScanCodes
                | HidElementType::InputNull
        ) {
            return;
        }
        if v.len() > osg_hid_report::MAX_REPORT_BYTES {
            lost.store(true, Ordering::Release);
            return;
        }
        let raw = v.bytes();
        let integer = (!element.is_array()
            && element.report_count() == 1
            && element.report_size_bits() <= isize::BITS
            && raw.len() <= std::mem::size_of::<isize>())
        .then(|| v.integer_value().to_string());
        let sample = json!({
            "cookie": element.cookie(),
            "usagePage": element.usage_page(),
            "usage": element.usage(),
            "integer": integer,
            "valueHex": hex(&raw),
            "relative": element.is_relative(),
            "array": element.is_array(),
            "logicalMin": element.logical_min().to_string(),
            "logicalMax": element.logical_max().to_string(),
            "reportSizeBits": element.report_size_bits(),
            "reportCount": element.report_count()
        });
        enqueue(
            &tx,
            &bytes,
            &lost,
            Event::Element {
                sample,
                report_id: element.report_id(),
                timestamp: v.timestamp(),
            },
        );
    }) {
        Ok(subscription) => Some(subscription),
        Err(e) if report.is_some() && error(e.clone())["kind"] == "unsupported" => None,
        Err(e) => return Err(error(e).to_string()),
    };
    let handle = Handle {
        device,
        registry,
        id: id.into(),
        row,
        descriptor,
        rx,
        removed,
        overflow,
        budget,
        permission_granted: Cell::new(request::access_granted()),
        last_verified: Cell::new(Instant::now()),
        report,
        value,
        removal: Some(removal),
    };
    handle.verify().map_err(|e| e.to_string())?;
    Ok(Box::new(Capture::new(handle, parsed)))
}
struct Handle {
    device: HidDevice,
    registry: u64,
    id: String,
    row: Value,
    descriptor: Option<Vec<u8>>,
    rx: mpsc::Receiver<(Event, usize)>,
    removed: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
    budget: Arc<AtomicUsize>,
    permission_granted: Cell<bool>,
    last_verified: Cell<Instant>,
    report: Option<TimestampedReportSubscription>,
    value: Option<ValueSubscription>,
    removal: Option<DeviceRemovalSubscription>,
}
impl Transport for Handle {
    fn verify(&self) -> Result<(), Value> {
        if self.removed.load(Ordering::Acquire) {
            return Err(failure("removed", "Selected IOHID interface disconnected"));
        }
        if self.overflow.load(Ordering::Acquire) {
            return Err(failure(
                "overflow",
                "IOHID callback queue exceeded capacity; generation ended",
            ));
        }
        let granted = request::access_granted();
        if self.permission_granted.get() && !granted {
            return Err(failure(
                "permission-denied",
                "Input Monitoring access was revoked",
            ));
        }
        if granted {
            self.permission_granted.set(true);
        }
        // Cheap held-device evidence is checked for every delivery and GET;
        // namespace enumeration is throttled only while idle.
        if self.device.id().map_err(error)?.0 != self.registry
            || super::super::inventory::descriptor(&self.device)
                .map_err(|e| failure("stale-generation", e))?
                != self.descriptor
        {
            return Err(failure(
                "stale-generation",
                "Selected IOHID handle identity or descriptor changed",
            ));
        }
        if self.last_verified.get().elapsed() >= Duration::from_millis(100) {
            let (fresh, row) = super::super::inventory::selected(&self.id).map_err(|e| {
                let kind = if e.contains("Restricted") {
                    "permission-denied"
                } else if e.contains("changed") || e.contains("Unknown") {
                    "stale-generation"
                } else {
                    "removed"
                };
                failure(kind, e)
            })?;
            if row["generationEvidence"] != self.row["generationEvidence"]
                || fresh.id().map_err(error)?.0 != self.registry
            {
                return Err(failure(
                    "stale-generation",
                    "Selected IOHID identity or descriptor changed",
                ));
            }
            self.last_verified.set(Instant::now());
        }
        Ok(())
    }
    fn poll(&mut self, timeout: Duration) -> Result<Option<Event>, Value> {
        match self.rx.recv_timeout(timeout) {
            Ok((event, size)) => {
                self.budget.fetch_sub(size, Ordering::Release);
                Ok(Some(event))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(failure("offline", "IOHID callback subscription ended"))
            }
        }
    }
    fn get(&mut self, req: &Get, length: usize, timeout: Duration) -> Result<Vec<u8>, Value> {
        request::get(&self.device, &self.id, req, length, timeout)
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        drop(self.report.take());
        drop(self.value.take());
        drop(self.removal.take());
    }
}
fn error(e: HidError) -> Value {
    let kind = match &e {
        HidError::Timeout(_) => "timeout",
        HidError::Unsupported(_) => "unsupported",
        HidError::IoReturn(_, status) | HidError::DeviceOpenFailed(status) => {
            request::status_kind(*status)
        }
        _ => "io-error",
    };
    failure(kind, e.to_string())
}

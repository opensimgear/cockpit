use super::*;
pub(super) struct Io {
    pub start: fn(&File, &mut Pending, Option<ReportKind>) -> Result<(), ReceiveError>,
    pub complete: fn(&File, &mut Pending) -> Option<Result<usize, ReceiveError>>,
    pub wait_event: fn(&Pending, Duration) -> Result<bool, ReceiveError>,
    pub cancel: fn(&File, &Pending),
}
pub(super) static NATIVE: Io = Io {
    start,
    complete,
    wait_event,
    cancel,
};
fn start(file: &File, pending: &mut Pending, kind: Option<ReportKind>) -> Result<(), ReceiveError> {
    let length = pending.bytes.len() as u32;
    let pointer = pending.bytes.as_mut_ptr();
    let ok = if let Some(kind) = kind {
        let control = match kind {
            ReportKind::Input => GET_INPUT,
            ReportKind::Feature => GET_FEATURE,
            ReportKind::Output => {
                return Err(error(
                    ErrorKind::InvalidRequest,
                    "No output IOCTL exists in receive path",
                ))
            }
        };
        unsafe {
            DeviceIoControl(
                file.handle,
                control,
                pointer.cast(),
                length,
                pointer.cast(),
                length,
                null_mut(),
                &mut pending.overlap,
            )
        }
    } else {
        unsafe {
            ReadFile(
                file.handle,
                pointer,
                length,
                null_mut(),
                &mut pending.overlap,
            )
        }
    };
    if ok == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
        Err(last("Start read-only HID operation"))
    } else {
        Ok(())
    }
}
fn complete(file: &File, pending: &mut Pending) -> Option<Result<usize, ReceiveError>> {
    let mut count = 0;
    if unsafe { GetOverlappedResult(file.handle, &pending.overlap, &mut count, 0) } != 0 {
        return Some(Ok(count as usize));
    }
    let code = unsafe { GetLastError() };
    if code == ERROR_IO_INCOMPLETE || code == ERROR_IO_PENDING {
        None
    } else {
        Some(Err(policy::os(code, "Complete HID receive")))
    }
}
fn wait_event(pending: &Pending, deadline: Duration) -> Result<bool, ReceiveError> {
    match unsafe { WaitForSingleObject(pending.overlap.hEvent, deadline.as_millis() as u32) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(last("Wait for HID receive")),
    }
}
fn cancel(file: &File, pending: &Pending) {
    unsafe {
        CancelIoEx(file.handle, &pending.overlap);
    }
}

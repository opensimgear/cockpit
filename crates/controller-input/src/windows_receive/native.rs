use super::*;
#[path = "io.rs"]
mod io;
#[cfg(test)]
#[path = "kernel_tests.rs"]
mod kernel_tests;
#[cfg(test)]
#[path = "receiver_tests.rs"]
mod receiver_tests;
#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
use std::{
    mem::size_of,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use windows_sys::Win32::{
    Devices::HumanInterfaceDevice::*,
    Foundation::*,
    Storage::FileSystem::*,
    System::{Threading::*, IO::*},
};

const SESSION_LIMIT: usize = 8;
static SLOTS: AtomicUsize = AtomicUsize::new(0);
static RETIRED: Mutex<Vec<Retired>> = Mutex::new(Vec::new());
// HID_OUT_DIRECT_CTL_CODE(100/104), hidclass.h. These are GET-only controls.
const GET_FEATURE: u32 = (0x0b << 16) | (100 << 2) | 2;
const GET_INPUT: u32 = (0x0b << 16) | (104 << 2) | 2;
struct OpenApi {
    open: unsafe extern "system" fn(
        *const u16,
        u32,
        u32,
        *const windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
        u32,
        u32,
        HANDLE,
    ) -> HANDLE,
    attributes: unsafe extern "system" fn(HANDLE, *mut HIDD_ATTRIBUTES) -> bool,
    preparsed: unsafe extern "system" fn(HANDLE, *mut isize) -> bool,
    caps: unsafe extern "system" fn(isize, *mut HIDP_CAPS) -> i32,
    snapshot: unsafe fn(isize) -> Snapshot,
    io: &'static io::Io,
}
static OPEN_NATIVE: OpenApi = OpenApi {
    open: CreateFileW,
    attributes: HidD_GetAttributes,
    preparsed: HidD_GetPreparsedData,
    caps: HidP_GetCaps,
    snapshot: windows_caps::snapshot,
    io: &io::NATIVE,
};
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        SLOTS.fetch_sub(1, Ordering::AcqRel);
    }
}
struct File {
    handle: HANDLE,
    path: String,
    pp: isize,
    _permit: Permit,
    io: &'static io::Io,
}
// Kernel handles permit concurrent independent overlapped operations. Each
// operation's user memory remains exclusively owned by its stable Box.
unsafe impl Send for File {}
unsafe impl Sync for File {}
impl Drop for File {
    fn drop(&mut self) {
        unsafe {
            if self.pp != 0 {
                HidD_FreePreparsedData(self.pp);
            }
            if self.handle != INVALID_HANDLE_VALUE {
                CloseHandle(self.handle);
            }
        }
    }
}
struct Pending {
    overlap: OVERLAPPED,
    bytes: Vec<u8>,
}
unsafe impl Send for Pending {}
impl Pending {
    fn new(length: usize) -> Result<Box<Self>, ReceiveError> {
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            return Err(last("Create HID completion event"));
        }
        let mut overlap = OVERLAPPED::default();
        overlap.hEvent = event;
        Ok(Box::new(Self {
            overlap,
            bytes: vec![0; length],
        }))
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.overlap.hEvent);
        }
    }
}
struct Retired {
    file: Arc<File>,
    pending: Vec<Box<Pending>>,
}
pub(super) struct Session {
    file: Arc<File>,
    stream: Option<Box<Pending>>,
    get: Option<Box<Pending>>,
}

fn last(context: &str) -> ReceiveError {
    policy::os(unsafe { GetLastError() }, context)
}
fn completion(file: &File, pending: &mut Pending) -> Option<Result<usize, ReceiveError>> {
    (file.io.complete)(file, pending)
}
fn reap() {
    let mut retired = RETIRED.lock().unwrap_or_else(|e| e.into_inner());
    retired.retain_mut(|entry| {
        entry
            .pending
            .retain_mut(|pending| completion(&entry.file, pending).is_none());
        !entry.pending.is_empty()
    });
}
fn wait(
    file: &File,
    pending: &mut Pending,
    deadline: Duration,
) -> Result<Option<usize>, ReceiveError> {
    if let Some(result) = completion(file, pending) {
        return result.map(Some);
    }
    match (file.io.wait_event)(pending, deadline)? {
        true => completion(file, pending)
            .unwrap_or_else(|| {
                Err(error(
                    ErrorKind::Os,
                    "HID event signaled without completion",
                ))
            })
            .map(Some),
        false => Ok(None),
    }
}
impl Session {
    pub(super) fn open(selected: &Selection) -> Result<(Self, Snapshot), ReceiveError> {
        Self::open_with(selected, &OPEN_NATIVE)
    }
    fn open_with(selected: &Selection, api: &OpenApi) -> Result<(Self, Snapshot), ReceiveError> {
        reap();
        if RETIRED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|entry| entry.file.path.eq_ignore_ascii_case(&selected.path))
        {
            return Err(error(
                ErrorKind::Busy,
                "A cancelled operation for this HID interface has not completed",
            ));
        }
        SLOTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < SESSION_LIMIT).then_some(count + 1)
            })
            .map_err(|_| {
                error(
                    ErrorKind::Capacity,
                    "HID receive sessions or cancelled driver operations exceed capacity",
                )
            })?;
        let permit = Permit;
        let path: Vec<u16> = selected.path.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe {
            (api.open)(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(last("Open shared read-only HID collection"));
        }
        let mut file = File {
            handle,
            path: selected.path.clone(),
            pp: 0,
            _permit: permit,
            io: api.io,
        };
        let mut attributes = HIDD_ATTRIBUTES::default();
        attributes.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
        if !unsafe { (api.attributes)(handle, &mut attributes) } {
            return Err(last("Verify opened HID identity"));
        }
        if (
            attributes.VendorID,
            attributes.ProductID,
            attributes.VersionNumber,
        ) != (
            selected.vendor_id,
            selected.product_id,
            selected.version_number,
        ) {
            return Err(error(
                ErrorKind::Removed,
                "Selected HID interface identity changed before open",
            ));
        }
        let mut preparsed = 0;
        if !unsafe { (api.preparsed)(handle, &mut preparsed) } {
            return Err(last("Read live HID preparsed data"));
        }
        file.pp = preparsed;
        let mut caps = HIDP_CAPS::default();
        if unsafe { (api.caps)(file.pp, &mut caps) } != HIDP_STATUS_SUCCESS {
            return Err(error(ErrorKind::Unsupported, "Live HID caps unavailable"));
        }
        if (
            caps.UsagePage,
            caps.Usage,
            caps.InputReportByteLength as usize,
            caps.FeatureReportByteLength as usize,
        ) != (
            selected.usage_page,
            selected.usage,
            selected.input_report_bytes,
            selected.feature_report_bytes,
        ) {
            return Err(error(
                ErrorKind::Removed,
                "Selected HID collection identity or report lengths changed",
            ));
        }
        let snapshot = unsafe { (api.snapshot)(file.pp) };
        Ok((
            Self {
                file: Arc::new(file),
                stream: None,
                get: None,
            },
            snapshot,
        ))
    }
    pub(super) fn preparsed(&self) -> isize {
        self.file.pp
    }
    pub(super) fn poll(
        &mut self,
        length: usize,
        deadline: Duration,
    ) -> Result<Option<Vec<u8>>, ReceiveError> {
        reap();
        if self.stream.is_none() {
            let mut pending = Pending::new(length)?;
            (self.file.io.start)(&self.file, &mut pending, None)?;
            self.stream = Some(pending);
        }
        let result = wait(&self.file, self.stream.as_mut().unwrap(), deadline)?;
        let Some(count) = result else {
            return Ok(None);
        };
        let pending = self.stream.take().unwrap();
        if count == 0 {
            return Err(error(
                ErrorKind::Removed,
                "HID stream completed with no report bytes",
            ));
        }
        if count > pending.bytes.len() {
            return Err(error(
                ErrorKind::Os,
                "HID stream completion exceeded its buffer",
            ));
        }
        Ok(Some(pending.bytes[..count].to_vec()))
    }
    pub(super) fn get(
        &mut self,
        kind: ReportKind,
        id: u8,
        length: usize,
        deadline: Duration,
    ) -> Result<Vec<u8>, ReceiveError> {
        reap();
        if let Some(pending) = self.get.as_mut() {
            if completion(&self.file, pending).is_none() {
                return Err(error(
                    ErrorKind::Busy,
                    "Previous cancelled HID GET has not completed",
                ));
            }
            self.get.take();
        }
        let mut pending = Pending::new(length)?;
        pending.bytes[0] = id;
        (self.file.io.start)(&self.file, &mut pending, Some(kind))?;
        self.get = Some(pending);
        match wait(&self.file, self.get.as_mut().unwrap(), deadline)? {
            Some(count) => {
                let pending = self.get.take().unwrap();
                if count == 0 {
                    return Err(error(
                        ErrorKind::Os,
                        "HID GET completed without report data",
                    ));
                }
                // HIDClass excludes its synthetic zero prefix from the transfer
                // count for unnumbered GET reports; preserve that framing byte.
                let count = count
                    .checked_add(usize::from(id == 0))
                    .ok_or_else(|| error(ErrorKind::Capacity, "HID GET length overflow"))?;
                if count > pending.bytes.len() {
                    return Err(error(
                        ErrorKind::Os,
                        "HID GET completion exceeded its buffer",
                    ));
                }
                Ok(pending.bytes[..count].to_vec())
            }
            None => {
                (self.file.io.cancel)(&self.file, self.get.as_ref().unwrap());
                Err(error(
                    ErrorKind::Timeout,
                    "HID GET deadline expired; late completion is fenced",
                ))
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        for mut operation in [self.stream.take(), self.get.take()].into_iter().flatten() {
            (self.file.io.cancel)(&self.file, &operation);
            if completion(&self.file, &mut operation).is_none() {
                pending.push(operation);
            }
        }
        if !pending.is_empty() {
            RETIRED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Retired {
                    file: Arc::clone(&self.file),
                    pending,
                });
        }
        // At most SESSION_LIMIT retained sessions; their permits remain held.
        // No kernel wait here. Later calls reap completed cancellations safely.
    }
}

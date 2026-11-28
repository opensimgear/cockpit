use super::*;

pub(super) fn deadline(deadline: Duration) -> Result<(), ReceiveError> {
    if deadline > MAX_DEADLINE {
        return Err(error(
            ErrorKind::InvalidRequest,
            "HID deadline exceeds two seconds",
        ));
    }
    Ok(())
}
pub(super) fn selection(selection: &Selection) -> Result<(), ReceiveError> {
    if metadata_only(selection.usage_page, selection.usage) {
        return Err(error(
            ErrorKind::MetadataOnly,
            "Keyboard, mouse and system collections are metadata only",
        ));
    }
    if selection.path.is_empty() || selection.path.len() > 8192 || selection.path.contains('\0') {
        return Err(error(
            ErrorKind::InvalidRequest,
            "Invalid selected HID interface path",
        ));
    }
    if selection.input_report_bytes > MAX_REPORT_BYTES
        || selection.feature_report_bytes > MAX_REPORT_BYTES
    {
        return Err(error(
            ErrorKind::Capacity,
            "HID report length exceeds receive capacity",
        ));
    }
    Ok(())
}
pub(super) fn get(
    request: &str,
    id: u8,
    max: usize,
    timeout: Duration,
    length: usize,
    ids: &[u8],
) -> Result<(), ReceiveError> {
    deadline(timeout)?;
    if request.is_empty() || request.len() > 80 || request.chars().any(char::is_control) {
        return Err(error(
            ErrorKind::InvalidRequest,
            "A bounded request ID is required",
        ));
    }
    if length == 0 || !ids.contains(&id) {
        return Err(error(
            ErrorKind::Unsupported,
            "GET kind/report ID is not advertised by live preparsed caps",
        ));
    }
    if max < length || max > MAX_REPORT_BYTES {
        return Err(error(
            ErrorKind::InvalidRequest,
            "GET buffer bound does not cover the declared report length",
        ));
    }
    Ok(())
}
pub(super) fn completed_get(id: u8, length: usize, bytes: &[u8]) -> Result<(), ReceiveError> {
    if bytes.len() < 2 || bytes.len() > length || bytes[0] != id {
        return Err(error(
            ErrorKind::Os,
            "GET returned an invalid report length or report ID",
        ));
    }
    Ok(())
}
pub(super) fn os(code: u32, context: &str) -> ReceiveError {
    use windows_sys::Win32::Foundation::*;
    let kind = match code {
        ERROR_DEVICE_NOT_CONNECTED | ERROR_NO_SUCH_DEVICE | ERROR_INVALID_HANDLE => {
            ErrorKind::Removed
        }
        ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION => ErrorKind::AccessDenied,
        ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION => ErrorKind::Unsupported,
        _ => ErrorKind::Os,
    };
    ReceiveError {
        kind,
        code: Some(code),
        message: format!(
            "{context}: {}",
            std::io::Error::from_raw_os_error(code as i32)
        ),
    }
}

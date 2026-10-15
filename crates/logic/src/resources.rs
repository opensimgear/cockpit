//! Resource limits complement interpreter budgets; this is not an OS permission sandbox.
#[cfg(unix)]
pub fn child_limits() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    crate::HELPER_HEAP.limit(256 * 1024 * 1024)?;
    // Keep memory allocation and accidental descriptor growth bounded before accepting code.
    for (resource, limit) in [
        #[cfg(not(target_os = "macos"))]
        (libc::RLIMIT_DATA, 256 * 1024 * 1024),
        (libc::RLIMIT_NOFILE, 32),
    ] {
        let mut inherited = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(resource, &mut inherited) } != 0 {
            return Err(format!(
                "Cannot inspect script resource {resource}: {}",
                std::io::Error::last_os_error()
            ));
        }
        let limit = limit.min(inherited.rlim_cur).min(inherited.rlim_max);
        let value = libc::rlimit {
            rlim_cur: limit,
            rlim_max: limit,
        };
        if unsafe { libc::setrlimit(resource, &value) } != 0 {
            return Err(format!(
                "Cannot enforce script resource {resource} limit {limit}: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}
#[cfg(not(unix))]
pub fn child_limits() -> Result<(), String> {
    Ok(())
}
#[cfg(windows)]
pub struct Guard {
    _handle: std::os::windows::io::OwnedHandle,
}
#[cfg(not(windows))]
pub struct Guard;
#[cfg(windows)]
impl Guard {
    pub fn attach(child: &std::process::Child) -> Result<Self, String> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::*;
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(job) };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = 256 * 1024 * 1024;
        if unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
            || unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self { _handle: handle })
    }
}
#[cfg(not(windows))]
impl Guard {
    pub fn attach(_child: &std::process::Child) -> Result<Self, String> {
        Ok(Self)
    }
}

//! Explicit Windows elevation and exact child lifetime. No shell command line or
//! user-selected executable is accepted; the native UI decides when to request it.
use crate::{registry::wide, windows_coordination::Cancellation, windows_process::Identity};
use std::{path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Com::*,
        Threading::{GetExitCodeProcess, WaitForSingleObject},
    },
    UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
};
pub const ARGUMENT: &str = "--osg-vjoy-provision";
pub const QUALIFIED: bool = false;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Create(u8),
    CreateLayout {
        slot: u8,
        layout: crate::descriptor::Layout,
    },
    Recover,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub operation: String,
    pub owner: Identity,
    pub action: Action,
}
impl Request {
    fn validate(&self) -> Result<(), String> {
        self.owner.validate()?;
        if let Action::CreateLayout { slot, layout } = self.action {
            layout.descriptor(slot)?;
        }
        if uuid::Uuid::parse_str(&self.operation)
            .ok()
            .map(|u| u.to_string())
            .as_deref()
            != Some(&self.operation)
            || matches!(self.action, Action::Create(id) if !(1..=16).contains(&id))
        {
            return Err("Invalid bounded provisioning request.".into());
        }
        Ok(())
    }
    pub fn arguments(&self) -> Result<String, String> {
        self.validate()?;
        let action = match self.action {
            Action::Create(id) => id.to_string(),
            Action::CreateLayout { slot, layout } => {
                format!("{slot}:{}:{}:{}", layout.axes, layout.buttons, layout.hats)
            }
            Action::Recover => "recover".into(),
        };
        Ok(format!(
            "{ARGUMENT} {} {} {} {action}",
            self.operation, self.owner.pid, self.owner.created
        ))
    }
    pub fn parse(args: &[String]) -> Result<Self, String> {
        if args.len() != 5 || args[0] != ARGUMENT || args.iter().any(|a| a.len() > 64) {
            return Err("Invalid provisioning helper arguments.".into());
        }
        let pid = args[2].parse::<u32>().map_err(|_| "Invalid owner PID.")?;
        if pid.to_string() != args[2] {
            return Err("Owner PID is not canonical.".into());
        }
        let action = if args[4] == "recover" {
            Action::Recover
        } else if args[4].contains(':') {
            let parts: Vec<_> = args[4].split(':').collect();
            if parts.len() != 4 {
                return Err("Invalid layout request.".into());
            }
            let slot = parts[0].parse::<u8>().map_err(|_| "Invalid slot.")?;
            let layout = crate::descriptor::Layout {
                axes: parts[1].parse::<u16>().map_err(|_| "Invalid axes.")?,
                buttons: parts[2].parse::<u8>().map_err(|_| "Invalid buttons.")?,
                hats: parts[3].parse::<u8>().map_err(|_| "Invalid hats.")?,
            };
            if format!("{slot}:{}:{}:{}", layout.axes, layout.buttons, layout.hats) != args[4] {
                return Err("Layout request is not canonical.".into());
            }
            Action::CreateLayout { slot, layout }
        } else {
            let target = args[4].parse::<u8>().map_err(|_| "Invalid target slot.")?;
            if target.to_string() != args[4] {
                return Err("Target slot is not canonical.".into());
            }
            Action::Create(target)
        };
        let request = Self {
            operation: args[1].clone(),
            owner: Identity {
                pid,
                created: args[3].clone(),
            },
            action,
        };
        request.validate()?;
        Ok(request)
    }
}
struct Com;
impl Com {
    fn initialize() -> Result<Self, String> {
        let code = unsafe {
            CoInitializeEx(
                ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        if code < 0 {
            return Err(format!(
                "Windows elevation requires a COM apartment ({code:#x})."
            ));
        }
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
pub struct Child(HANDLE);
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Child {
    /// Do not kill an elevated operation to cancel it: observe this exact handle.
    pub fn poll(&self) -> Result<Option<u32>, String> {
        match unsafe { WaitForSingleObject(self.0, 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.0, &mut code) } == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                Ok(Some(code))
            }
            _ => Err(std::io::Error::last_os_error().to_string()),
        }
    }
}
fn execute(program: &Path, parameters: &str, verb: &str) -> Result<Child, String> {
    let _com = Com::initialize()?;
    if !program.is_absolute() || !program.is_file() {
        return Err("Helper executable is unavailable.".into());
    }
    let file = wide(program.to_str().ok_or("Invalid helper executable path.")?)?;
    let parameters = wide(parameters)?;
    let verb = wide(verb)?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(format!(
            "Windows elevation was declined or could not start: {}.",
            std::io::Error::last_os_error()
        ));
    }
    if info.hProcess.is_null() {
        return Err("Windows did not return an owned helper process handle.".into());
    }
    Ok(Child(info.hProcess))
}
pub struct Ticket {
    pub request: Request,
    child: Child,
    cancellation: std::sync::Arc<Cancellation>,
}
impl Ticket {
    pub(crate) fn start(
        request: Request,
        cancellation: std::sync::Arc<Cancellation>,
    ) -> Result<Self, String> {
        let parameters = request.arguments()?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let child = execute(&executable, &parameters, "runas")?;
        Ok(Self {
            request,
            child,
            cancellation,
        })
    }
    pub fn cancel(&self) -> Result<(), String> {
        self.cancellation.cancel()
    }
    pub fn poll(&self) -> Result<Option<u32>, String> {
        self.child.poll()
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        // Preserve an in-flight refresh and let the helper roll back cooperatively.
        let _ = self.cancellation.cancel();
    }
}
pub fn launch(action: Action, recovery_id: Option<String>) -> Result<Ticket, String> {
    if !QUALIFIED {
        return Err("Elevated vJoy provisioning remains unqualified and disabled.".into());
    }
    let request = Request {
        operation: recovery_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        owner: crate::windows_process::current()?,
        action,
    };
    let cancellation = std::sync::Arc::new(Cancellation::create(&request.operation)?);
    Ticket::start(request, cancellation)
}
#[cfg(test)]
#[path = "windows_launcher_tests.rs"]
mod tests;

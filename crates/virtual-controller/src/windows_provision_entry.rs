//! Explicit elevated entry. The helper is transient and owns cooperative cleanup;
//! interruption of every process leaves a durable record for explicit recovery.
use crate::{
    provision::{self, Backend, Journal},
    windows_coordination::{Exclusive, Monitor},
    windows_launcher::{Action, Request, ARGUMENT, QUALIFIED},
    windows_provision::{NativeBackend, Outcome},
};
use windows_sys::Win32::{Foundation::CloseHandle, Security::*, System::Threading::*};
trait Operation: Backend {
    fn prepare(&mut self) -> Result<(), String>;
    fn id(&self) -> String;
    fn prior(&self) -> Result<Journal, String>;
    fn finish(&mut self, outcome: &Outcome) -> Result<(), String>;
}
impl Operation for NativeBackend {
    fn prepare(&mut self) -> Result<(), String> {
        self.prepare_journals()?;
        self.ensure_no_other_pending()
    }
    fn id(&self) -> String {
        self.operation().into()
    }
    fn prior(&self) -> Result<Journal, String> {
        self.read_journal()
    }
    fn finish(&mut self, outcome: &Outcome) -> Result<(), String> {
        NativeBackend::finish(self, outcome)
    }
}
fn dispatch(
    backend: &mut impl Operation,
    action: Action,
    mut check: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    check()?;
    // Resolve dependency/layout or existing recovery intent before creating a
    // protected journal directory. Preparation itself performs no driver changes.
    let prepared = match action {
        Action::Create(target) => {
            let mut journal = provision::plan(backend.snapshot()?, target)?;
            journal.id = backend.id();
            journal
        }
        Action::CreateLayout { slot, layout } => {
            let mut journal = provision::plan_layout(backend.snapshot()?, slot, layout)?;
            journal.id = backend.id();
            journal
        }
        Action::Recover => backend.prior()?,
    };
    backend.prepare()?;
    let result = match action {
        Action::Create(_) | Action::CreateLayout { .. } => {
            provision::execute_controlled(backend, prepared, check).map(|_| ())
        }
        Action::Recover => {
            check()?; // Once restoration starts, complete it without cancellation.
            provision::restore(backend, &prepared)
        }
    };
    let outcome = Outcome {
        success: result.is_ok(),
        message: match &result {
            Ok(()) => {
                "Provisioning operation completed with verified configuration readback.".into()
            }
            Err(error) => error.chars().take(512).collect(),
        },
    };
    let stored = backend.finish(&outcome);
    match result {
        Ok(()) => stored,
        Err(error) => Err(match stored {
            Ok(()) => error,
            Err(storage) => format!("{error} Outcome storage failed: {storage}"),
        }),
    }
}
fn elevated() -> Result<bool, String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut length = 0;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    };
    unsafe {
        CloseHandle(token);
    }
    if ok == 0 || length != std::mem::size_of::<TOKEN_ELEVATION>() as u32 {
        return Err("Unable to verify elevated helper token.".into());
    }
    Ok(elevation.TokenIsElevated != 0)
}
fn native(request: Request) -> Result<(), String> {
    if !elevated()? {
        return Err("Explicit administrator authorization is required for provisioning.".into());
    }
    let monitor = Monitor::open(&request.operation, &request.owner)?;
    monitor.check()?;
    let _exclusive = Exclusive::acquire()?;
    let mut backend = NativeBackend::native(&request.operation)?;
    dispatch(&mut backend, request.action, || monitor.check())
}
fn entry(
    args: &[String],
    qualified: bool,
    run: impl FnOnce(Request) -> Result<(), String>,
) -> Option<i32> {
    if args.first().is_none_or(|v| v != ARGUMENT) {
        return None;
    }
    let request = match Request::parse(args) {
        Ok(request) => request,
        Err(_) => return Some(2),
    };
    if !qualified {
        return Some(78);
    }
    Some(if run(request).is_ok() { 0 } else { 1 })
}
pub fn helper_main() -> Option<i32> {
    // Bound the private argument frame before parsing. Ordinary app startup is inert.
    let args: Vec<_> = std::env::args().skip(1).take(6).collect();
    entry(&args, QUALIFIED, native)
}
#[cfg(test)]
#[path = "windows_provision_entry_tests.rs"]
mod tests;

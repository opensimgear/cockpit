//! Minimal free-threaded COM sink. It only copies PIDs into a bounded queue.
use super::windows::{Event, Inbox};
use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use windows::{
    core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT},
    Win32::System::{
        Variant::{VariantClear, VARIANT, VT_I4, VT_UI4},
        Wmi::{IWbemClassObject, IWbemObjectSink, IWbemObjectSink_Vtbl},
    },
};

#[repr(C)]
struct Sink {
    vtable: *const IWbemObjectSink_Vtbl,
    refs: AtomicU32,
    inbox: Arc<Inbox>,
    dropped: bool,
}
static VTABLE: IWbemObjectSink_Vtbl = IWbemObjectSink_Vtbl {
    base__: IUnknown_Vtbl {
        QueryInterface: query,
        AddRef: add_ref,
        Release: release,
    },
    Indicate: indicate,
    SetStatus: status,
};
pub(super) fn create(inbox: Arc<Inbox>, dropped: bool) -> IWbemObjectSink {
    let sink = Box::new(Sink {
        vtable: &VTABLE,
        refs: AtomicU32::new(1),
        inbox,
        dropped,
    });
    // The allocation's initial reference belongs to the returned COM interface.
    unsafe { IWbemObjectSink::from_raw(Box::into_raw(sink).cast()) }
}
unsafe extern "system" fn query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return HRESULT(0x80004003u32 as i32);
    }
    unsafe {
        *out = ptr::null_mut();
    }
    if unsafe { *iid == IUnknown::IID || *iid == IWbemObjectSink::IID } {
        unsafe {
            *out = this;
            add_ref(this);
        }
        HRESULT(0)
    } else {
        HRESULT(0x80004002u32 as i32)
    }
}
unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    unsafe { &*this.cast::<Sink>() }
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let count = unsafe { &*this.cast::<Sink>() }
        .refs
        .fetch_sub(1, Ordering::Release)
        - 1;
    if count == 0 {
        std::sync::atomic::fence(Ordering::Acquire);
        unsafe {
            drop(Box::from_raw(this.cast::<Sink>()));
        }
    }
    count
}
unsafe extern "system" fn indicate(
    this: *mut c_void,
    count: i32,
    objects: *const *mut c_void,
) -> HRESULT {
    let sink = unsafe { &*this.cast::<Sink>() };
    if sink.dropped {
        sink.inbox.push(Event::Failed(
            "WMI dropped a process notification; X-Plane ownership is no longer proven.".into(),
        ));
    } else if !(1..=1024).contains(&count) || objects.is_null() {
        sink.inbox.push(Event::Failed(
            "WMI supplied an invalid process notification batch.".into(),
        ));
    } else {
        for raw in unsafe { std::slice::from_raw_parts(objects, count as usize) } {
            // Borrow the callback's interface without taking ownership of its reference.
            let event = if raw.is_null() {
                Event::Failed("WMI supplied an empty process notification.".into())
            } else {
                let object = unsafe { IWbemClassObject::from_raw_borrowed(raw) }.unwrap();
                match process_id(object) {
                    Ok(pid) => Event::Started(pid),
                    Err(error) => Event::Failed(error),
                }
            };
            sink.inbox.push(event);
        }
    }
    HRESULT(0)
}
unsafe extern "system" fn status(
    this: *mut c_void,
    flags: i32,
    result: HRESULT,
    _: *mut c_void,
    _: *mut c_void,
) -> HRESULT {
    // WBEM_STATUS_COMPLETE (0), including S_OK, means our endless subscription ended.
    if flags == 0 || result.is_err() {
        unsafe { &*this.cast::<Sink>() }
            .inbox
            .push(Event::Failed(format!(
                "The WMI process notification subscription ended ({result:?})."
            )));
    }
    HRESULT(0)
}
fn process_id(object: &IWbemClassObject) -> Result<u32, String> {
    read_pid(|value| unsafe { object.Get(windows::core::w!("ProcessID"), 0, value, None, None) })
}
fn read_pid<F>(reader: F) -> Result<u32, String>
where
    F: FnOnce(&mut VARIANT) -> windows::core::Result<()>,
{
    let mut value = VARIANT::default();
    let read = reader(&mut value);
    let pid = if read.is_ok() {
        let inner = unsafe { &value.Anonymous.Anonymous };
        match inner.vt {
            VT_UI4 => Some(unsafe { inner.Anonymous.ulVal }),
            VT_I4 => Some(unsafe { inner.Anonymous.lVal } as u32),
            _ => None,
        }
    } else {
        None
    };
    let cleared = unsafe { VariantClear(&mut value) };
    if cleared.is_err() {
        return Err("Cannot release the WMI process notification value.".into());
    }
    pid.filter(|pid| *pid != 0)
        .ok_or_else(|| "WMI supplied an invalid process identity.".into())
}

#[cfg(test)]
#[path = "sink_payload_tests.rs"]
mod payload_tests;

#[cfg(test)]
mod tests {
    use super::super::windows::{test_drain, test_inbox};
    use super::*;

    #[test]
    fn com_reference_lifetime_retains_the_callback_queue() {
        let (inbox, _) = test_inbox(4);
        let sink = create(inbox.clone(), false);
        assert_eq!(Arc::strong_count(&inbox), 2);
        let retained = sink.clone();
        drop(sink);
        assert_eq!(Arc::strong_count(&inbox), 2);
        drop(retained);
        assert_eq!(Arc::strong_count(&inbox), 1);
    }

    #[test]
    fn invalid_callback_and_successful_subscription_completion_fail_closed() {
        let (inbox, receiver) = test_inbox(4);
        let sink = create(inbox.clone(), false);
        unsafe {
            sink.Indicate(&[None]).unwrap();
        }
        assert!(test_drain(&inbox, &receiver, 42)
            .unwrap_err()
            .contains("empty"));
        unsafe {
            sink.SetStatus(0, HRESULT(0), &windows::core::BSTR::new(), None)
                .unwrap();
        }
        assert!(test_drain(&inbox, &receiver, 42)
            .unwrap_err()
            .contains("ended"));
    }

    #[test]
    fn provider_loss_notification_is_terminal_without_reading_its_payload() {
        let (inbox, receiver) = test_inbox(4);
        let sink = create(inbox.clone(), true);
        unsafe {
            sink.Indicate(&[None]).unwrap();
        }
        assert!(test_drain(&inbox, &receiver, 42)
            .unwrap_err()
            .contains("dropped"));
    }
}

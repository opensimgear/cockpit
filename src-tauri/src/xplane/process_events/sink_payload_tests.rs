use super::super::windows::{test_drain, test_inbox};
use super::*;
use windows::{
    core::{BSTR, PCWSTR},
    Win32::System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize,
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
        },
        Wmi::{IWbemLocator, WbemLocator, WBEM_GENERIC_FLAG_TYPE},
    },
};

struct Apartment;
impl Apartment {
    fn new() -> Self {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.unwrap();
        Self
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
fn instance(class: &str) -> IWbemClassObject {
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }.unwrap();
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from("ROOT\\CIMV2"),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        )
    }
    .unwrap();
    unsafe {
        CoSetProxyBlanket(
            &services,
            10,
            0,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .unwrap();
    let mut class_object = None;
    unsafe {
        services.GetObject(
            &BSTR::from(class),
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut class_object),
            None,
        )
    }
    .unwrap();
    unsafe { class_object.unwrap().SpawnInstance(0) }.unwrap()
}
fn signed(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    let inner = unsafe { &mut variant.Anonymous.Anonymous };
    inner.vt = VT_I4;
    inner.Anonymous.lVal = value;
    variant
}

#[test]
fn typed_pid_reader_accepts_signed_or_unsigned_uint32_and_rejects_missing_or_failed_values() {
    assert_eq!(
        read_pid(|value| {
            *value = signed(42);
            Ok(())
        })
        .unwrap(),
        42
    );
    assert_eq!(
        read_pid(|value| {
            let inner = unsafe { &mut value.Anonymous.Anonymous };
            inner.vt = VT_UI4;
            inner.Anonymous.ulVal = u32::MAX;
            Ok(())
        })
        .unwrap(),
        u32::MAX
    );
    assert!(read_pid(|value| {
        *value = signed(0);
        Ok(())
    })
    .is_err());
    assert!(read_pid(|_| Ok(())).is_err());
    assert!(read_pid(|_| Err(windows::core::Error::from_hresult(HRESULT(
        0x80004005u32 as i32
    ))))
    .is_err());
}

#[test]
fn legitimate_in_memory_wmi_payload_reaches_the_sink_and_rejects_unset_or_absent_pid() {
    let _apartment = Apartment::new();
    let (inbox, receiver) = test_inbox(16);
    let sink = create(inbox.clone(), false);
    let object = instance("Win32_ProcessStartTrace");
    let mut value = signed(42);
    // This edits only an unsaved client-side instance. No PutInstance/provider
    // method is called and no process-event subscription is requested.
    unsafe { object.Put(windows::core::w!("ProcessID"), 0, &value, 0) }.unwrap();
    unsafe { sink.Indicate(&[Some(object.clone())]) }.unwrap();
    test_drain(&inbox, &receiver, 42).unwrap();
    assert_eq!(process_id(&object).unwrap(), 42);
    unsafe { VariantClear(&mut value) }.unwrap();
    unsafe { sink.Indicate(&[Some(instance("Win32_ProcessStartTrace"))]) }.unwrap();
    assert!(test_drain(&inbox, &receiver, 42)
        .unwrap_err()
        .contains("invalid process identity"));
    unsafe { sink.Indicate(&[Some(instance("Win32_OperatingSystem"))]) }.unwrap();
    assert!(test_drain(&inbox, &receiver, 42)
        .unwrap_err()
        .contains("invalid process identity"));
}

#[test]
fn batch_bounds_and_nonterminal_status_are_enforced_on_the_real_sink_interface() {
    let (inbox, receiver) = test_inbox(16);
    let sink = create(inbox.clone(), false);
    unsafe { sink.Indicate(&[]) }.unwrap();
    assert!(test_drain(&inbox, &receiver, 42)
        .unwrap_err()
        .contains("invalid process notification batch"));
    unsafe { sink.Indicate(&vec![None; 1025]) }.unwrap();
    assert!(test_drain(&inbox, &receiver, 42)
        .unwrap_err()
        .contains("invalid process notification batch"));
    assert!(unsafe { (Interface::vtable(&sink).Indicate)(sink.as_raw(), 1, ptr::null()) }.is_ok());
    assert!(test_drain(&inbox, &receiver, 42)
        .unwrap_err()
        .contains("invalid process notification batch"));
    unsafe { sink.SetStatus(1, HRESULT(0), &BSTR::new(), None) }.unwrap();
    test_drain(&inbox, &receiver, 42).unwrap();
    unsafe { sink.SetStatus(1, HRESULT(0x80004005u32 as i32), &BSTR::new(), None) }.unwrap();
    assert!(test_drain(&inbox, &receiver, 42)
        .unwrap_err()
        .contains("ended"));
}

#[test]
fn query_interface_accepts_only_supported_interfaces_and_valid_output_parameters() {
    let (inbox, _) = test_inbox(4);
    let sink = create(inbox, false);
    let unknown: IUnknown = sink.cast().unwrap();
    drop(unknown);
    assert!(sink.cast::<IWbemLocator>().is_err());
    let mut output = ptr::null_mut();
    let vtable = Interface::vtable(&sink);
    assert!(
        unsafe { (vtable.base__.QueryInterface)(sink.as_raw(), ptr::null(), &mut output) }.is_err()
    );
    assert!(unsafe {
        (vtable.base__.QueryInterface)(sink.as_raw(), &IUnknown::IID, ptr::null_mut())
    }
    .is_err());
    assert!(output.is_null());
}

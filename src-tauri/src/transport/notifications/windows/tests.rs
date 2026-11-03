use super::*;

#[test]
fn callback_only_signals_arrival_removal_and_is_inert_after_close() {
    let wake = Arc::new(NativeWake::default());
    let context = Context { wake: wake.clone(), closed: AtomicBool::new(false) };
    for _ in 0..1000 { context.dispatch(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE); }
    assert_eq!(wake.observed(), 0);
    for _ in 0..1000 { context.dispatch(CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL); }
    assert_eq!(wake.observed(), 1000);
    context.dispatch(CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL);
    assert_eq!(wake.observed(), 1001);
    context.closed.store(true, Ordering::Release);
    context.dispatch(CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL);
    assert_eq!(wake.observed(), 1001);
    unsafe { callback(HCMNOTIFICATION::default(), std::ptr::null(), CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, std::ptr::null(), 0); }
}

#[test]
fn failed_registration_is_explicit_and_shutdown_fences_callbacks_before_freeing() {
    let wake = Arc::new(NativeWake::default());
    let result = Serial::register(wake.clone(), |_, _| CONFIGRET(123));
    assert!(matches!(result, Err(error) if error.contains("123")));
    let mut serial = Serial::register(wake.clone(), |context, handle| {
        // The registration API can call back before returning its handle.
        unsafe { callback(HCMNOTIFICATION::default(), context, CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, std::ptr::null(), 0); }
        *handle = HCMNOTIFICATION(1usize as *mut _);
        CR_SUCCESS
    }).unwrap();
    let context = (&**serial.context.as_ref().unwrap() as *const Context).cast();
    serial.shutdown_with(|_| {
        unsafe { callback(HCMNOTIFICATION::default(), context, CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, std::ptr::null(), 0); }
        CR_SUCCESS
    }).unwrap();
    assert_eq!(wake.observed(), 1);
    assert!(serial.context.is_none());
    serial.shutdown_with(|_| panic!("Already unregistered")).unwrap();
}

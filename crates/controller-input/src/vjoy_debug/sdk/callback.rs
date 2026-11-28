use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::SyncSender,
        OnceLock,
    },
};

#[repr(C)]
pub(super) struct FfbData {
    pub size: u32,
    pub command: u32,
    pub data: *const u8,
}
// Windows ULONG is 32 bits even on x64; `size` counts the 8 wire header
// bytes, not sizeof(FFB_DATA), which also contains the native pointer.
const _: () = {
    assert!(std::mem::size_of::<FfbData>() == 16);
    assert!(std::mem::align_of::<FfbData>() == 8);
    assert!(std::mem::offset_of!(FfbData, size) == 0);
    assert!(std::mem::offset_of!(FfbData, command) == 4);
    assert!(std::mem::offset_of!(FfbData, data) == 8);
};
pub(super) enum Message {
    Packet(u32, Vec<u8>),
    Error(String),
    Stop,
}
static ROUTE: OnceLock<SyncSender<Message>> = OnceLock::new();
static SLOT: AtomicU8 = AtomicU8::new(0);
pub(super) static OVERFLOW: AtomicBool = AtomicBool::new(false);

pub(super) fn install(slot: u8, sender: SyncSender<Message>) -> Result<(), String> {
    ROUTE
        .set(sender)
        .map_err(|_| "vJoy helper cannot be reused for another capture.")?;
    SLOT.store(slot, Ordering::Release);
    Ok(())
}
pub(super) fn detach() {
    SLOT.store(0, Ordering::Release);
}

/// The SDK retains packet memory. Copy it completely before returning from
/// its synchronous window-thread callback; never send pointers to the caller.
unsafe fn copy_packet(raw: *mut c_void, slot: u8) -> Result<Option<(u32, Vec<u8>)>, String> {
    if raw.is_null() {
        return Err("vJoy SDK supplied a null FFB packet.".into());
    }
    let packet = std::ptr::read_unaligned(raw.cast::<FfbData>());
    if !(9..=256).contains(&packet.size) || packet.data.is_null() {
        return Err(format!(
            "vJoy SDK supplied an invalid FFB wire size ({}) or pointer.",
            packet.size
        ));
    }
    let bytes = std::slice::from_raw_parts(packet.data, (packet.size - 8) as usize);
    if bytes[0] >> 4 != slot {
        return Ok(None);
    }
    Ok(Some((packet.command, bytes.to_vec())))
}

pub(super) unsafe extern "system" fn receive(raw: *mut c_void, _: *mut c_void) {
    // No locks, blocking sends, user code, or unwinding across the SDK ABI.
    let slot = SLOT.load(Ordering::Acquire);
    if slot == 0 {
        return;
    }
    let copied = std::panic::catch_unwind(|| copy_packet(raw, slot));
    let message = match copied {
        Ok(Ok(Some((command, bytes)))) => Message::Packet(command, bytes),
        Ok(Ok(None)) => return,
        Ok(Err(error)) => Message::Error(error),
        Err(_) => Message::Error("vJoy FFB callback panicked.".into()),
    };
    if let Some(sender) = ROUTE.get() {
        if sender.try_send(message).is_err() {
            OVERFLOW.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_native_abi_copies_wire_payload_and_preserves_unknown_command() {
        let mut bytes = vec![0x35, 1, 0xff];
        let mut packet = FfbData {
            size: 11,
            command: 0xf000_ffff,
            data: bytes.as_ptr(),
        };
        let raw = (&mut packet as *mut FfbData).cast();
        let (command, copied) = unsafe { copy_packet(raw, 3) }.unwrap().unwrap();
        bytes.fill(0);
        assert_eq!(command, 0xf000_ffff);
        assert_eq!(copied, [0x35, 1, 0xff]);
        assert!(unsafe { copy_packet(raw, 2) }.unwrap().is_none());
    }
    #[test]
    fn malformed_native_packets_fail_before_dereferencing_data() {
        assert!(unsafe { copy_packet(std::ptr::null_mut(), 1) }.is_err());
        for size in [0, 7, 8, 257, u32::MAX] {
            let mut packet = FfbData {
                size,
                command: 0,
                data: std::ptr::dangling(),
            };
            assert!(unsafe { copy_packet((&mut packet as *mut FfbData).cast(), 1) }.is_err());
        }
        let mut packet = FfbData {
            size: 9,
            command: 0,
            data: std::ptr::null(),
        };
        assert!(unsafe { copy_packet((&mut packet as *mut FfbData).cast(), 1) }.is_err());
    }
    #[test]
    fn largest_qualified_packet_keeps_all_248_payload_bytes() {
        let mut bytes = [0xff; 248];
        bytes[0] = 0xf5;
        let mut packet = FfbData {
            size: 256,
            command: 0xb0017,
            data: bytes.as_ptr(),
        };
        assert_eq!(
            unsafe { copy_packet((&mut packet as *mut FfbData).cast(), 15) }
                .unwrap()
                .unwrap()
                .1,
            bytes
        );
    }
}

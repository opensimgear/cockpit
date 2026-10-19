use espflash::{
    connection::{Connection, ResetAfterOperation, ResetBeforeOperation},
    flasher::Flasher,
    image_format::Segment,
    target::{Chip, ProgressCallbacks},
};
use serde_json::{json, Value};
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Clone, Copy)]
pub enum FirmwareTarget { Esp32, Esp32s3 }
impl FirmwareTarget {
    fn boot_offset(self) -> u32 { match self { Self::Esp32=>0x1000, Self::Esp32s3=>0 } }
    fn flash_end(self) -> u32 { match self { Self::Esp32=>0x400000, Self::Esp32s3=>0x800000 } }
    fn chip(self) -> Chip { match self { Self::Esp32=>Chip::Esp32, Self::Esp32s3=>Chip::Esp32s3 } }
}
pub struct FirmwareSegment {
    pub offset: u32,
    pub bytes: &'static [u8],
}
struct Progress {
    state: Arc<Mutex<Value>>,
    total: usize,
}
impl ProgressCallbacks for Progress {
    fn init(&mut self, _: u32, total: usize) {
        self.total = total;
    }
    fn update(&mut self, current: usize) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state["progress"] = json!((current * 100 / self.total.max(1)).min(100));
    }
    fn verifying(&mut self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner())["stage"] = json!("Verifying firmware");
    }
    fn finish(&mut self, _: bool) {}
}
pub fn validate_firmware(firmware: &[FirmwareSegment]) -> Result<(), String> {
    validate_firmware_for(FirmwareTarget::Esp32s3, firmware)
}
pub fn validate_firmware_for(target: FirmwareTarget, firmware: &[FirmwareSegment]) -> Result<(), String> {
    if firmware.len() != 3
        || firmware
            .iter()
            .zip([target.boot_offset(), 0x8000, 0x10000])
            .any(|(s, offset)| s.offset != offset || s.bytes.is_empty())
    {
        return Err("Invalid board firmware layout.".into());
    }
    for (index, segment) in firmware.iter().enumerate() {
        let limit = if index + 1 < firmware.len() {
            firmware[index + 1].offset
        } else {
            target.flash_end()
        };
        if segment.bytes.len() > (limit - segment.offset) as usize {
            return Err("Board firmware exceeds its partition.".into());
        }
    }
    if firmware[0].bytes[0] != 0xe9 || firmware[2].bytes[0] != 0xe9 {
        return Err("Invalid ESP32 firmware image.".into());
    }
    Ok(())
}
fn flash_error(error: impl std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
fn esp32_security_configured(crypt: u32, boot: u32) -> bool {
    crypt & (0x7f << 20) != 0 || boot & (3 << 4) != 0
}
#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn original_esp32_security_efuses_fail_closed() {
        assert!(!esp32_security_configured(0, 0));
        for bit in 20..27 { assert!(esp32_security_configured(1 << bit, 0)); }
        for bit in 4..6 { assert!(esp32_security_configured(0, 1 << bit)); }
        assert!(!esp32_security_configured(1, 1));
    }
}
fn writes_segment(preserve:bool,offset:u32)->bool{!preserve || matches!(offset,0x8000|0x10000)}
pub fn flash_firmware(
    port: &str,
    firmware: &[FirmwareSegment],
    target: FirmwareTarget,
    manual_boot: bool,
    state: &Arc<Mutex<Value>>,
) -> Result<(), String> {
    validate_firmware_for(target, firmware)?;
    let info = serialport::available_ports()
        .map_err(flash_error)?
        .into_iter()
        .find(|p| p.port_name == port)
        .ok_or("Choose a connected USB device.")?;
    let serialport::SerialPortType::UsbPort(info) = info.port_type else {
        return Err("Choose a USB device.".into());
    };
    let serial = serialport::new(port, 115200)
        .timeout(Duration::from_secs(3))
        .open_native()
        .map_err(flash_error)?;
    let connection = Connection::new(
        serial,
        info,
        ResetAfterOperation::HardReset,
        if manual_boot { ResetBeforeOperation::NoReset } else { ResetBeforeOperation::DefaultReset },
        115200,
    );
    let mut flasher = Flasher::connect(
        connection,
        true,
        true,
        false,
        Some(target.chip()),
        Some(460800),
    )
    .map_err(|error|format!("bootloader-required:{}",flash_error(error)))?;
    // The original ESP32 ROM/stub has no GET_SECURITY_INFO command.
    // Read its security eFuses instead (same register layout as esptool).
    let secured = match target {
        FirmwareTarget::Esp32 => {
            let crypt = flasher.connection().read_reg(0x3ff5a000).map_err(flash_error)?;
            let boot = flasher.connection().read_reg(0x3ff5a018).map_err(flash_error)?;
            esp32_security_configured(crypt, boot)
        }
        FirmwareTarget::Esp32s3 => {
            let security = flasher.security_info().map_err(flash_error)?;
            security.flags & 5 != 0 || security.flash_crypt_cnt != 0
        }
    };
    if secured {
        return Err(
            "Automatic installation is unavailable on a secured or flash-encrypted board.".into(),
        );
    }
    let segments: Vec<_> = firmware
        .iter()
        .filter(|s| writes_segment(state.lock().unwrap_or_else(|p|p.into_inner())["preserveSetup"]==true,s.offset))
        .map(|s| Segment {
            addr: s.offset,
            data: Cow::Borrowed(s.bytes),
        })
        .collect();
    flasher
        .write_bins_to_flash(
            &segments,
            &mut Progress {
                state: state.clone(),
                total: 1,
            },
        )
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updates_write_partition_table_and_application_but_preserve_pairing_storage() {
        for offset in [0,0x1000,0x8000,0x9000,0xe000,0x10000]{
            assert_eq!(writes_segment(true,offset),matches!(offset,0x8000|0x10000));
            assert!(writes_segment(false,offset));
        }
    }
    #[test]
    fn rejects_invalid_or_overlapping_images_before_opening_a_port() {
        assert!(validate_firmware(&[]).is_err());
        let invalid = [
            FirmwareSegment {
                offset: 0,
                bytes: &[0xe9],
            },
            FirmwareSegment {
                offset: 0x8000,
                bytes: &[0],
            },
            FirmwareSegment {
                offset: 0x10000,
                bytes: &[0xe9],
            },
        ];
        assert!(validate_firmware(&invalid).is_ok());
        assert!(validate_firmware_for(FirmwareTarget::Esp32,&invalid).is_err());
        let esp32=[
            FirmwareSegment{offset:0x1000,bytes:&[0xe9]},
            FirmwareSegment{offset:0x8000,bytes:&[0]},
            FirmwareSegment{offset:0x10000,bytes:&[0xe9]},
        ];
        assert!(validate_firmware_for(FirmwareTarget::Esp32,&esp32).is_ok());
        assert!(validate_firmware_for(FirmwareTarget::Esp32s3,&esp32).is_err());
        let bad = [
            FirmwareSegment {
                offset: 0,
                bytes: &[0],
            },
            FirmwareSegment {
                offset: 0x8000,
                bytes: &[0],
            },
            FirmwareSegment {
                offset: 0x10000,
                bytes: &[0xe9],
            },
        ];
        assert!(validate_firmware(&bad).is_err());
    }
}

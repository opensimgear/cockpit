//! Native CDC transport; enumeration never opens a device.
use serde_json::{json, Value};
use std::{
    io::{self, Read, Write},
    time::Duration,
};

#[derive(Clone, Debug)]
pub(crate) struct PortInfo {
    pub path: String,
    pub serial_number: Option<String>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub manufacturer: Option<String>,
    pub friendly_name: Option<String>,
}

impl PortInfo {
    pub fn value(&self) -> Value {
        let mut value = json!({"path": self.path});
        for (key, field) in [
            ("serialNumber", &self.serial_number),
            ("vendorId", &self.vendor_id),
            ("productId", &self.product_id),
            ("manufacturer", &self.manufacturer),
            ("friendlyName", &self.friendly_name),
        ] {
            if let Some(field) = field {
                value[key] = json!(field.chars().take(256).collect::<String>());
            }
        }
        value
    }
}

pub(crate) fn allowed_path(path: &str) -> bool {
    let ascii_suffix = |suffix: &str| {
        !suffix.is_empty()
            && suffix
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'.' | b'-'))
    };
    if let Some(suffix) = path
        .strip_prefix("/dev/cu.usbmodem")
        .or_else(|| path.strip_prefix("/dev/tty.usbmodem"))
    {
        return ascii_suffix(suffix);
    }
    path.strip_prefix("/dev/ttyACM")
        .or_else(|| path.strip_prefix("COM"))
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|ch| ch.is_ascii_digit()))
}

pub(crate) trait Connection: Read + Write + Send {
    fn set_control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String>;
}

pub(crate) trait Factory: Send {
    fn ports(&mut self) -> Result<Vec<PortInfo>, String>;
    fn open(
        &mut self,
        port: &PortInfo,
        read_timeout: Duration,
    ) -> Result<Box<dyn Connection>, String>;
}

pub(crate) struct NativeFactory;
struct NativeConnection(Box<dyn serialport::SerialPort>);

impl Read for NativeConnection {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}
impl Write for NativeConnection {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
impl Connection for NativeConnection {
    fn set_control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String> {
        self.0
            .write_data_terminal_ready(dtr)
            .map_err(|error| error.to_string())?;
        self.0
            .write_request_to_send(rts)
            .map_err(|error| error.to_string())
    }
}
impl Factory for NativeFactory {
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        serialport::available_ports()
            .map_err(|error| format!("Could not enumerate USB serial ports: {error}"))
            .map(|ports| {
                ports
                    .into_iter()
                    .filter(|port| allowed_path(&port.port_name))
                    .map(|port| {
                        let mut info = PortInfo {
                            path: port.port_name,
                            serial_number: None,
                            vendor_id: None,
                            product_id: None,
                            manufacturer: None,
                            friendly_name: None,
                        };
                        if let serialport::SerialPortType::UsbPort(usb) = port.port_type {
                            info.serial_number = usb.serial_number;
                            info.vendor_id = Some(format!("{:04x}", usb.vid));
                            info.product_id = Some(format!("{:04x}", usb.pid));
                            info.manufacturer = usb.manufacturer;
                            info.friendly_name = usb.product;
                        }
                        info
                    })
                    .collect()
            })
    }
    fn open(
        &mut self,
        port: &PortInfo,
        read_timeout: Duration,
    ) -> Result<Box<dyn Connection>, String> {
        serialport::new(&port.path, 115_200)
            .timeout(read_timeout)
            .open()
            .map(|connection| Box::new(NativeConnection(connection)) as Box<dyn Connection>)
            .map_err(|error| format!("Could not open the Pico serial port: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permits_only_cdc_device_paths() {
        for path in [
            "/dev/cu.usbmodem201401",
            "/dev/tty.usbmodemOSG-1",
            "/dev/ttyACM0",
            "COM15",
        ] {
            assert!(allowed_path(path));
        }
        for path in [
            "/tmp/file",
            "/dev/cu.usbserial-UART",
            "/dev/cu.usbmodem../../file",
            "COM",
            "COM1/file",
        ] {
            assert!(!allowed_path(path));
        }
    }
}

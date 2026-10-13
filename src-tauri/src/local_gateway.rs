//! Local gateway endpoints use the same protocol handshake as USB CDC.
//! Advertisements select candidates; they never replace firmware identity checks.
use crate::transport::{Connection, PortInfo};
use std::time::Duration;

#[cfg(unix)]
mod unix {
    use super::*;
    use serde_json::Value;
    use std::{
        fs,
        io::{self, Read, Write},
        net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
        os::unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
        path::{Path, PathBuf},
    };

    unsafe extern "C" {
        fn geteuid() -> u32;
        fn kill(pid: i32, signal: i32) -> i32;
    }

    const ADVERTISEMENT_LIMIT: u64 = 4096;

    fn uid() -> u32 {
        // POSIX exposes this process's effective user without allocating state.
        unsafe { geteuid() }
    }

    fn directory() -> PathBuf {
        PathBuf::from(format!("/tmp/opensimgear-{}/gateways", uid()))
    }

    fn identity(value: &str) -> bool {
        value.len() == 24
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && value.bytes().any(|byte| byte != b'0')
            && value.bytes().any(|byte| byte != b'f')
    }

    enum Endpoint<'a> {
        Unix(&'a str),
        Tcp { serial: &'a str, port: u16 },
    }

    fn endpoint<'a>(directory: &Path, value: &'a str) -> Option<Endpoint<'a>> {
        if let Some(value) = value.strip_prefix("tcp:127.0.0.1:") {
            let (number, serial) = value.split_once('/')?;
            let port = number.parse::<u16>().ok()?;
            return (port > 0 && number == port.to_string() && identity(serial))
                .then_some(Endpoint::Tcp { serial, port });
        }
        let path = value.strip_prefix("unix:")?;
        let prefix = format!("{}/", directory.display());
        let file = path.strip_prefix(&prefix)?;
        let serial = file.strip_suffix(".sock")?;
        identity(serial).then_some(Endpoint::Unix(serial))
    }

    fn private_directory(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            metadata.file_type().is_dir()
                && metadata.uid() == uid()
                && metadata.mode() & 0o777 == 0o700
        })
    }

    fn live_process(pid: u64) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // Signal zero checks liveness without signalling or altering the process.
        pid > 0 && unsafe { kill(pid, 0) } == 0
    }

    fn advertisement(directory: &Path, serial: &str) -> Option<PortInfo> {
        let path = directory.join(format!("{serial}.json"));
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.file_type().is_file()
            || metadata.uid() != uid()
            || metadata.mode() & 0o022 != 0
            || metadata.len() > ADVERTISEMENT_LIMIT
        {
            return None;
        }
        // Reading is bounded even if an atomic replacement races enumeration.
        let mut bytes = Vec::new();
        let file = fs::File::open(path).ok()?;
        let opened = file.metadata().ok()?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return None;
        }
        file.take(ADVERTISEMENT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > ADVERTISEMENT_LIMIT {
            return None;
        }
        let value: Value = serde_json::from_slice(&bytes).ok()?;
        if !matches!(value["protocol"].as_u64(), Some(2 | 3))
            || value["role"] != "gateway"
            || value["serialNumber"] != serial
            || value["manufacturer"] != "OpenSimGear"
            || value["endpoint"] != format!("{serial}.sock")
            || !value["pid"].as_u64().is_some_and(live_process)
        {
            return None;
        }
        let name = value["friendlyName"].as_str()?;
        if name.is_empty()
            || name.len() > 256
            || name.chars().any(char::is_control)
            || !name.to_ascii_lowercase().contains("gateway")
        {
            return None;
        }
        let path = if let Some(number) = value.get("tcpPort") {
            let port = u16::try_from(number.as_u64()?)
                .ok()
                .filter(|port| *port > 0)?;
            format!("tcp:127.0.0.1:{port}/{serial}")
        } else {
            let socket = directory.join(format!("{serial}.sock"));
            let metadata = fs::symlink_metadata(&socket).ok()?;
            if !metadata.file_type().is_socket() || metadata.uid() != uid() {
                return None;
            }
            format!("unix:{}", socket.display())
        };
        Some(PortInfo {
            path,
            serial_number: Some(serial.into()),
            manufacturer: Some("OpenSimGear".into()),
            friendly_name: Some(name.into()),
            vendor_id: None,
            product_id: None,
        })
    }

    fn discover_in(directory: &Path) -> Vec<PortInfo> {
        if !directory.parent().is_some_and(private_directory) || !private_directory(directory) {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return Vec::new();
        };
        entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file = entry.file_name();
                let serial = file.to_str()?.strip_suffix(".json")?;
                identity(serial)
                    .then(|| advertisement(directory, serial))
                    .flatten()
            })
            .collect()
    }

    enum LocalConnection {
        Unix(UnixStream),
        Tcp(TcpStream),
    }

    impl Read for LocalConnection {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Unix(stream) => stream.read(buffer),
                Self::Tcp(stream) => stream.read(buffer),
            }
        }
    }
    impl Write for LocalConnection {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            match self {
                Self::Unix(stream) => stream.write(buffer),
                Self::Tcp(stream) => stream.write(buffer),
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            match self {
                Self::Unix(stream) => stream.flush(),
                Self::Tcp(stream) => stream.flush(),
            }
        }
    }
    impl Connection for LocalConnection {
        fn set_control_lines(&mut self, _: bool, _: bool) -> Result<(), String> {
            Ok(())
        }
    }

    fn open_in(
        directory: &Path,
        port: &PortInfo,
        timeout: Duration,
    ) -> Result<Box<dyn Connection>, String> {
        let endpoint = endpoint(directory, &port.path).ok_or("Invalid local gateway endpoint.")?;
        if !discover_in(directory).iter().any(|candidate| {
            candidate.path == port.path && candidate.serial_number == port.serial_number
        }) {
            return Err("The local gateway is no longer advertised.".into());
        }
        let connection = match endpoint {
            Endpoint::Unix(serial) => {
                let stream = UnixStream::connect(directory.join(format!("{serial}.sock")))
                    .map_err(|error| format!("Could not connect to the local gateway: {error}"))?;
                stream
                    .set_read_timeout(Some(timeout))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(timeout))
                    .map_err(|error| error.to_string())?;
                LocalConnection::Unix(stream)
            }
            Endpoint::Tcp {
                port: number,
                serial,
            } => {
                if port.serial_number.as_deref() != Some(serial) {
                    return Err("Invalid local gateway identity.".into());
                }
                let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, number));
                let stream = TcpStream::connect_timeout(&address, timeout)
                    .map_err(|error| format!("Could not connect to the local gateway: {error}"))?;
                stream
                    .set_read_timeout(Some(timeout))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(timeout))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_nodelay(true)
                    .map_err(|error| error.to_string())?;
                LocalConnection::Tcp(stream)
            }
        };
        Ok(Box::new(connection))
    }

    pub(super) fn allowed_path(path: &str) -> bool {
        endpoint(&directory(), path).is_some()
    }
    pub(super) fn ports() -> Vec<PortInfo> {
        discover_in(&directory())
    }
    pub(super) fn open(port: &PortInfo, timeout: Duration) -> Result<Box<dyn Connection>, String> {
        open_in(&directory(), port, timeout)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde_json::json;
        use std::net::TcpListener;
        use std::os::unix::{
            fs::{symlink, PermissionsExt},
            net::UnixListener,
        };

        const SERIAL: &str = "00112233445566778899aabb";
        struct Fixture {
            root: PathBuf,
            directory: PathBuf,
        }
        impl Fixture {
            fn new() -> Self {
                // Keep Unix socket paths below the platform sockaddr_un limit.
                let workspace = PathBuf::from("/tmp");
                let root = workspace.join(format!(
                    "uds-{}",
                    &uuid::Uuid::new_v4().simple().to_string()[..8]
                ));
                let directory = root.join("gateways");
                fs::create_dir_all(&directory).unwrap();
                for path in [&root, &directory] {
                    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
                }
                Self { root, directory }
            }
            fn socket(&self) -> UnixListener {
                UnixListener::bind(self.directory.join(format!("{SERIAL}.sock"))).unwrap()
            }
            fn advertise(&self, patch: Value) {
                let mut value = json!({"protocol":2,"role":"gateway","serialNumber":SERIAL,
                    "manufacturer":"OpenSimGear","friendlyName":"OpenSimGear CAN gateway",
                    "endpoint":format!("{SERIAL}.sock"),"pid":std::process::id()});
                for (key, item) in patch.as_object().unwrap() {
                    value[key] = item.clone();
                }
                fs::write(
                    self.directory.join(format!("{SERIAL}.json")),
                    value.to_string(),
                )
                .unwrap();
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        #[test]
        fn local_endpoint_paths_are_confined_to_current_user_gateway_directory() {
            let root = directory();
            assert!(allowed_path(&format!(
                "unix:{}/{SERIAL}.sock",
                root.display()
            )));
            assert!(allowed_path(&format!("tcp:127.0.0.1:32123/{SERIAL}")));
            for path in [
                format!("unix:/tmp/{SERIAL}.sock"),
                format!("unix:{root:?}/../{SERIAL}.sock"),
                format!("unix:{}/{}.sock", root.display(), "0".repeat(24)),
                format!("unix:{}/{}.sock", root.display(), "f".repeat(24)),
                format!("unix:{}/{SERIAL}.can.sock", root.display()),
                format!("tcp:127.0.0.1:0/{SERIAL}"),
                format!("tcp:127.0.0.1:65536/{SERIAL}"),
                format!("tcp:127.0.0.1:032123/{SERIAL}"),
                format!("tcp:localhost:32123/{SERIAL}"),
                format!("tcp:127.0.0.2:32123/{SERIAL}"),
                format!("tcp:192.0.2.1:32123/{SERIAL}"),
                "tcp:127.0.0.1:32123".into(),
                format!("tcp:127.0.0.1:32123/{SERIAL}/extra"),
            ] {
                assert!(!allowed_path(&path));
            }
        }

        #[test]
        fn discovery_requires_live_gateway_metadata_and_private_socket_directory() {
            let fixture = Fixture::new();
            let _listener = fixture.socket();
            assert!(discover_in(&fixture.directory).is_empty());
            fixture.advertise(json!({}));
            assert_eq!(discover_in(&fixture.directory).len(), 1);
            fixture.advertise(json!({"protocol":3}));
            assert_eq!(discover_in(&fixture.directory).len(), 1);
            for patch in [
                json!({"pid":2147483647}),
                json!({"role":"module"}),
                json!({"protocol":4}),
                json!({"serialNumber":"ffeeddccbbaa998877665544"}),
                json!({"endpoint":"../outside.sock"}),
                json!({"manufacturer":"Other"}),
            ] {
                fixture.advertise(patch);
                assert!(discover_in(&fixture.directory).is_empty());
            }
            fixture.advertise(json!({}));
            fs::set_permissions(&fixture.directory, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(discover_in(&fixture.directory).is_empty());
        }

        #[test]
        fn discovery_ignores_symlinked_or_oversized_advertisements() {
            let fixture = Fixture::new();
            let _listener = fixture.socket();
            fixture.advertise(json!({}));
            let ad = fixture.directory.join(format!("{SERIAL}.json"));
            let target = fixture.root.join("advertisement.json");
            fs::rename(&ad, &target).unwrap();
            symlink(&target, &ad).unwrap();
            assert!(discover_in(&fixture.directory).is_empty());
            fs::remove_file(&ad).unwrap();
            fs::write(ad, vec![b' '; 4097]).unwrap();
            assert!(discover_in(&fixture.directory).is_empty());
        }

        #[test]
        fn advertised_socket_uses_byte_stream_and_read_timeout_without_spawning_gateway() {
            let fixture = Fixture::new();
            let listener = fixture.socket();
            fixture.advertise(json!({}));
            let port = discover_in(&fixture.directory).pop().unwrap();
            let mut client = open_in(&fixture.directory, &port, Duration::from_millis(20)).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            client.set_control_lines(true, false).unwrap();
            client.write_all(b"cockpit 1 snapshot\n").unwrap();
            let mut bytes = [0u8; 19];
            server.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"cockpit 1 snapshot\n");
            server.write_all(b"{\"id\":1}\n").unwrap();
            let mut reply = [0u8; 9];
            client.read_exact(&mut reply).unwrap();
            assert_eq!(&reply, b"{\"id\":1}\n");
            let error = client.read(&mut [0u8; 1]).unwrap_err();
            assert!(matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ));
            drop(server);
            assert_eq!(client.read(&mut [0u8; 1]).unwrap(), 0);
            fixture.advertise(json!({"pid":2147483647}));
            assert!(open_in(&fixture.directory, &port, Duration::from_millis(20)).is_err());
        }

        #[test]
        fn tcp_discovery_requires_private_live_metadata_and_a_valid_port() {
            let fixture = Fixture::new();
            fixture.advertise(json!({"tcpPort":32123}));
            let port = discover_in(&fixture.directory).pop().unwrap();
            assert_eq!(port.path, format!("tcp:127.0.0.1:32123/{SERIAL}"));
            for patch in [
                json!({"tcpPort":0}),
                json!({"tcpPort":65536}),
                json!({"tcpPort":-1}),
                json!({"tcpPort":"32123"}),
                json!({"tcpPort":null}),
                json!({"tcpPort":32123,"pid":2147483647}),
                json!({"tcpPort":32123,"endpoint":"tcp:192.0.2.1:32123"}),
            ] {
                fixture.advertise(patch);
                assert!(discover_in(&fixture.directory).is_empty());
            }
            fixture.advertise(json!({"tcpPort":32123}));
            fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(discover_in(&fixture.directory).is_empty());
        }

        #[test]
        fn advertised_tcp_stream_has_timeout_and_rediscovery_after_port_changes() {
            let fixture = Fixture::new();
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            fixture.advertise(json!({"tcpPort":listener.local_addr().unwrap().port()}));
            let port = discover_in(&fixture.directory).pop().unwrap();
            let mut client = open_in(&fixture.directory, &port, Duration::from_millis(20)).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            client.set_control_lines(true, false).unwrap();
            client.write_all(b"cockpit 1 snapshot\n").unwrap();
            let mut bytes = [0u8; 19];
            server.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"cockpit 1 snapshot\n");
            server.write_all(b"{\"id\":1}\n").unwrap();
            let mut reply = [0u8; 9];
            client.read_exact(&mut reply).unwrap();
            assert_eq!(&reply, b"{\"id\":1}\n");
            let error = client.read(&mut [0u8; 1]).unwrap_err();
            assert!(matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ));
            drop(server);
            assert_eq!(client.read(&mut [0u8; 1]).unwrap(), 0);

            let replacement = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            fixture.advertise(json!({"tcpPort":replacement.local_addr().unwrap().port()}));
            assert!(open_in(&fixture.directory, &port, Duration::from_millis(20)).is_err());
            let fresh = discover_in(&fixture.directory).pop().unwrap();
            assert_ne!(fresh.path, port.path);
            assert_eq!(fresh.serial_number, port.serial_number);
            let _client = open_in(&fixture.directory, &fresh, Duration::from_millis(20)).unwrap();
            let _server = replacement.accept().unwrap();
        }
    }
}

pub(crate) fn allowed_path(path: &str) -> bool {
    #[cfg(unix)]
    {
        unix::allowed_path(path)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}
pub(crate) fn ports() -> Vec<PortInfo> {
    #[cfg(unix)]
    {
        unix::ports()
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}
pub(crate) fn open(port: &PortInfo, timeout: Duration) -> Result<Box<dyn Connection>, String> {
    #[cfg(unix)]
    {
        unix::open(port, timeout)
    }
    #[cfg(not(unix))]
    {
        let _ = (port, timeout);
        Err("Local gateway sockets are unavailable on this platform.".into())
    }
}

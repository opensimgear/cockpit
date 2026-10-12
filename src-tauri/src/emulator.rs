//! Development-only transport to the portable production C gateway emulator.
use crate::transport::{Connection, Factory, PortInfo};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

const UID: &str = "00112233445566778899aabb";
struct Process {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<io::Result<Vec<u8>>>,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub(crate) struct EmulatorFactory {
    executable: PathBuf,
    process: Option<Arc<Mutex<Process>>>,
}
impl EmulatorFactory {
    pub(crate) fn from_environment() -> Option<Self> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let executable = PathBuf::from(std::env::var_os("OSG_NATIVE_EMULATOR")?);
        executable.is_absolute().then_some(Self {
            executable,
            process: None,
        })
    }
    fn process(&mut self) -> Result<Arc<Mutex<Process>>, String> {
        if let Some(process) = &self.process {
            return Ok(process.clone());
        }
        let mut child = Command::new(&self.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Could not start the native firmware emulator: {e}"))?;
        let input = child.stdin.take().ok_or("Emulator stdin is unavailable.")?;
        let mut output = child
            .stdout
            .take()
            .ok_or("Emulator stdout is unavailable.")?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || loop {
            let mut buffer = [0u8; 4096];
            match output.read(&mut buffer) {
                Ok(0) => {
                    let _ = sender.send(Err(io::ErrorKind::UnexpectedEof.into()));
                    break;
                }
                Ok(count) => {
                    if sender.send(Ok(buffer[..count].to_vec())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = sender.send(Err(e));
                    break;
                }
            }
        });
        let process = Arc::new(Mutex::new(Process {
            child,
            input,
            output: receiver,
        }));
        self.process = Some(process.clone());
        Ok(process)
    }
}
struct EmulatorConnection {
    process: Arc<Mutex<Process>>,
    buffer: VecDeque<u8>,
    timeout: Duration,
}
impl Read for EmulatorConnection {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.buffer.is_empty() {
            let process = self
                .process
                .lock()
                .map_err(|_| io::Error::other("Emulator lock failed"))?;
            self.buffer
                .extend(
                    process
                        .output
                        .recv_timeout(self.timeout)
                        .map_err(|e| match e {
                            mpsc::RecvTimeoutError::Timeout => {
                                io::Error::from(io::ErrorKind::TimedOut)
                            }
                            mpsc::RecvTimeoutError::Disconnected => {
                                io::Error::from(io::ErrorKind::UnexpectedEof)
                            }
                        })??,
                );
        }
        let count = output.len().min(self.buffer.len());
        for byte in &mut output[..count] {
            *byte = self.buffer.pop_front().unwrap();
        }
        Ok(count)
    }
}
impl Write for EmulatorConnection {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.process
            .lock()
            .map_err(|_| io::Error::other("Emulator lock failed"))?
            .input
            .write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.process
            .lock()
            .map_err(|_| io::Error::other("Emulator lock failed"))?
            .input
            .flush()
    }
}
impl Connection for EmulatorConnection {
    fn set_control_lines(&mut self, _: bool, _: bool) -> Result<(), String> {
        Ok(())
    }
}
impl Factory for EmulatorFactory {
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        Ok(vec![PortInfo {
            path: "/dev/cu.usbmodemOSGNATIVE".into(),
            serial_number: Some(UID.into()),
            manufacturer: Some("OpenSimGear".into()),
            friendly_name: Some("CAN Gateway Emulator".into()),
            vendor_id: None,
            product_id: None,
        }])
    }
    fn open(&mut self, _: &PortInfo, timeout: Duration) -> Result<Box<dyn Connection>, String> {
        Ok(Box::new(EmulatorConnection {
            process: self.process()?,
            buffer: VecDeque::new(),
            timeout,
        }))
    }
}

//! Opt-in qualification evidence for the two authored vJoy buttons.
use std::{
    env,
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const HEADER: &[u8; 8] = b"OSGVJAC1";
const MAX_ROWS: usize = 2_100_000;

pub(super) struct Trace {
    file: BufWriter<File>,
    frequency: u64,
    masks: [u8; 16],
    rows: usize,
    failed: bool,
}

impl Trace {
    pub(super) fn open() -> Result<Option<Self>, String> {
        if env::var_os("OSG_REACTIVE_QUALIFICATION").as_deref() != Some(OsStr::new("1")) {
            return Ok(None);
        }
        let Some(directory) = env::var_os("OSG_VJOY_ACCEPTANCE_TRACE_DIR") else {
            return Ok(None);
        };
        let directory = PathBuf::from(directory);
        if !directory.is_absolute() {
            return Err("vJoy acceptance trace directory must be absolute.".into());
        }
        let path = directory.join(format!("vjoy-acceptance-{}.bin", std::process::id()));
        Self::create(&path)
            .map(Some)
            .map_err(|error| format!("Could not open vJoy acceptance trace: {error}"))
    }

    pub(super) fn create(path: &Path) -> io::Result<Self> {
        let mut frequency = 0;
        if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
            return Err(io::Error::other(
                "Windows performance counter frequency is unavailable",
            ));
        }
        let mut file = BufWriter::with_capacity(
            64 * 1024,
            OpenOptions::new().write(true).create_new(true).open(path)?,
        );
        file.write_all(HEADER)?;
        Ok(Self {
            file,
            frequency: frequency as u64,
            masks: [0; 16],
            rows: 0,
            failed: false,
        })
    }

    pub(super) fn submit(&mut self, slot: u8, mask: u8, update: impl FnOnce() -> i32) -> i32 {
        let index = usize::from(slot - 1);
        if self.failed || self.rows == MAX_ROWS || mask == self.masks[index] {
            return update();
        }
        let before = self.now();
        let accepted = update();
        if accepted == 1 {
            let after = self.now();
            self.masks[index] = mask;
            self.failed = match (before, after) {
                (Some(before), Some(after)) => self.write(slot, mask, before, after).is_err(),
                _ => true,
            };
        }
        accepted
    }

    fn now(&self) -> Option<u64> {
        let mut ticks = 0;
        if unsafe { QueryPerformanceCounter(&mut ticks) } == 0 || ticks < 0 {
            return None;
        }
        u64::try_from(ticks as u128 * 1_000_000_000 / u128::from(self.frequency)).ok()
    }

    fn write(&mut self, slot: u8, mask: u8, before: u64, after: u64) -> io::Result<()> {
        let mut row = [0; 18];
        row[..2].copy_from_slice(&[slot, mask]);
        row[2..10].copy_from_slice(&before.to_le_bytes());
        row[10..].copy_from_slice(&after.to_le_bytes());
        self.file.write_all(&row)?;
        self.rows += 1;
        Ok(())
    }

    pub(super) fn flush(&mut self) {
        self.failed |= self.file.flush().is_err();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_only_changed_successful_calls_with_a_counter_bracket() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("acceptance.bin");
        let mut trace = Trace::create(&path).unwrap();
        assert!(Trace::create(&path).is_err());
        assert_eq!(trace.submit(2, 0, || 1), 1);
        assert_eq!(trace.submit(2, 1, || 0), 0);
        let frequency = trace.frequency;
        let mut inside = 0;
        assert_eq!(
            trace.submit(2, 1, || {
                assert_ne!(unsafe { QueryPerformanceCounter(&mut inside) }, 0);
                1
            }),
            1
        );
        assert_eq!(trace.submit(2, 1, || 1), 1);
        assert_eq!(trace.submit(2, 0, || 1), 1);
        trace.flush();

        let data = std::fs::read(path).unwrap();
        assert_eq!(&data[..8], HEADER);
        let rows: Vec<_> = data[8..].chunks(18).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(&rows[0][..2], &[2, 1]);
        assert_eq!(&rows[1][..2], &[2, 0]);
        let before = u64::from_le_bytes(rows[0][2..10].try_into().unwrap());
        let after = u64::from_le_bytes(rows[0][10..18].try_into().unwrap());
        let inside = (inside as u128 * 1_000_000_000 / u128::from(frequency)) as u64;
        assert!(before <= inside && inside <= after);
    }
}

use super::*;
use std::{ffi::c_void, ptr};
type Handle = *mut c_void;
const READ: u32 = 4;
const QUERY_LIMITED: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x00100000;
const WAIT_TIMEOUT: u32 = 258;
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn GetProcessTimes(
        process: Handle,
        created: *mut FileTime,
        exited: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        buffer: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn MapViewOfFile(
        mapping: Handle,
        access: u32,
        offset_high: u32,
        offset_low: u32,
        bytes: usize,
    ) -> *mut c_void;
    fn UnmapViewOfFile(base: *const c_void) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}
struct Process {
    handle: Handle,
    epoch: u64,
    image: String,
}
impl Process {
    fn open(pid: u32) -> Result<Self, String> {
        let handle = unsafe { OpenProcess(QUERY_LIMITED | SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return Err(format!(
                "Cannot open selected ACR process {pid}: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut created = FileTime::default();
        let mut exited = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            let error = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(format!(
                "Cannot identify selected ACR process creation: {error}"
            ));
        }
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut length) } == 0 {
            let error = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(format!(
                "Cannot identify selected ACR process image: {error}"
            ));
        }
        image.truncate(length as usize);
        let process = Self {
            handle,
            epoch: ((created.high as u64) << 32) | created.low as u64,
            image: String::from_utf16_lossy(&image),
        };
        if process.epoch == 0 || !process.running() {
            return Err("Selected ACR process is no longer running.".into());
        }
        Ok(process)
    }
    fn running(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle, 0) == WAIT_TIMEOUT }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
struct Mapping {
    handle: Handle,
    view: *mut u8,
    length: usize,
}
impl Mapping {
    fn open(name: &str, length: usize) -> Result<Self, String> {
        let wide: Vec<_> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { OpenFileMappingW(READ, 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "ACR telemetry page {name} is not available: {}",
                std::io::Error::last_os_error()
            ));
        }
        let view = unsafe { MapViewOfFile(handle, READ, 0, 0, length) } as *mut u8;
        if view.is_null() {
            let error = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(format!("Cannot map ACR page {name}: {error}"));
        }
        Ok(Self {
            handle,
            view,
            length,
        })
    }
    fn copy(&self) -> Vec<u8> {
        // The external producer can change bytes between reads. Volatile
        // byte loads avoid creating a Rust reference to concurrently mutable
        // shared storage; consistent_copy checks bounded repeated snapshots.
        (0..self.length)
            .map(|offset| unsafe { ptr::read_volatile(self.view.add(offset)) })
            .collect()
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view as *const c_void);
            CloseHandle(self.handle);
        }
    }
}
struct Pages {
    physics: Mapping,
    graphics: Mapping,
    statics: Mapping,
}
impl Pages {
    fn open(game: Game) -> Result<Self, String> {
        Self::open_named(game, game.mapping_prefix())
    }
    fn open_named(game: Game, prefix: &str) -> Result<Self, String> {
        let (physics_size, graphics_size, static_size) = game.full_page_lengths();
        let (physics_prefix, graphics_prefix, static_prefix) = game.page_lengths();
        let physics_name = format!("{prefix}_physics");
        Ok(Self {
            // Acquire documented complete pages where present. A producer
            // with only a known prefix keeps legacy channels; absent tail
            // fields remain Unknown instead of being padded with zeroes.
            physics: Mapping::open(&physics_name, physics_size)
                .or_else(|_| Mapping::open(&physics_name, physics_prefix))
                .or_else(|error| {
                    if game == Game::Rally {
                        Mapping::open(&physics_name, PHYSICS_SIZE)
                    } else {
                        Err(error)
                    }
                })?,
            graphics: Mapping::open(&format!("{prefix}_graphics"), graphics_size)
                .or_else(|_| Mapping::open(&format!("{prefix}_graphics"), graphics_prefix))?,
            statics: Mapping::open(&format!("{prefix}_static"), static_size)
                .or_else(|_| Mapping::open(&format!("{prefix}_static"), static_prefix))?,
        })
    }
    fn copy(&self, page: Page) -> Vec<u8> {
        match page {
            Page::Physics => self.physics.copy(),
            Page::Graphics => self.graphics.copy(),
            Page::Statics => self.statics.copy(),
        }
    }
}
pub(crate) struct WindowsSource {
    game: Game,
    pid: u32,
    expected_image: String,
    process: Option<Process>,
    pages: Option<Pages>,
    last_packet: Option<u32>,
    producer_checked_at: Option<std::time::Instant>,
}
impl WindowsSource {
    pub fn new(process_id: u32) -> Result<Self, String> {
        Self::new_for_image(process_id, "acr.exe")
    }
    pub fn new_for_image(process_id: u32, expected_image: &str) -> Result<Self, String> {
        Self::new_for_game(process_id, expected_image, Game::Rally)
    }
    pub fn new_for_game(process_id: u32, expected_image: &str, game: Game) -> Result<Self, String> {
        if process_id == 0 {
            return Err("Select a nonzero ACR process ID.".into());
        }
        crate::acr_setup::ensure_single_producer_for_game(process_id, game)?;
        let process = Process::open(process_id)?;
        selected_game_image_matches(&process.image, expected_image, game)?;
        Ok(Self {
            game,
            pid: process_id,
            expected_image: expected_image.into(),
            process: Some(process),
            pages: None,
            last_packet: None,
            producer_checked_at: Some(std::time::Instant::now()),
        })
    }
    pub fn process_image(&self) -> Option<&str> {
        self.process.as_ref().map(|p| p.image.as_str())
    }
    fn read(&mut self) -> Result<RawSample, String> {
        if self.process.is_none() {
            let process = Process::open(self.pid)?;
            selected_game_image_matches(&process.image, &self.expected_image, self.game)?;
            self.process = Some(process);
        }
        let process = self.process.as_ref().unwrap();
        if !process.running() {
            return Err("Selected ACR process exited; telemetry was detached.".into());
        }
        if self.pages.is_none() {
            self.pages = Some(Pages::open(self.game)?);
        }
        let identity = SourceIdentity {
            id: self.game.source_id(self.pid),
            epoch: process.epoch,
        };
        let pages = self.pages.as_ref().unwrap();
        let sample =
            consistent_copy_for_game(identity, self.game, &mut |page| Ok(pages.copy(page)))?;
        if !process.running() {
            return Err("Selected ACR process exited during telemetry capture.".into());
        }
        Ok(sample)
    }
}
// Ownership moves to one provider worker. The process/mappings are never
// concurrently accessed; the Source trait intentionally has no Sync bound.
unsafe impl Send for WindowsSource {}
impl Source for WindowsSource {
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        if self
            .producer_checked_at
            .is_none_or(|last| last.elapsed() >= std::time::Duration::from_millis(250))
        {
            if let Err(error) =
                crate::acr_setup::ensure_single_producer_for_game(self.pid, self.game)
            {
                self.close();
                return Err(error);
            }
            self.producer_checked_at = Some(std::time::Instant::now());
        }
        match self.read() {
            Ok(sample) => {
                let packet = u32::from_le_bytes(sample.physics[..4].try_into().unwrap());
                if counter_reset(self.last_packet, packet) {
                    // A stage/game reset can replace named mapping objects.
                    // Release our views before acquiring the next baseline.
                    self.close();
                }
                self.last_packet = Some(packet);
                Ok(Some(sample))
            }
            Err(error) => {
                self.close();
                Err(error)
            }
        }
    }
    fn close(&mut self) {
        self.pages = None;
        self.process = None;
        self.last_packet = None;
        self.producer_checked_at = None;
    }
}
impl Drop for WindowsSource {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod tests;

//! Read existing Assetto named pages without creating or writing game mappings.
//! Shared names are not a game identity; the caller explicitly selects a PID.
use crate::game_telemetry::{
    parse, parse_for_game, Game, RawSample, Source, SourceIdentity, GRAPHICS_SIZE,
    PHYSICS_CURRENT_MAX_RPM_OFFSET, PHYSICS_EXTENDED_SIZE, PHYSICS_SIZE, STATIC_SIZE,
    EVO_CAR_MODEL_OFFSET, EVO_GRAPHICS_SIZE,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Page {
    Physics,
    Graphics,
    Statics,
}
fn counter_reset(previous: Option<u32>, current: u32) -> bool {
    previous.is_some_and(|previous| current.wrapping_sub(previous) >= 0x80000000)
}
fn selected_image_matches(image: &str, expected: &str) -> Result<(), String> {
    if expected.is_empty()
        || expected.len() > 128
        || !expected.to_ascii_lowercase().ends_with(".exe")
        || !expected
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("Configure an explicit game executable basename such as acs.exe.".into());
    }
    let basename = image.rsplit(['\\', '/']).next().unwrap_or(image);
    if !basename.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "Selected process image {basename} does not match configured game image {expected}."
        ));
    }
    Ok(())
}
fn selected_game_image_matches(image: &str, expected: &str, game: Game) -> Result<(), String> {
    selected_image_matches(image, expected)?;
    let expected = expected.to_ascii_lowercase();
    let known = match game {
        Game::Corsa => matches!(expected.as_str(), "acs.exe" | "acs_x86.exe"),
        Game::Competizione => expected == "ac2-win64-shipping.exe",
        Game::Evo => expected == "assettocorsaevo.exe",
        Game::Rally => matches!(expected.as_str(), "acr.exe" | "acr-win64-shipping.exe"),
    };
    if !known { return Err(format!("The selected executable is not a {} telemetry producer.", game.id())); }
    Ok(())
}

/// No public ACR seqlock/transaction guarantee exists. Two equal bounded copies
/// are a best-effort stability check, not a claim of multi-page atomicity.
fn consistent_copy(
    source: SourceIdentity,
    read: &mut dyn FnMut(Page) -> Result<Vec<u8>, String>,
) -> Result<RawSample, String> {
    consistent_copy_for_game(source, Game::Rally, read)
}
fn consistent_copy_for_game(
    source: SourceIdentity,
    game: Game,
    read: &mut dyn FnMut(Page) -> Result<Vec<u8>, String>,
) -> Result<RawSample, String> {
    let mut copy = || -> Result<RawSample, String> {
        Ok(RawSample {
            source: source.clone(),
            physics: read(Page::Physics)?,
            graphics: read(Page::Graphics)?,
            statics: read(Page::Statics)?,
        })
    };
    for _ in 0..3 {
        let first = copy()?;
        let second = copy()?;
        parse_for_game(&first, game)?;
        parse_for_game(&second, game)?;
        if first.physics[..PHYSICS_SIZE] == second.physics[..PHYSICS_SIZE]
            && (!matches!(game, Game::Rally | Game::Evo)
                || first.physics.get(PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE)
                    == second.physics.get(PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE))
            && first.graphics[..GRAPHICS_SIZE] == second.graphics[..GRAPHICS_SIZE]
            && (game != Game::Evo || first.graphics[EVO_CAR_MODEL_OFFSET..EVO_GRAPHICS_SIZE]
                == second.graphics[EVO_CAR_MODEL_OFFSET..EVO_GRAPHICS_SIZE])
            && first.statics[..game.page_lengths().2] == second.statics[..game.page_lengths().2]
        {
            return Ok(second);
        }
    }
    Err("Assetto telemetry changed during all bounded snapshot attempts.".into())
}

#[cfg(windows)]
mod platform {
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
            if unsafe { QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut length) }
                == 0
            {
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
            let (physics_size, graphics_size, static_size) = game.page_lengths();
            let prefix = game.mapping_prefix();
            let physics_name = format!("{prefix}_physics");
            Ok(Self {
                // Older producers can expose only the original prefix. ACR's
                // current RPM limit lives later in physics; acquire it when the
                // existing mapping supports the extended read.
                physics: Mapping::open(&physics_name, physics_size)
                    .or_else(|error| if game == Game::Rally { Mapping::open(&physics_name, PHYSICS_SIZE) } else { Err(error) })?,
                graphics: Mapping::open(&format!("{prefix}_graphics"), graphics_size)?,
                statics: Mapping::open(&format!("{prefix}_static"), static_size)?,
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
            let sample = consistent_copy_for_game(identity, self.game, &mut |page| Ok(pages.copy(page)))?;
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
                if let Err(error) = crate::acr_setup::ensure_single_producer_for_game(self.pid, self.game) {
                    self.close();
                    return Err(error);
                }
                self.producer_checked_at = Some(std::time::Instant::now());
            }
            match self.read() {
                Ok(sample) => {
                    let packet = parse_for_game(&sample, self.game)?.packet_id;
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
}
#[cfg(windows)]
pub(crate) use platform::WindowsSource;

#[cfg(not(windows))]
pub(crate) struct WindowsSource;
#[cfg(not(windows))]
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
        selected_game_image_matches(expected_image, expected_image, game)?;
        Err("Live ACR shared memory is available on Windows; use a fixture source on this platform.".into())
    }
    pub fn process_image(&self) -> Option<&str> {
        None
    }
}
#[cfg(not(windows))]
impl Source for WindowsSource {
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        Err("Live ACR shared memory is available on Windows.".into())
    }
    fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(page: Page) -> Vec<u8> {
        match page {
            Page::Physics => include_bytes!("../../tests/fixtures/acr/live.physics.bin").to_vec(),
            Page::Graphics => include_bytes!("../../tests/fixtures/acr/live.graphics.bin").to_vec(),
            Page::Statics => include_bytes!("../../tests/fixtures/acr/live.static.bin").to_vec(),
        }
    }
    fn identity() -> SourceIdentity {
        SourceIdentity {
            id: "fixture".into(),
            epoch: 1,
        }
    }
    #[test]
    fn stable_owned_page_copies_match_the_production_parser() {
        let sample = consistent_copy(identity(), &mut |page| Ok(bytes(page))).unwrap();
        assert_eq!(
            parse(&sample).unwrap().channels["rpm"].value,
            Some(serde_json::json!(6300))
        );
        let mut copies = 0;
        let sample = consistent_copy(identity(), &mut |page| {
            copies += 1;
            let mut data = bytes(page);
            data.extend([1, 2, 3]);
            Ok(data)
        })
        .unwrap();
        assert_eq!(sample.source, identity());
        assert_eq!(copies, 6);
        assert_eq!(sample.statics.len(), STATIC_SIZE + 3);
    }
    #[test]
    fn a_torn_read_retries_and_unstable_or_missing_pages_fail_with_bounded_work() {
        let mut physics_reads = 0;
        let sample = consistent_copy(identity(), &mut |page| {
            let mut data = bytes(page);
            if matches!(page, Page::Physics) {
                physics_reads += 1;
                data[..4]
                    .copy_from_slice(&(if physics_reads == 1 { 10u32 } else { 11 }).to_le_bytes());
            }
            Ok(data)
        })
        .unwrap();
        assert_eq!(parse(&sample).unwrap().packet_id, 11);
        assert_eq!(physics_reads, 4);
        physics_reads = 0;
        assert!(consistent_copy(identity(), &mut |page| {
            let mut data = bytes(page);
            if matches!(page, Page::Physics) {
                physics_reads += 1;
                data[..4].copy_from_slice(&(physics_reads as u32).to_le_bytes());
            }
            Ok(data)
        })
        .is_err());
        assert_eq!(physics_reads, 6);
        assert!(consistent_copy(identity(), &mut |_| Err("mapping unavailable".into())).is_err());
        assert!(consistent_copy(identity(), &mut |_| Ok(vec![0])).is_err());
    }
    #[test]
    fn current_rpm_limit_is_part_of_the_stable_copy_even_when_the_packet_is_unchanged() {
        let mut physics_reads = 0;
        let sample = consistent_copy(identity(), &mut |page| {
            let mut data = bytes(page);
            if matches!(page, Page::Physics) {
                physics_reads += 1;
                data.resize(PHYSICS_EXTENDED_SIZE, 0);
                let maximum: i32 = if physics_reads == 1 { 8500 } else { 9000 };
                data[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                    .copy_from_slice(&maximum.to_le_bytes());
            }
            Ok(data)
        })
        .unwrap();
        assert_eq!(physics_reads, 4);
        assert_eq!(
            parse(&sample).unwrap().channels["max_rpm"].value,
            Some(serde_json::json!(9000))
        );

        physics_reads = 0;
        assert!(consistent_copy(identity(), &mut |page| {
            let mut data = bytes(page);
            if matches!(page, Page::Physics) {
                physics_reads += 1;
                data.resize(PHYSICS_EXTENDED_SIZE, 0);
                let maximum: i32 = 8500 + physics_reads;
                data[PHYSICS_CURRENT_MAX_RPM_OFFSET..PHYSICS_EXTENDED_SIZE]
                    .copy_from_slice(&maximum.to_le_bytes());
            }
            Ok(data)
        })
        .is_err());
        assert_eq!(physics_reads, 6);
    }
    #[test]
    fn mapping_reset_policy_distinguishes_wrap_duplicates_and_backward_counters() {
        assert!(!counter_reset(None, 0));
        assert!(!counter_reset(Some(10), 10));
        assert!(!counter_reset(Some(10), 11));
        assert!(!counter_reset(Some(u32::MAX), 0));
        assert!(counter_reset(Some(10), 0));
        assert!(counter_reset(Some(10), 9));
    }
    #[test]
    fn selected_pid_requires_an_explicit_matching_game_executable_basename() {
        assert!(selected_image_matches("C:\\Games\\ACR\\ACR.exe", "acr.exe").is_ok());
        assert!(selected_image_matches(
            "C:\\Games\\ACR\\ACR-Win64-Shipping.exe",
            "ACR-Win64-Shipping.exe"
        )
        .is_ok());
        assert!(selected_image_matches("C:\\Games\\ACC\\acc.exe", "acr.exe").is_err());
        for expected in ["", "../acr.exe", "acr", "a.exe\0", &"a".repeat(129)] {
            assert!(selected_image_matches("acr.exe", expected).is_err());
        }
    }
    #[test]
    fn native_producer_identity_cannot_be_relabelled_as_a_different_assetto_game() {
        let producers = [
            (Game::Corsa, "acs.exe"), (Game::Corsa, "acs_x86.exe"),
            (Game::Competizione, "AC2-Win64-Shipping.exe"),
            (Game::Evo, "AssettoCorsaEVO.exe"), (Game::Rally, "acr.exe"),
        ];
        for (owner, image) in producers {
            assert!(selected_game_image_matches(&format!("C:\\Games\\{image}"), image, owner).is_ok());
            for game in [Game::Corsa, Game::Competizione, Game::Evo, Game::Rally] {
                assert_eq!(selected_game_image_matches(image, image, game).is_ok(), owner == game);
            }
        }
        assert!(selected_game_image_matches("AssettoCorsa.exe", "AssettoCorsa.exe", Game::Corsa).is_err());
        assert!(selected_game_image_matches("AC2.exe", "AC2.exe", Game::Competizione).is_err());
    }
    #[test]
    fn evo_stability_check_uses_its_graphics_car_identity_and_narrow_static_metadata() {
        let mut car_reads = 0;
        let result = consistent_copy_for_game(identity(), Game::Evo, &mut |page| {
            let mut data = match page {
                Page::Physics => vec![0; PHYSICS_EXTENDED_SIZE],
                Page::Graphics => vec![0; EVO_GRAPHICS_SIZE],
                Page::Statics => vec![0; Game::Evo.page_lengths().2],
            };
            if matches!(page, Page::Graphics) {
                car_reads += 1;
                data[EVO_CAR_MODEL_OFFSET] = if car_reads == 1 { b'A' } else { b'B' };
            }
            Ok(data)
        }).unwrap();
        assert_eq!(car_reads, 4);
        assert_eq!(parse_for_game(&result, Game::Evo).unwrap().context.car_model, "B");
    }
    #[cfg(not(windows))]
    #[test]
    fn unsupported_live_platform_is_explicit_and_does_not_manufacture_game_data() {
        assert!(WindowsSource::new(0).is_err());
        assert!(WindowsSource::new(123).is_err());
        let mut source = WindowsSource;
        assert!(source.poll().is_err());
        assert_eq!(source.process_image(), None);
        source.close();
    }
}

#[cfg(all(test, windows))]
mod live_validation {
    use super::*;
    #[test]
    #[ignore = "requires explicit OSG_ACR_LIVE_PID and a running ACR driving session"]
    fn selected_running_game_produces_fresh_native_telemetry() {
        let pid = std::env::var("OSG_ACR_LIVE_PID")
            .expect("set live PID")
            .parse()
            .unwrap();
        let mut source = WindowsSource::new(pid).unwrap();
        let mut session = crate::game_telemetry::Session::new(1500).unwrap();
        let start = std::time::Instant::now();
        let mut active = false;
        for _ in 0..20 {
            let snapshot = session.poll(&mut source, start.elapsed().as_millis() as u64);
            if snapshot.fresh {
                assert!(snapshot.values().contains_key("rpm"));
                assert!(snapshot.values().contains_key("gear"));
                assert!(snapshot.values().contains_key("speed_kmh"));
                println!("LIVE ACR {}", snapshot.value());
                active = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        source.close();
        assert!(active, "native acquisition never produced fresh telemetry");
    }
}

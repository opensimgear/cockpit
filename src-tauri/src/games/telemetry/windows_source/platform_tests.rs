//! Owned Windows mappings exercise the actual read-only ABI. These are not
//! simulator qualification, and never create the simulator's public page names.
use super::*;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileMappingW(
        file: Handle,
        attributes: *const c_void,
        protect: u32,
        high: u32,
        low: u32,
        name: *const u16,
    ) -> Handle;
    fn GetLastError() -> u32;
}
struct Producer {
    handle: Handle,
    view: *mut u8,
    length: usize,
}
impl Producer {
    fn new(name: &str, length: usize) -> Self {
        let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileMappingW(
                -1isize as Handle,
                ptr::null(),
                4,
                0,
                length as u32,
                name.as_ptr(),
            )
        };
        assert!(!handle.is_null());
        // A fixture may only write mappings that it created itself.
        assert_ne!(
            unsafe { GetLastError() },
            183,
            "Fixture mapping already exists"
        );
        let view = unsafe { MapViewOfFile(handle, 2 | READ, 0, 0, length) } as *mut u8;
        assert!(!view.is_null());
        unsafe { ptr::write_bytes(view, 0, length) };
        Self {
            handle,
            view,
            length,
        }
    }
    fn packet(&self, value: u32) {
        assert!(self.length >= 4);
        for (index, byte) in value.to_le_bytes().iter().enumerate() {
            unsafe { ptr::write_volatile(self.view.add(index), *byte) };
        }
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view.cast());
            CloseHandle(self.handle)
        };
    }
}

#[test]
fn owned_mapping_copies_are_bounded_and_missing_or_oversized_views_fail() {
    let name = format!("Local\\osg-native-map-{}", uuid::Uuid::new_v4());
    assert!(Mapping::open(&name, 128).is_err());
    let producer = Producer::new(&name, 4096);
    producer.packet(42);
    let reader = Mapping::open(&name, 128).unwrap();
    assert_eq!(reader.copy().len(), 128);
    assert_eq!(&reader.copy()[..4], 42u32.to_le_bytes());
    assert!(Mapping::open(&name, usize::MAX).is_err());
    producer.packet(43);
    assert_eq!(&reader.copy()[..4], 43u32.to_le_bytes());
    drop(reader);
    drop(producer);
    assert!(Mapping::open(&name, 128).is_err());
}

#[test]
fn real_process_and_owned_pages_preserve_identity_packet_resets_and_close_on_errors() {
    let pid = std::process::id();
    assert!(WindowsSource::new(0).is_err());
    assert!(WindowsSource::new_for_game(u32::MAX, "acr.exe", Game::Rally).is_err());
    assert!(WindowsSource::new_for_image(pid, "acr.exe").is_err());
    for game in [Game::Rally, Game::Corsa, Game::Competizione, Game::Evo] {
        let prefix = format!("Local\\osg-native-pages-{}", uuid::Uuid::new_v4());
        let (physics, graphics, statics) = game.full_page_lengths();
        let physics = Producer::new(&format!("{prefix}_physics"), physics);
        let graphics = Producer::new(&format!("{prefix}_graphics"), graphics);
        let statics = Producer::new(&format!("{prefix}_static"), statics);
        physics.packet(10);
        let pages = Pages::open_named(game, &prefix).unwrap();
        assert_eq!(pages.copy(Page::Physics).len(), game.full_page_lengths().0);
        let process = Process::open(pid).unwrap();
        assert!(process.running());
        let epoch = process.epoch;
        let mut source = WindowsSource {
            game,
            pid,
            expected_image: "acr.exe".into(),
            process: Some(process),
            pages: Some(pages),
            last_packet: None,
            producer_checked_at: Some(std::time::Instant::now()),
        };
        assert!(source
            .process_image()
            .unwrap()
            .to_lowercase()
            .contains("osg_cockpit"));
        let first = source.poll().unwrap().unwrap();
        assert_eq!(first.source.epoch, epoch);
        assert_eq!(first.source.id, game.source_id(pid));
        assert_eq!(&first.physics[..4], 10u32.to_le_bytes());
        source.producer_checked_at = None;
        physics.packet(11);
        assert_eq!(
            &source.poll().unwrap().unwrap().physics[..4],
            11u32.to_le_bytes()
        );
        physics.packet(1);
        assert!(source.poll().unwrap().is_some());
        assert!(source.pages.is_none());
        assert!(source.process.is_none());
        assert!(source.poll().is_err());
        assert!(source.process_image().is_none());
        assert!(source.last_packet.is_none());
        source.close();
        drop(source);
        drop((physics, graphics, statics));
        assert!(Pages::open_named(game, &prefix).is_err());
    }
}

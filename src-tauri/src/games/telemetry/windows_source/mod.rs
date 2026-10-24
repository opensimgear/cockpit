//! Read existing Assetto named pages without creating or writing game mappings.
//! Shared names are not a game identity; the caller explicitly selects a PID.
#[cfg(test)]
use crate::game_telemetry::{
    parse, parse_for_game, EVO_CAR_MODEL_OFFSET, EVO_GRAPHICS_SIZE, PHYSICS_CURRENT_MAX_RPM_OFFSET,
    PHYSICS_EXTENDED_SIZE, STATIC_SIZE,
};
use crate::game_telemetry::{
    validate_for_game, Game, RawSample, Source, SourceIdentity, PHYSICS_SIZE,
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
    if !known {
        return Err(format!(
            "The selected executable is not a {} telemetry producer.",
            game.id()
        ));
    }
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
        validate_for_game(&first, game)?;
        validate_for_game(&second, game)?;
        if first.physics == second.physics
            && first.graphics == second.graphics
            && first.statics == second.statics
        {
            return Ok(second);
        }
    }
    Err("Assetto telemetry changed during all bounded snapshot attempts.".into())
}

#[cfg(windows)]
mod platform;
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
mod tests;
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

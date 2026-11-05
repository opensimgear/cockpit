use windows_sys::Win32::System::WindowsProgramming::QueryInterruptTimePrecise;

/// One precise, sleep-inclusive system clock shared by parent and helper.
/// GetTickCount64 quantizes deadlines to the 10–16 ms system timer tick.
pub fn now_us() -> u64 {
    let mut interrupt_time = 0;
    unsafe { QueryInterruptTimePrecise(&mut interrupt_time); }
    interrupt_time / 10
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;

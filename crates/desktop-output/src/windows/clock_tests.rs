use super::now_us;
use windows_sys::Win32::System::WindowsProgramming::QueryInterruptTimePrecise;

#[test]
fn native_deadline_clock_matches_precise_interrupt_time() {
    let mut sub_millisecond = false;
    for _ in 0..64 {
        let (mut before, mut after) = (0, 0);
        // All three samples use one system-wide, sleep-inclusive clock domain.
        unsafe { QueryInterruptTimePrecise(&mut before); }
        let sampled = now_us();
        unsafe { QueryInterruptTimePrecise(&mut after); }
        assert!(before / 10 <= sampled && sampled <= after / 10,
            "Deadline clock {sampled} is outside precise samples {}..{}", before / 10, after / 10);
        sub_millisecond |= sampled % 1000 != 0;
    }
    assert!(sub_millisecond, "The native deadline clock was quantized to milliseconds");
}

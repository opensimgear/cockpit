# Windows helper process exit waits

The desktop transport now blocks on the exact spawned process handle
during the existing 350 ms shutdown grace period. If it terminates the
worker, it waits up to 500 ms for confirmed exit. The audit distinguishes
ordinary exit, forced exit and unconfirmed exit. Forced exit does not
prove release of a key or button. The portable transport's non-Windows
cleanup is unchanged. Cockpit enables the native desktop worker on Windows.

The explicit vJoy refresh helper uses the same Windows process-wait API
with its existing refresh and reap deadlines. A wait failure or timeout
still triggers termination. A missing exit receipt retains the durable
refresh guard, and the diagnostic preserves the initial wait error.
The tests exercised harmless child processes and did not restart a driver.

## Local checks

All 192 desktop-output and 113 virtual-controller tests passed. Both
configured crate coverage gates passed for all 53 modules. The Windows
desktop shutdown region reached 85% production line coverage. The whole
transport file measured 19.40% in the standalone suite because its native
IPC paths are outside the crate's portable gate. The vJoy refresh file
reached 92.15% production line coverage. LLVM reported no Rust branch
coverage. The full Windows Cockpit suite passed 2,432 tests with 18
default ignores.

## Running Windows Cockpit

The rebuilt dev app ran a neutral profile in a disposable database. The
test recorded the desktop worker's PID and creation time under the dedicated
Cockpit process. After Stop, the backend confirmed cleanup with empty held
and possibly held lists. Windows reported no remaining desktop helper while
the original Cockpit process was still live. The fixture sent no non-neutral
keyboard or mouse input. It exercised the desktop lifecycle directly without
depending on a configured simulator.

Native executable SHA-256
`2e8d87fb0591cf0f1947ca8a4e5a46a30fa7ce0047f3e7913dcb850db19ad915`.
The local live record is `.research/reactive/windows-child-waits/live-neutral.log`.

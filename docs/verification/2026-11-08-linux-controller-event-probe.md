# Linux controller event probe

The opt-in Linux controller probe now opens the completed udev input monitor
before its initial scan. The worker waits for evdev report readiness, udev
notifications and an eventfd shared by control and shutdown. It processes
ready reports before rescanning after topology changes. It withdraws exact
sources on removal or descriptor loss and stops capture if monitor continuity
fails. The wait has a deadline only while suppression is awaiting its baseline.
[Linux input event protocol](https://docs.kernel.org/input/event-codes.html) ·
[udev monitor interface](https://www.freedesktop.org/software/systemd/man/latest/udev_monitor_receive_device.html)

An evdev overrun withdraws the previous generation and requires a kernel
snapshot. A device waiting for its resynchronization boundary stays parked
until its own descriptor becomes ready. A bounded drain that consumes its
entire batch explicitly schedules another turn. Reports from a peer cannot
trigger a read from the waiting device.

The offline Ubuntu WSL controller suite passed 179 tests. Its descriptor
fixtures use real kernel readiness to cover report delivery, overflow recovery,
topology retirement, monitor loss and command or shutdown wakes. The actual
read-only probe registered with the running udev service and stopped cleanly.
It observed zero eligible controllers. The Linux worker reached 81.82%
production line coverage and 80% LLVM function coverage. Shared capture,
evdev decoding, the controller service and suppression also exceeded 71%
production line coverage. The instrumented Windows controller suite passed
174 tests. The rebuilt Windows native and browser-dev clients each observed
an exact controller metadata rename, restore and reader reopen.

Physical Linux report cadence and unplug or replug remain in #199. Ordinary
Linux capture stays behind the existing platform gate. The macOS probe still
has timed reads and topology checks.

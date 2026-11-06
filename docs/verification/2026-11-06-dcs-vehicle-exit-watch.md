# DCS vehicle process exit watch

The DCS vehicle reader retains a Windows process handle that matches the
selected PID, creation time and executable. A blocking kernel wait observes
process exit. A separate stop event cancels the wait when the reader retires.
This path requires access to the selected process handle and uses no WMI
subscription or elevation request.

The vehicle tracker retains only weak process evidence. Process exit wakes the
capture worker and invalidates the cached BIOS and ownship views, including
while the reader is detached. The quiet reader skips the 100 ms process check
when the exact handle and the required capture wakes are available. If one of
those registrations fails, the existing maintenance path still covers it.

The native regression uses a real Windows child to reject a mismatched creation
epoch, cancel a wait without stopping its process, and verify notification on
process exit. The DCS scheduler regression exercises an exact process watch and
the ownship retry deadline while BIOS is silent. Process discovery and applied
profile validation remain separate work in #188. Actual simulator and hardware
qualification remain in #199.

The Windows dev build passed with executable SHA-256
`cf8a4b64d9499e4e1bff8cbe521f2ab21a600f31c897831a869d7d4022c72ee8`.
The running Cockpit passed the passive telemetry inspector and workspace
subscription checks. The temporary unlaunched catalogue entries were removed.
All 14 frontend telemetry tests passed, and the timing audit accounts for all
247 pinned sites.

The complete native suite passed 2,346 tests with 18 ignored. The new ignored
entry is the child fixture launched by the exit-watch regression. All six
coverage gates passed. The exit watcher measured 92.96%, its native helpers
88.89%, and the vehicle sources, tracker, polling and views measured 89.44%,
98.09%, 94.78% and 99.21%.

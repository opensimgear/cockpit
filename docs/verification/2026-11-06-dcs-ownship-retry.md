# DCS ownship retry scheduling

The vehicle worker includes a missing DCS ownship source's retry deadline when
it parks. The deadline can schedule capture work without any BIOS packet or
process maintenance tick. A retry does not extend process evidence. A failed
attempt schedules the existing two-second backoff, while an acquired source
uses its native wake.

The regression uses the native BIOS reader with a private loopback port and no
packets. It verifies that the retry is ineligible before the deadline, becomes
eligible independently of the process schedule, and preserves the original
process observation time. Actual simulator qualification remains in #199.

The Windows dev build passed with executable SHA-256
`a600ce3b3376083caea2a19d823141faa7d3d4419b75293cfe256201695bcbcd`.
The rebuilt Cockpit passed the passive telemetry inspector and workspace
subscription checks. The inspector used two temporary unlaunched catalogue
entries, removed afterward. The timing audit accounts for all 247 pinned sites.
All 14 frontend telemetry tests passed. The native suite passed 2,345 tests
with 17 ignored. Four coverage gates passed. Vehicle sources measured 89.46%,
the tracker 98.09%, vehicle polling 94.78%, and the context runtime 95.27%.

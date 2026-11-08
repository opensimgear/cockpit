# Passive telemetry capture by integration

Cockpit now gives each registered game integration its own capture worker.
Source capture, process exit, and inspection demand wake the affected worker.
Library and setup commits wake all workers to recheck their retained readers.
A worker waits for its next source notification or an existing freshness or
inspection deadline. The tracker still qualifies installation, setup,
process evidence, and reader generation before publishing a result.

The Windows regression holds one adapter inside a native read while another
consumes two authored Assetto packets and publishes live telemetry. It then
removes the blocked adapter's installation, releases its read, and checks
that the late result is discarded. All 93 focused native tests passed.

The full Windows native suite passed 2,462 tests with 18 default skips after
freeing build-cache space. Production line coverage ranged from 85.60% to
100% across the ten changed modules. The two new modules reached 100% line
and LLVM function coverage. The existing setup cache reached all 20 named
production functions and 63.89% of LLVM's function records, which also count
closures. LLVM emitted no branch counts. Two empty profiles from the
earlier runs were excluded from the merge.

The rebuilt Windows dev Cockpit passed the authored DCS ownship and BIOS
fixture. The test exercised independent withdrawal and recovery, a stalled
inspector reader, a competing process, and process exit. Cleanup reported
zero errors. Executable SHA-256
`907c9c916a63061104551fae5d8e4bf51d91a673ae133b5731a21a95953fa016`.

Provider startup isolation remains under #192. Tests with live simulators
remain in #199.

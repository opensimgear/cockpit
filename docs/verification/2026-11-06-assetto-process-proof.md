# Retained Assetto vehicle process evidence

The vehicle worker uses the capture owner's exact native process monitor for
watched Assetto readers. The owner binds the selected PID, creation epoch and
normalized executable path before sharing the stream. The watch holds a weak
monitor reference so a cached view cannot retain the process owner. A stopped
monitor or closed stream withdraws the cached view, including while the reader
is detached for decoding. Installation and setup checks still fence publication.

The acquisition owner keeps the exact Windows process handle across mapping
errors and packet resets. Handle exit is terminal. The same owner can therefore
report process loss before any valid capture. A quiet watched Assetto reader
no longer schedules the vehicle worker's 100 ms process verification. It wakes
on source changes, terminal state, or the local capture expiry deadline.

The approved per-game sampler cadence remains. Non-Assetto readers and readers
without retained process evidence still use vehicle process maintenance. The
separate game-context process pass and applied-profile validation are outside
this vehicle worker change. User consent for Windows process monitoring and
actual simulator qualification remain in #199.

- The Windows dev build passed. Its executable SHA-256 is
  `25cc539caf390f6b4a5acec71b159e86c2e84ad6272c22de956d457f0c1c1b1c`.
- The running Cockpit passed the passive telemetry inspector lifecycle and
  the shared workspace subscription checks. Temporary unlaunched catalogue
  entries were removed after the inspector check. All 14 frontend telemetry
  tests passed, and the timing audit accounts for all 247 pinned sites.
- Focused native tests verify a retained monitor before the first packet, its
  loss waking subscribers, continued process identity across mapping reset,
  and a detached result being discarded after the monitor retires. The quiet
  watched reader needs no process timer or extra provider read at expiry.
- The complete native suite passed 2,343 tests with 17 ignored. Nine coverage
  gates passed, from 76.36% on the Windows source to 99.38% on the telemetry
  module. The shared source, vehicle tracker and context runtime measured
  97.17%, 98.04% and 95.27% respectively.

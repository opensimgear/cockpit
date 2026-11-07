# DCS ownship tracker wake

The passive vehicle tracker previously consumed an ordinary ownship journal
slot. If the journal was full, the tracker also discarded its exact Windows
process-exit evidence and fell back to the 100 ms qualification pass.

The ownship source now keeps a weak notification for its vehicle tracker.
Registration delivers the current baseline wake. Source commits wake the
tracker for new data, expiry, publication failure and receiver closure.
Profile and inspection consumers continue to use the bounded journal.
Receiver closure has its own state, independent of a mission's stopped
telemetry. The tracker joins a closed receiver before retrying its socket.

The native regression fills all 64 ordinary watch slots, verifies that the
Windows exit watch remains authoritative, and receives the next UDP commit
through the tracker wake. The source tests cover publication failure,
terminal wake delivery and weak ownership.

The running Windows Cockpit passed the authored DCS fixture with no cleanup
errors. Passive inspection and the applied output-free profile received
ownship and BIOS data. Ownship expiry left BIOS live. BIOS expiry left
ownship live. Confirmed exit withdrew both. The fixture also exercised
competing process withdrawal, pause, export denial, respawn and recovery.
Evidence is in `.research/dcs-physics-native/1794066102341/evidence.json`.
The executable SHA-256 was
`2834ee5e435ca915c2d94af91ccb07cf5908637faa408ec677b7ce097bc72244`.

All 2,410 Windows tests passed with 18 default ignores. Both pinned
DCS-BIOS archive tests and 14 frontend telemetry tests passed separately.
Production line coverage was 94.95% for the ownship source and 89.38%
for vehicle sources. LLVM function coverage was 72.97% and 92.68%,
respectively. The pinned build recorded no branch counters.

The 100 ms process fallback still applies when exact native process
evidence is unavailable. Simulator export and physical acceptance remain
in #199.

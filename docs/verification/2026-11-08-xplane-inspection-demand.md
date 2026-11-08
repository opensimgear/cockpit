# X-Plane passive inspection demand

The telemetry inspector validates requested channels against the retained
X-Plane catalogue and records the bounded union of inspection leases. The
shared vehicle tracker releases its lock before the capture worker sends
the native selection command and waits for a receipt. A changed union wakes
the capture worker even when the source is quiet. Closing the last inspector
uses the same path to remove demand.

The native regression verifies that requesting a quiet inspector leaves the
source selection unchanged until the capture worker processes the demand.
It verifies the selected channels afterward and checks that closing the
lease wakes capture to release them.

The full Windows native suite passed 2,461 tests with 18 default ignores.
The optional installed X-Plane SDK catalogue test passed separately.
Production line and LLVM function coverage were 90.14% and 85.00% for
the owner client, 89.04% and 93.33% for the source adapter, and 99.24%
and 92.59% for the tracker view. LLVM emitted no branch counts. One
truncated subprocess profile was excluded from the merge. The full
suite's main profile and the focused profiles were retained.

The rebuilt Windows Cockpit passed setup and subscribed catalogue changes
for private X-Plane 11 and 12 installations. Executable SHA-256
`9ccca16628e730e14cdcc20e9671172702dcfec80b89911fd76e71fbf724a26d`.
The fixture checked catalogue delivery. Simulator telemetry and
process-monitor consent remain in #199.

The capture worker still processes adapter projections in sequence.
Cross-adapter projection and startup isolation remain under #192.

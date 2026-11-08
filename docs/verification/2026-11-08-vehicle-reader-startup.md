# Passive vehicle reader startup

Once the context observer has qualified an installed game and its process,
it queues the native reader open on that integration's startup worker.
Native open, initial inspection, and capture-watch registration happen
outside the shared tracker. The worker checks the installation, setup
readiness, and exact process before and after the open. The tracker checks
the observation generation and signature before admitting the reader.
A failed open wakes the context owner to schedule the existing retry.

Each game retains one pending open. A replacement supersedes pending work
for the old process. An in-flight open remains fenced by its original
scope. Startup and capture use separate workers so an open cannot hold up
capture projection or freshness expiry.

The Windows regression holds one integration's open, then dispatches
another integration and two replacements for the blocked one. It verifies
that the second integration starts before the first is released and only
the newest replacement runs afterward.

The full Windows native suite passed 2,463 tests with 18 default skips.
The six changed production modules reached 94.72% to 100% line coverage.
The new startup worker reached 96.43% production line coverage and
100% LLVM function coverage. LLVM emitted no branch counts.

The rebuilt Windows dev Cockpit passed the authored DCS fixture. It checked
startup and passive inspection, withdrawal when a competing process appears,
recovery with a sole process, and independent BIOS and ownship expiry and
recovery with a stalled inspector reader. The fixture reported no cleanup
errors. Executable SHA-256
`acea4d9a1a8fb645573e8ce23dbefd6dcd5d314848b80624bd03911fff1f41e6`.

Process inventory and setup qualification still share the context observer.
Actual simulator acceptance remains in #199.

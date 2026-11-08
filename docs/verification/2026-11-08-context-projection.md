# Per-integration game context qualification

The context observer owns the shared process inventory and reconciles the
complete game library. It dispatches background qualification by integration.
Executable validation, exact process liveness, initial passive reader
admission, and catalogue lookup run on each integration's worker. Native
reader startup and capture have separate workers. Explicit profile preparation
continues to wait for its own context observation.

Each integration retains the latest pending batch. A worker checks the library,
process, and setup scope before publication. A newer fallback result for the
same game can supersede a blocked check even when the watch epoch stays the
same. The tracker and broker still fence publication by observation generation.

The native regression stalls an exact process check and verifies a second
integration publishes while it waits. It exercises a process epoch change and
a replacement from the shared fallback. In each case the old result is
discarded when the blocked check returns.

The full Windows native suite passed 2,465 tests with 18 default skips.
All 96 focused native tests and 42 related frontend tests passed.
The context qualification module reached 97.83% production line coverage
and 89.19% LLVM function coverage. The context runtime reached 95.37%
production line coverage and 70.83% LLVM function coverage. LLVM emitted
no branch counts.

The rebuilt Windows dev Cockpit passed the authored DCS fixture. It verified
competing-process withdrawal, recovery with one process, independent BIOS
and ownship expiry, and process exit. Cleanup reported no errors.
Executable SHA-256
`580fa3ec2726eab3f5011fea8f8dee47997bc2fbbdff326a547f065df126a8e7`.

Setup file watch registration and setup baseline refresh still run on the
shared observer. Real simulator and process-monitor consent tests remain in
#199.

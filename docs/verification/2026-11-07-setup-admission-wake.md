# Deferred game setup inspection

Watched DCS and Assetto file changes retain their pending inspection when
the setup owner is busy. The synchronous admission guard signals the
context worker after unlocking. Async setup completion already signals
the worker after clearing its busy state. The context worker can park
between notifications when process and file coverage are live.

If the first try-lock fails, the observer records demand and tries again.
This covers a release between the failed attempt and demand registration.
Multiple deferred inspections share one release wake. The periodic
fallback checks its scope before entering admission and skips fully watched
providers. Uncovered file paths, process fallback and watcher registration
retries retain their separate deadlines.

The running Windows Cockpit passed the DCS catalogue edit, blocked path
recovery and reopen fixture. It also passed the managed DCS setup and repair
fixture, which applied an output-free profile to authored BIOS and ownship
streams and verified independent expiry and process exit. Both fixtures
used private configuration. The setup run recorded zero cleanup errors.

The desktop executable SHA-256 was
`1b4ac7dcdb098c4f8c44b5cb2e37f909b3d4ea42849b935806b00a9c04c3a732`.
Logs are under `.research/reactive/setup-admission-wake/`. The managed
setup trace is in `.research/dcs-physics-native/1794066780992/evidence.json`.

The instrumented Windows build initially exhausted disk space. The retry
kept coverage instrumentation and omitted debug symbols for the Cockpit
test crate. The first suite run passed 2,405 tests with 18 default ignores.
Seven X-Plane helper tests failed while copying their fixture executable
under concurrent disk pressure. All seven passed serially with the same
binary. The initial coverage file was incomplete. A full serial run with
one shared profile passed all 2,412 tests with the same 18 default ignores.
The changed modules measured 81.82–100% production line coverage. LLVM's
supplementary function report measured 74.75% across those files, with
65.91% for game context and 68.75% for the DCS watch. The build recorded
no branch counters.

Both pinned DCS archive tests and 71 frontend provider and read-model tests
also passed. The provider coverage gates reached 100% lines, branches and
functions after rerunning with sufficient temporary space.

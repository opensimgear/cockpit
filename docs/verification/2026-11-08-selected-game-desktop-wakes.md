# Selected-game desktop scope wakes

An active Windows profile that combines a watched game source with a healthy
selected-game keyboard and mouse scope can use the shared routing wake.
Committed process changes and observer retirement notify routing. Observations
from the one-second discovery fallback carry the observer's two-second
freshness deadline. That deadline shares the host's existing native
deadline selection. On expiry or unavailable evidence the desktop adapter
releases its exact process scope and waits. A contended read keeps the exact
scope and retries using the existing service bound.

The native helper continues to enforce executable, PID, creation epoch and
foreground eligibility. A changed process scope completes release before it
binds a replacement, which requires fresh neutral input. Child-owned launch,
paused vehicle transitions and unwatched adapters keep bounded service.

## Running Windows Cockpit

The same authored DCS executable and telemetry fixture ran on the previous
build and the candidate using a private database. The previous build
reported a 5 ms profile service interval. The candidate reported a null
interval with a healthy parent watch and live helper lease. Its scope bound
the fixture's executable, PID and creation epoch. The same scope and helper
admission remained live after 1,100 ms without gateway reads. The executable
stayed in the background and the fixture supplied only a neutral desktop
value. BIOS and ownship still reached the profile, with their existing
independent expiry and recovery.

After process exit, both telemetry families withdrew and the desktop scope
became Waiting. The helper receipt and durable process scope were absent,
cleanup was complete, and held and possibly held controls were empty.
The run restored its preferences and installation with zero cleanup errors.
Evidence is in `.research/dcs-physics-native/1794102740672/evidence.json`.
The same build passed the explicit desktop-wide regression with zero cleanup
errors. Its evidence is in `.research/dcs-physics-native/1794103277470/evidence.json`.
The dev executable SHA-256 was
`caac4671b83f2d04168c4a8be865aa53b2483ac930de7f88d0bb6e72928deed8`.

This fixture exercises the approved one-second discovery fallback. Live
Windows event access, real DCS effects and physical controller latency remain
in [#199](https://github.com/opensimgear/cockpit/issues/199). The fixture does
not send a desktop key or move the pointer.

## Local checks

The isolated Windows suite passed 2,425 tests with 18 default ignores. Both
pinned DCS archive checks and 62 focused regressions passed separately.
The six changed modules reached 78.12–98.85% production line coverage and
70.77–87.30% reported function coverage. The native instrumented build did
not report branches. The first sandboxed test pass
could not receive UDP in the affected fixtures. The serial run used a private
temporary directory and Windows loopback access. The tested Rust, manifest
and SQL files match the staged source after line-ending normalization. The
isolated Cargo manifest builds the Rust library and omits debug symbols for
the dev binary to fit the local disk.

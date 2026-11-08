# Watched process inventory retries

The context observer keeps definitive process results for the exact watched
library and process-event epoch. If a batch includes an unavailable row,
the context cache retries that row while preserving the successful, absent,
and ambiguous rows. A new epoch or a changed installation selection requires
a fresh inventory. This still uses the shared Windows process owner.

A full inventory whose row count differs from the request returns an
unavailable result for each game. The cache keeps the previous definitive
rows when a partial retry is incomplete. This prevents a misaligned result
from being applied to the wrong game.

If the library or process epoch changes during capture, or if the watch
loses continuity, Cockpit discards the result before passing it to the
context broker or publishing it for active profile validation. The
unavailable result follows the existing proof lifetime. Strict command
admission still checks the exact process independently.

The cache regression exercises a partial retry, an incomplete response,
recovery and a new event epoch. The scope regression changes the library
or process epoch or retires the watch inside the capture callback and checks
that neither consumer can use the superseded result.

The full Windows native suite passed 2,464 tests with 18 default skips.
All 95 focused native tests and 21 telemetry and process-monitor UI tests
passed. Production line coverage was 100% for the process cache and 96.02%
for the context runtime. LLVM function coverage was 100% and 76.19%,
respectively. LLVM emitted no branch counts.

The rebuilt Windows dev Cockpit passed the authored DCS fixture using the
shared Windows process fallback. It exercised competing-process withdrawal,
sole-process recovery, independent BIOS and ownship freshness, and exit.
Cleanup reported no errors. Executable SHA-256
`523fb00319d1ad26e3627a08e7367a1e7f0a28a5a3f325a885a8fc5c96a958f4`.

The first focused run exposed a race in the DCS test fixture. It now
establishes the tracker baseline after joining the background observers and
before injecting the aircraft. Only test code changed after the dev build
started.

The shared observer still performs per-game process qualification and
setup checks. Windows event consent and real simulator tests remain in #199.

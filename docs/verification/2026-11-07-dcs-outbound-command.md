# DCS commands during combined Windows load

The native Cockpit fixture registers the pinned DCS-BIOS 0.11.7 A-10C
catalogue for an authored A-10C_2 session. It applies a profile with two
one-shot routes. A native debug button sends `A102_HMCS_PW INC` and a
compiled browser companion button sends `AAP_CDUPWR DEC`. The receiver binds
exclusively to `127.0.0.1:7778` after the fixture verifies DCS is closed.

Both exact UDP datagrams must arrive once, in order, while the authored 1 kHz
and 125 Hz sources overlap. The same fixture verifies two live browser panel
canvases, a real Windows vJoy slot, and continuing progress with native and
MCP observation receipts held. It restores full BIOS before reapplying the
command profile and checks that Stop and reapply never replay either action.
The original telemetry-only variant still tests ownship-only reapply.

Before the fix, both action packs appeared in the applied reducer and the
aircraft route was writable, but successful Apply advanced the epoch that
the command worker checked against its preparation epoch. The successful
transition now binds the next epoch to the exact command owner while holding
the coordinator lock. Stop or another newer control can still cancel the
epoch before taking that lock. Launch binds the command epoch alongside its
existing held admission epoch.

After the epoch fix, a diagnostic run measured 14 ms in strict process
verification and a further 95 ms in catalogue verification. Inventory took
less than the trace's 1 ms resolution. The 100 ms deadline correctly rejected
the job. The passive validator and command worker now share the last
completed content check for the same immutable catalogue capture. Both still
check metadata when forced, and the full content check keeps its two-second
interval. File I/O runs outside the shared proof lock.

The fixture uses authored telemetry, an actual browser button and Cockpit's
real native UDP sender. Its timestamps start at the debug input offer and
browser click request. Physical-controller latency and actual DCS effects
remain in [user acceptance #199](https://github.com/opensimgear/cockpit/issues/199).

## Running desktop evidence

The final Windows run received both datagrams exactly once during 191.98 ms
of measured overlap. The debug input offer to receiver took 33.52 ms. The
browser click request to receiver took 32.30 ms. The real vJoy receiver saw
all 193 transitions at 1,000.00 Hz and all 25 transitions at 125.01 Hz. Source
offer to Windows Raw Input p95 was 11.89 ms and 12.29 ms for those lanes.
Both panels painted during the overlap. The fast native reader received 33
notifications while one receipt remained outstanding on its stalled sibling.
Both MCP readers retired cleanly. Stop, reapply, process exit, all fixture
timing budgets and cleanup passed.

Run `tests/ui/dcs-physics-native.mjs` using the combined desktop fixture with
`OSG_DCS_COMBINED_NATIVE_RATES=1` and `OSG_DCS_COMMANDS_NATIVE=1`. The local
evidence is `.research/dcs-physics-native/1794060098321/evidence.json`. The
Windows executable SHA-256 was
`CD1A5D5BE96F5FC95FB64718B14AA9DD43C03C7C42A9DC741BC86D38DEF8EC3F`.

## Local native checks

The Windows native suite passed 2,402 tests with 18 default ignores. The
enabled native qualification check and both pinned archive checks passed
separately. An earlier eight-worker run failed the process-cache timing test.
That test passed in isolation and in the final four-worker suite.

All seven measured production line gates passed, with a minimum of 80.52%.
LLVM measured 93.65% function coverage for `dcs_profile.rs`, 78.95% for
`profile_session_stage.rs`, 75.76% for `profile_session_host.rs` and 73.33%
for `profile_session_launch_host.rs`. The broader gateway, automatic
activation and editor save files measured 59.52%, 56.25% and 48.15%
respectively. The configured native gates for these modules use production
lines. The instrumented compiler emitted no branch counters for these files.

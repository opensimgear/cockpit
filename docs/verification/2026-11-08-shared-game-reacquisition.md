# Windows automatic game reacquisition

When a running profile loses its simulator reader, automatic recovery on
Windows uses the committed process inventory and integration context. An
absent, ambiguous, unavailable or expired shared row returns before
strict preparation. A current row for a different process also waits
until the context worker prepares that exact identity.

The replacement candidate checks the shared row again after inspecting
its catalogue, then goes through the existing strict process check,
reader acquisition and fresh-baseline admission. Explicit Apply still
performs its own preparation. The host retains its existing bounded
preparation retry and the new gate prevents retries from enumerating
processes or forcing setup refresh while shared evidence is absent.

## Local verification

The native regression checks missing, absent, ambiguous, unavailable,
expired and mismatched shared observations. All return before a platform
lookup. Once the committed row and prepared context match, the strict
path performs one identity lookup and one liveness check.

The focused run passed 55 tests. The broad Windows suite passed 2,477
tests with 18 default ignores. The STM32 deadline regression and both
checksum-pinned DCS archive checks passed separately. Production line
coverage reached 94.79% in the game context and 79.25% in the profile
host. LLVM function coverage reached 73.08% and 74.58% respectively.
The Windows instrumented build emitted no branch counters.

## Running Cockpit

Windows desktop and browser-dev both passed the authored DCS restart
scenario with default process-event settings. The fixture exited the
stand-in simulator, verified that both ownship and BIOS values withdrew,
and kept it absent for another 2.2 seconds. A replacement process and
export session restored the expected values under the same profile owner
without another Apply. Both runs also passed active catalogue invalidation
and recovery. Cleanup reported no errors.

| Transport | Evidence directory under `.research/dcs-physics-native` | Binary SHA-256 |
| --- | --- | --- |
| Windows desktop | `1794174632906` | `efdb8b005c08f5d6d0ca22e43e96b84e73fc8dfd33ee5a3323bd89b1f5a85ce7` |
| Browser-dev | `1794174694395` | `e84672bc96b23cc2c3538b3467e073ef35dd978923efbe3ebd927db16bb4dd0a` |

Set `OSG_DCS_PROCESS_RESTART=1` when running the normal
`tests/ui/dcs-physics-native.mjs` fixture to repeat the restart scenario.
The simulator packets are authored and physical acceptance remains in #199.

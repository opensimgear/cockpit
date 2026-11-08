# Direct shutdown wakes during USB and STM32 command recovery

Direct USB configuration reconciliation now waits on its actor wake until the
hardware deadline. The worker rechecks shutdown after each wake and preserves
the same deadline across unrelated notifications. The STM32 gateway worker
parks to its recovery deadline. Shutdown publishes cancellation and unparks
that worker before joining it. Action discovery uses the same interruptible
wait before retrying a read. Both paths retain the existing hardware settle
and retry delays, identity checks, and single-write reconciliation.

## Windows verification

The native suite passed 2,437 tests with 18 default ignores. Two ignored
tests passed separately with the pinned DCS-BIOS archive. The suite covered
the three new shutdown and deadline regressions, the existing bounded STM32
service shutdown and USB pending-reply shutdown tests, and the action
discovery retry. Coverage included execution of both new waits and the
STM32 shutdown unpark.

| Module | Production line coverage | LLVM function coverage |
| --- | ---: | ---: |
| Direct USB | 88.03% | 74.42% |
| STM32 | 89.34% | 92.27% |

The rebuilt native Cockpit passed both USB input and metadata view fixtures
in a private database. The views rejected unknown endpoints without taking
configuration or input authority. These fixtures exercised the running app
and IPC. The native software tests exercised recovery with authored peers.
The physical USB and CAN recovery checks remain in #199. The exact-port and
route-readiness retries still require producer wakes.

Dev executable SHA-256
`ec77d9bad6abd5e111ad49919b4addccbeba7161875fe9bfe93e3504cad40e37`.

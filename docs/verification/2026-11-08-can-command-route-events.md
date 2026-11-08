# CAN command route readiness events

The STM32 gateway admits configuration and action commands after the first
accepted heartbeat or input on the current CAN assignment. The publication
stream now tracks that readiness separately from whether the peripheral has
provided any input. The first accepted frame emits a sequenced change.
Repeated heartbeats stay quiet when other publication evidence is unchanged.
A replaced assignment needs its own accepted frame.

Cockpit waits for this change after a route rejection on an active feed
whose firmware advertises `configurationRouteEvents`. Action discovery and
active configuration reads refresh their gateway evidence before retrying.
The waiter also notices a change that arrives before the rejection. When
publication coverage is unavailable, command recovery retains its bounded
retry. Configuration activation after a write still has its own confirmation
wait. Command mutations stay single-dispatch and skip the route evidence
lookup used by the read waiter.

## Firmware and wire contract

- Gateway firmware `dfd83084bf8a48a40d4db15b59839978b7674b9b`.
- Protocol `455ebd69009853f21f78bbfeb11aa45b139a1214`.
- Four gateway suites passed in both sanitizer and coverage builds. All six
  firmware line-coverage gates passed. Gateway publication reached 96.65%
  and the Cockpit wire formatter reached 95.81%.
- All six Windows native CMake tests passed. Four production C vectors
  advertised `configurationRouteEvents: true`.
- All eight sanitized POSIX simulator tests passed with fixture state on the
  WSL filesystem. The initial run on the Windows mount failed before protocol
  exercise because its flash lock requires Unix owner and mode checks.
- The STM32G0B1CB compile-only build used 70,608 bytes of flash and 119,344
  bytes of RAM. ELF SHA-256
  `78a0443a88917f8dc20b637ff7d76ab4f0c1ead2d3c95b826d6d65dc520a6898`.

## Cockpit

Three native serial regressions verified a quiet wait without retry queries,
an event before the rejected action read, and refusal to retry a configuration
read after the expected peripheral boot changed. The successful configuration
case also verified the accepted active readback reached the native cache.

The rebuilt native dev Cockpit passed workspace baseline, USB preview and USB
metadata subscription checks in a private database. Dev executable SHA-256
`f5ff8757479a094c3f18fa6f1a930f3d5bb1b2246c560c91237d38cb22bd4187`.
All 27 workspace frontend tests passed with 99.22% lines, 89.51% branches
and 89.19% functions covered.

The full native run recorded 2,455 passes, 18 default ignores and one failure
in the existing offline companion reassignment fixture. It received an
unassigned-companion error during reconnect. The same executable passed the
case when run individually. Both pinned DCS archive checks passed separately.
The 92 selected STM32 tests all passed and supplied the changed-module
coverage because the failing full run did not flush its parent counters.

| Module | Production lines | LLVM functions |
| --- | ---: | ---: |
| STM32 gateway | 89.33% | 91.96% |
| Recovery waits | 95.70% | 90.91% |

Physical gateway and board qualification remain tracked in #199.

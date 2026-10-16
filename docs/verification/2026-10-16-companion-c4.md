# Companion C4 implementation verification — 16 October 2026

Issue: [#42](https://github.com/opensimgear/cockpit/issues/42).

## Delivered behavior

The Mappings page now has a dedicated companion controller section. It derives
roles, target identities, indices, axis ranges and fallbacks from the current
trusted immutable publication. Each retained instance can save recording-only or
exact local output assignments, reconcile a changed publication, inspect native
preflight, Start, Stop, recover and inspect a bounded report log. Two instances
of the same UI retain different profile/target identities.

A coordinated workspace bounds eight companion routes, with cumulative limits
of 16 source roles, eight targets and 256 nodes shared with the ordinary mapping.
Each companion owns its worker; slow output I/O does not execute on another
route's clock. Native slot reservations include preparing sessions and pending
recovery. Recording does not acquire or reserve a native output slot.

Live Start checks explicit enablement, current authorization/publication pin,
fresh complete neutral baseline, exact target shape/usages/fallback, backend
qualification/readiness, inventory, shared budgets and recovery before
acquisition. Authority, source generation and cancellation are checked again
before adoption/first flush. Stop supersedes queued and in-flight Start. Source
loss latches a route disarmed until explicit Start; a new baseline cannot revive
an old generation or revision. Separate durable journals preserve each route's
recovery boundary. Configuration restoration does not activate input or output.

The ordinary mapping path retains its runtime and participates in shared
reservations. Per-instance request queues and frontend operation fences allow
Stop and other instances to proceed while a route request is pending. Pairing
QR replies use compact binary rows; they preserve every module and fit the
existing bounded IPC response instead of increasing transport limits.

## Local validation

Validation used a clean archive of `72c6526` plus the owned C4 changes, copied to
isolated Windows and WSL/Linux workspaces. Unrelated changes from other chats
were excluded. Existing native dependency targets were reused; no installed
controller configuration, HidHide policy or real game was changed.

- Root frontend: Svelte/TypeScript checks, all transport/regression tests, all
  existing frontend coverage gates and production build passed. Transport has
  165 passing cases, including 19 gateway queue tests. Mappings has 18 passing
  cases; `mappings.ts` has 100% lines/branches/functions and
  `companion-mappings.ts` has 100% lines/functions and 97.45% branches.
- Companion workspace: contracts/designer/renderer/app checks, tests, coverage
  and production renderer build passed: 64 contracts, 61 designer, 16 renderer,
  12 library, four host-helper and 21 app tests. All existing coverage gates pass.
- Native host library: Windows 321 passed/one ignored, Linux 332 passed/one
  ignored, using serial test execution to avoid unrelated shared UDP-port races.
  The ignored test requires the separately downloaded official DCS-BIOS archive.
  Windows native host and private gateway service compiled successfully.
- Companion service: 47 tests passed. All eight measured service modules exceed
  71% production line coverage; `hub.rs` is 91.46%. Local desktop operator
  coverage is 98.05%, including the QR framing and trusted publication lookup.
- Controller-input coverage passed on Windows; every measured module exceeds
  71%. The existing macOS launch-test race was made deterministic with a fixture
  channel gate; the focused Linux launch suite passed four tests.
- Mapping-host: 65 tests passed on Windows and Linux (14 new C4 regressions;
  the concurrent live-feeder fixture exercises Windows only). All eight measured
  modules exceed 71% production line coverage: route manager 91.79%, companion
  source 97.70%, router 87.18%, service 75.69%, game 86.34%, learn 94.19%, settings
  97.60% and panels 100%. Maximum-shape requests and byte-paged recording replies
  fit the unchanged bounded IPC envelope; cursor advancement cannot skip omitted
  reports. Corrupt and orphaned journals block live admission and remain visible
  as incomplete recovery; unrelated recording remains available.
- Python WSS acceptance helpers: five tests passed.

The ordinary native mapping acceptance also passed with the installed Windows
driver: the feeder rejected empty targets before acquisition, production gates
remained false, and ordinary synthetic recording/restart behavior was preserved.

The independent private-service/WSS harness passed nine checks with three
simulated authenticated peers on one loopback TLS listener. It exercised two
same-UI flight instances and a distinct racing panel through real pairing,
authorization, assignment, baseline and input envelopes, without a production
fixture injection command. Exact button/axis/hat reports and the racing panel's
nonzero fallback (0.25 → 8192) were observed. Per-instance Stop, revocation,
wire Stop, reconnect and lease loss remained isolated while an ordinary
synthetic recording stayed active. Restart retained intended descriptors and
assignments with listener/mappings disarmed. Live Start was rejected with
production qualification/enablement still false.

This harness used simulated peers, not native companion installations. It sent
no virtual reports and changed no driver layout. Its TLS CA came through private
operator IPC; native client pin/bootstrap qualification is separate. Recording
and injected feeder reports establish software behavior, not game receipt.
The CI workflow now runs this same recording acceptance on Windows and retains
its evidence alongside controller coverage.

## Platform and qualification boundaries

Browser and mobile companion clients share these mapping semantics. Their OS
does not determine the selected host output backend. Windows output stays
unqualified pending #54; Linux/macOS host output remains deferred in #48/#49.
Multiple live feeder processes on non-Windows hosts are explicitly refused
until their backend ownership model is qualified. Recording remains available.
No output, provisioning, hiding, live-script or mobile-authentication gate was
enabled by C4, and no test feeder can be selected by production IPC.

[#66](https://github.com/opensimgear/cockpit/issues/66) retains real-game dual
virtual-controller delivery, real fallback/ownership/failure recovery, game
binding retention and whole-host termination. Mobile installation, protected stores,
LAN and lifecycle remain #55/#56; mixed-client performance/load remains #58. No missing manual or
physical check is marked passed. C5 (#43) and C6 (#44) remain Ready.

## Remote evidence boundary

Historical desktop package compilation records are retired after the browser migration. Android compilation remains in the mobile workflow. The local host checks above are software evidence and do not establish physical browser/mobile qualification.

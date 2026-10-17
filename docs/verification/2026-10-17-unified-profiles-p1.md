# Unified profiles P1 verification — 17 October 2026

Scope: [Profiles P1 #81](https://github.com/opensimgear/cockpit/issues/81).
The implementation composes provider-owned discovery into one native resource
inventory. It adds direct OSG USB commissioning and lossless typed descriptors;
it does not activate a profile or add the profile editor.

## Contract and identity evidence

The pure resource registry tests cover persistent local identities, independent
identical controllers, gateway-scoped unproven module identities, verified
USB/CAN/HID correlation, conflicting simultaneous routes, stale operation tokens,
partial catalogues, retained offline descriptors, restart sessions and atomic
metadata persistence. Complete catalogues are exercised with 20,000 ports.

Native provider tests exercise actual controller and companion descriptor APIs,
published companion feedback contracts and native transport evidence, dormant
game catalogues, CAN route/configuration-generation changes, USB capability
markers, bounded pagination, failed persistence followed by retry and corrupt
metadata preservation. Reads do not initialize companion trust services, start
integrations or acquire configuration ownership.

Direct USB tests use the production line codec, worker and Fleet with controlled
serial endpoints. They cover explicit generic Windows port selection, strict
automatic probing, complete identity handshake, claims, Apply/Save/Discard,
read-only reconciliation after uncertain replies, typed actions, ordered event
reads, stale routes, reconnects, duplicate serials and bounded queues/replies.
Fixtures are separate from physical firmware/device qualification.

The firmware identity formatter dependency is merged in
[device-template #16](https://github.com/opensimgear/device-template/pull/16)
(`d9bb3095fd0cbc516723a78365ad49b05009e9eb`). Its C tests cover the actual formatter
at maximum legal lengths, truncation, exact capacity and output-only layouts.
No firmware was flashed as part of this work.

## Recorded checks

- Pure profile library: **52 tests passed**. Production line coverage for the
  added registry/port/store modules: **94.95%, 92.96%, 92.55%** respectively. All
  existing profile modules retain their 71% per-module gate.
- Frontend inventory client: **11 tests passed**, 100% lines/functions and
  99.52% branches. The client preserves exact port contracts and rejects mixed
  sessions, revisions, generations and capability pages.
- Frontend USB client: **11 tests passed**, 100% lines, 97.60% branches and
  96.88% functions. Mutations are never retried; Disconnect cancels unsent work.
- Gateway transport regressions: **180 tests passed**, including per-endpoint
  serialization and independent inventory queues.
- Windows desktop library: **466 tests passed**, with three existing live
  hardware/game tests ignored. The native service framing unit and integration
  tests also passed.
- Controller and mapping-host coverage suites: **70 and 68 tests passed**. Every
  existing measured module exceeds its unchanged gate; the controller registry
  is 99.76% and companion source provider is 97.74%.
- The complete companion coverage gate passes; the native host is **73.89%** and
  all twelve measured production modules exceed the unchanged 71% gate.
- Svelte/browser checks and UI/browser production builds passed. These checks
  validate client contracts; profile list and editor UI follow in P6/P7.

The native USB/inventory coverage runner gates each of its seven production
modules at 71%, using instrumented production code and separate test files. Its
source-stamped summary, annotated lines and test log are retained as CI artifacts.

| Native production module | Covered lines | Coverage |
| --- | --- | --- |
| Module console codec | 279 / 292 | 95.55% |
| Standalone USB worker | 710 / 810 | 87.65% |
| USB Fleet | 578 / 606 | 95.38% |
| Configuration route policy | 135 / 135 | 100.00% |
| Inventory runtime | 213 / 218 | 97.71% |
| Provider composition | 531 / 578 | 91.87% |
| Port translation | 262 / 262 | 100.00% |

The integrated regression suite includes private configuration-owner mirrors,
explicit release before switching between USB and CAN, concurrent same-client
claims, boot/lease invalidation and writer pins retained until worker closure.
Action-only CAN modules with explicitly unsupported event capture retain their
advertised action ports without inventing an input baseline or repeated probes.
Native checks requiring DPAPI/process facilities run in the Windows user context.
Remote cross-platform CI results are recorded in the pull request.

## Remaining boundaries

P2 extends the portable graph to represent structured action arguments, constrained
text/RGB, explicit transformations and virtual layouts without a lossy scalar
projection. P3/P4 supply verified game context and runtime admission. P6/P7 supply
the Profiles list, Add flow, navigation and visual editor.

Existing standalone USB and identity issues #3/#4 retain their own acceptance
scope. Recording endpoints and authored fixtures do not establish Windows driver,
physical device, game or companion qualification; #50–#54 and #66/#67 remain
separately tracked. Linux/macOS activation gates remain in force.

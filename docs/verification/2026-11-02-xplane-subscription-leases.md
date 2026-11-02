# X-Plane subscription lease foundation — 2 November 2026

This is one prerequisite of R3 (#192), under #188. It does not complete the
shared, event-owned X-Plane producer or qualify a real simulator.

## Implementation

The RREF subscription planner unions independent reader leases with permanent
session guards. A shared dataref path has one active wire index. Replacing or
removing an observer lease preserves indices required by another lease. Removed
indices never become active again, including when the same path is selected
later. Context replacement allocates fresh indices for the complete active set.
Allocation exhaustion fails atomically rather than wrapping or reusing indices.

Limits are 64 reader leases, 1,024 selected dataref paths in their union, positive
signed 32-bit wire indices, and 399 bytes per dataref path. The active index map
removes retired entries, so selection churn does not retain an index history.

The current adapter now uses profile and inspection leases. Changing an
inspection page queues only the changed wire subscriptions, retaining native
values, timestamps, guard evidence and context for unchanged datarefs. New
components begin unknown. Aliases of one dataref share its report. Valid rows
from retired indices are ignored even when mixed with healthy rows in a UDP
datagram; malformed or duplicate rows invalidate the report before any ingest.

Selection validation and queue admission are transactional. Selecting a page
commits a requested selection, not confirmation of UDP delivery. Subscription
dispatch retains its existing bounded queue and protocol pacing. A context
fence that cannot be queued withdraws the reader with an explicit terminal
error requiring reactivation. The adapter is split into focused files below
500 lines; coverage includes its production `include!` fragments.

## Software verification

The isolated staged source excludes unrelated profile/editor work in the shared
workspace. Tested tree before adding this document:
`64a5d154ec4d44e2d83cef293121a118da1a7d1e`.

- Complete native library: **1,722 passed**, zero failed, 12 existing opt-in
  ignores; `.research/reactive/r3-xplane-selection-all-native.log`.
- Focused instrumented tests: **24 passed**, zero failed, one existing installed
  SDK check ignored; `r3-xplane-selection-coverage-final/native-tests.log`.
- Logical X-Plane adapter: **469/543 production lines, 86.37%**. New lease
  planner separately: **196/213 production lines, 92.02%**. Both exceed the
  unchanged 71% floor; `r3-xplane-selection-coverage-final.log` and its
  `integration-summary.json` retain the measurements.
- Real loopback UDP regressions cover unchanged profile continuity during page
  changes, unknown newly selected values, retired/reselected indices, mixed
  retired/current packets, alias fan-out, malformed report atomicity, bounded
  pending requests, context fencing and explicit terminal failure.
- Pure planner regressions cover overlapping leases, observer isolation,
  bounds, transactional rejection, removed index lookup and checked exhaustion.

The native executable built from these sources has SHA-256
`dc9be2b5208e954b8e9417bf50f37cb521f430b2ca35e7c33aa22ac6e023bb85`.

## Running dev Cockpit

The final `xplane-telemetry-native.mjs` run passed in **159.33 seconds** using
the actual native executable and authored loopback RREF publishers. Both
X-Plane 11 and 12 private stand-in processes used a full installed SDK numeric
catalogue, with a version header changed only in the private copy. The fixture
does not launch a real simulator or drive physical outputs.

- Complete metadata and on-demand passive selection worked for both versions.
  Inspection replacement issued **zero simulator-time guard cancellations**.
- Selected scalar/array physics, aliases and canonical channels reached native
  profile processing with their values and units preserved.
- Partial data expiry isolated missing channels. Pause, frozen simulator clock,
  silent UDP and process exit withdrew values; fresh advancing data recovered
  the permitted scenarios.
- Requests stayed within the fixture's existing subscription count, update-rate
  and 100-requests-per-100-ms protocol pacing checks.
- Both private processes/profiles/installations were removed. The original
  X-Plane 12 installation was retained exactly and its provider configuration
  restored; no cleanup errors or added scan roots were recorded.

Evidence: `.research/xplane-telemetry-native/1793644298933/evidence.json` and
`.research/reactive/r3-xplane-selection-native-fixture-final.log`. The test now
uses current supported game registration, installation selection and scoped
discovery-removal APIs. An initial test-only lookup failed before process launch
because it omitted Windows' extended-path prefix normalization; that private
registration was removed before the passing final run. It is not counted as a
pass. The regression suites and coverage above used the final production code.

## Remaining R3 work

Profiles and vehicle inspection still construct separate X-Plane source
instances. UDP receive, subscription scheduling and session expiry still run
from the existing `poll` path, and process/exclusivity checks still use the
existing polling monitor. Shutdown still uses its existing paced cancellation.
This change supplies stable lease semantics for the upcoming shared owner; it
does not relabel these remaining loops as event-driven.

The next owner must share acquisition by exact process/setup/connection contract,
publish typed native commits with original per-value capture times, and wait on
socket readiness, cancellation, process events and actual protocol/freshness
deadlines. Adding an inspector must not reset a running profile. Native Windows
process exit waits are available in the repository; process-start notification
support needed for exclusivity still requires implementation.

Real simulator effects, physical controller latency, controller preview latency,
other platforms and the combined soak remain unqualified. User-operated
acceptance remains in #199, first in the progress lane. No hardware or simulator
qualification flag changes in this milestone.
